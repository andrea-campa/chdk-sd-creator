//! Unpacking a CHDK build onto a card.

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::path::{Path, PathBuf};

/// Check that a zip opens and looks like a CHDK build.
pub fn validate_zip(path: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(path)?).context("not a valid zip")?;
    if zip.by_name("DISKBOOT.BIN").is_err() {
        bail!("{} does not contain DISKBOOT.BIN", path.display());
    }
    Ok(())
}

/// Extract every entry of `zip_path` into `dest`, overwriting existing files.
/// Existing user files (settings, scripts you added) are left alone.
pub fn extract(zip_path: &Path, dest: &Path, mut progress: impl FnMut(usize, usize)) -> Result<usize> {
    let mut zip = zip::ZipArchive::new(File::open(zip_path)?)?;
    let total = zip.len();
    for i in 0..total {
        let mut entry = zip.by_index(i)?;
        // enclosed_name rejects absolute paths and `..` components.
        let Some(rel) = entry.enclosed_name() else {
            bail!("unsafe path in archive: {}", entry.name());
        };
        let out: PathBuf = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = File::create(&out).with_context(|| format!("writing {}", out.display()))?;
            let written = std::io::copy(&mut entry, &mut f)?;
            f.sync_all()?;
            // macOS stores extended attributes on FAT as hidden "._name" files; drop them.
            if let Some(name) = out.file_name().and_then(|n| n.to_str()) {
                let _ = std::fs::remove_file(out.with_file_name(format!("._{name}")));
            }
            // Check what landed on the card, not just what we sent.
            let on_disk = std::fs::metadata(&out)?.len();
            if written != entry.size() || on_disk != entry.size() {
                bail!(
                    "{} was not copied completely ({on_disk} of {} bytes)",
                    rel.display(),
                    entry.size()
                );
            }
        }
        progress(i + 1, total);
    }
    Ok(total)
}

/// Write the empty file that makes the camera show its firmware version.
pub fn write_ver_req(mount: &Path, file_name: &str) -> Result<PathBuf> {
    let path = mount.join(file_name);
    File::create(&path).with_context(|| format!("creating {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extracts_and_validates() {
        let dir = std::env::temp_dir().join(format!("chdk-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("b.zip");
        {
            let mut w = zip::ZipWriter::new(File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("DISKBOOT.BIN", opts).unwrap();
            w.write_all(b"boot").unwrap();
            w.add_directory("CHDK/MODULES/", opts).unwrap();
            w.start_file("CHDK/MODULES/lua.flt", opts).unwrap();
            w.write_all(b"lua").unwrap();
            w.finish().unwrap();
        }
        validate_zip(&zip_path).unwrap();
        let card = dir.join("card");
        let n = extract(&zip_path, &card, |_, _| {}).unwrap();
        assert_eq!(n, 3);
        assert_eq!(std::fs::read(card.join("CHDK/MODULES/lua.flt")).unwrap(), b"lua");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
