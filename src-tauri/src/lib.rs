pub mod autobuild;
pub mod bootsector;
pub mod cameras;
pub mod disks;
pub mod install;

use autobuild::{Build, Channel};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::ipc::Channel as IpcChannel;
use tauri::Manager;

pub const PATCH_FLAG: &str = "--patch-bootsector";

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

#[tauri::command]
fn cameras() -> Vec<cameras::Camera> {
    cameras::all().to_vec()
}

#[tauri::command]
async fn builds(camera_id: String, channel: Channel) -> CmdResult<Vec<Build>> {
    autobuild::list_builds(&camera_id, channel).await.map_err(err)
}

#[tauri::command]
async fn cards() -> CmdResult<Vec<disks::Card>> {
    blocking(disks::list_cards).await
}

#[tauri::command]
async fn write_ver_req(device: String, camera_id: String) -> CmdResult<String> {
    blocking(move || {
        let cam = cameras::find(&camera_id)?;
        let card = disks::writable_card(&device)?;
        let path = install::write_ver_req(Path::new(&card.mount_point), &cam.ver_req)?;
        Ok(path.display().to_string())
    })
    .await
}

#[tauri::command]
async fn format_card(device: String, layout: disks::FormatLayout) -> CmdResult<()> {
    blocking(move || disks::format(&device, layout)).await
}

#[tauri::command]
async fn eject(device: String) -> CmdResult<()> {
    blocking(move || disks::eject(&device)).await
}

/// Open a web page or a card in Finder.
#[tauri::command]
fn open_external(target: String) -> CmdResult<()> {
    let allowed = target.starts_with("https://") || target.starts_with("/Volumes/");
    if !allowed {
        return Err(format!("refusing to open {target}"));
    }
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(&target).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Whether this app has Full Disk Access (needed to mark cards bootable).
/// `None` when it can't be told.
#[tauri::command]
fn full_disk_access() -> Option<bool> {
    full_disk_access_status()
}

pub fn full_disk_access_status() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        // These are only readable with Full Disk Access. Try several, as their
        // presence varies between macOS versions (the per-user TCC.db is gone in 27).
        let home = std::env::var("HOME").unwrap_or_default();
        let probes = [
            "/Library/Application Support/com.apple.TCC/TCC.db".to_string(),
            format!("{home}/Library/Application Support/com.apple.TCC/TCC.db"),
            format!("{home}/Library/Safari"),
            format!("{home}/Library/Mail"),
        ];
        for probe in probes {
            let path = std::path::Path::new(&probe);
            let result = if path.is_dir() {
                std::fs::read_dir(path).map(|_| ())
            } else {
                std::fs::File::open(path).map(|_| ())
            };
            match result {
                Ok(()) => return Some(true),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Some(false),
                Err(_) => continue, // not there on this system; try the next
            }
        }
    }
    None
}

/// Relaunch, so a newly granted Full Disk Access takes effect.
#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

/// Open System Settings at Privacy & Security → Full Disk Access.
#[tauri::command]
fn open_full_disk_access() -> CmdResult<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Select this app in Finder, so it can be dragged into the Full Disk Access list.
#[tauri::command]
fn reveal_app() -> CmdResult<()> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // .../CHDK SD Creator.app/Contents/MacOS/<exe> → the .app bundle
    let target = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .unwrap_or(&exe)
        .to_path_buf();
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("-R")
        .arg(&target)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Method {
    /// Menu > Firmware Update loads PS.FI2 by hand each time.
    Fi2,
    /// Locked card + BOOTDISK signature loads DISKBOOT.BIN at power-on.
    Autoboot,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallRequest {
    camera_id: String,
    firmware: String,
    channel: Channel,
    full: bool,
    device: String,
    method: Method,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum Progress {
    Step { text: String },
    Download { done: u64, total: Option<u64> },
    Extract { done: usize, total: usize },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallResult {
    build: Build,
    files: usize,
    bootable: bool,
    /// Second partition CHDK was also copied to.
    data_partition: Option<String>,
}

#[tauri::command]
async fn install(
    app: tauri::AppHandle,
    req: InstallRequest,
    on_progress: IpcChannel<Progress>,
) -> CmdResult<InstallResult> {
    let step = |t: &str| {
        let _ = on_progress.send(Progress::Step { text: t.to_string() });
    };
    let cam = cameras::find(&req.camera_id).map_err(err)?;
    if !cam.firmwares.contains(&req.firmware) {
        return Err(format!("{} has no CHDK build for firmware {}", cam.name, req.firmware));
    }
    let device = req.device.clone();
    let card = blocking(move || disks::writable_card(&device)).await?;

    // Check autoboot compatibility before touching anything.
    if req.method == Method::Autoboot && card.fat == "FAT32" && !cam.fat32_autoboot {
        return Err(format!(
            "The {} can only autoboot from a FAT16 card, but this card is FAT32. \
             Format it as FAT16 first, or use the Firmware Update method.",
            cam.name
        ));
    }

    step("Looking up the latest build…");
    let build = autobuild::list_builds(&cam.id, req.channel)
        .await
        .map_err(err)?
        .into_iter()
        .find(|b| b.firmware == req.firmware && b.full == req.full)
        .ok_or_else(|| format!("no {} build for firmware {}", cam.id, req.firmware))?;

    step(&format!("Downloading {}…", build.file_name));
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("builds");
    let zip = autobuild::download(&build, &cache, |done, total| {
        let _ = on_progress.send(Progress::Download { done, total });
    })
    .await
    .map_err(err)?;

    let zip_len = std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
    if card.free < zip_len * 4 {
        return Err("Not enough free space on the card.".into());
    }
    let data = if cam.multipart { card.data_partition().cloned() } else { None };
    if let Some(p) = &data {
        if p.free < zip_len * 4 {
            return Err(format!("Not enough free space on the “{}” partition.", p.name));
        }
    }

    step(&format!("Copying files to {}…", card.name));
    let mount = card.mount_point.clone();
    let progress = on_progress.clone();
    let zip2 = zip.clone();
    let files = blocking(move || {
        install::extract(&zip2, Path::new(&mount), |done, total| {
            let _ = progress.send(Progress::Extract { done, total });
        })
    })
    .await?;

    // On a two-partition card the camera ends up on the FAT32 partition,
    // so CHDK's files (modules, scripts, settings) must be there as well.
    if let Some(p) = &data {
        step(&format!("Copying files to {} (where the camera keeps photos and CHDK)…", p.name));
        let mount = p.mount_point.clone();
        let progress = on_progress.clone();
        let zip = zip.clone();
        blocking(move || {
            install::extract(&zip, Path::new(&mount), |done, total| {
                let _ = progress.send(Progress::Extract { done, total });
            })
        })
        .await?;
    }

    let mut bootable = false;
    if req.method == Method::Autoboot {
        step("Marking the card as bootable (you may be asked for your password)…");
        let device = card.device.clone();
        blocking(move || disks::make_bootable(&device)).await?;
        bootable = true;
    }

    step("Done.");
    Ok(InstallResult { build, files, bootable, data_partition: data.map(|p| p.name) })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            cameras,
            builds,
            cards,
            write_ver_req,
            format_card,
            eject,
            open_external,
            open_full_disk_access,
            full_disk_access,
            restart_app,
            reveal_app,
            install
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
