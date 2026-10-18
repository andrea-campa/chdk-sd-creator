// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `<app> --patch-bootsector /dev/rdiskNsM` is how the app re-runs itself
    // with administrator rights to write the boot sector; no UI is started.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(chdk_sd_creator_lib::PATCH_FLAG) {
        let Some(path) = args.get(2) else {
            eprintln!("usage: {} {} <device>", args[0], chdk_sd_creator_lib::PATCH_FLAG);
            std::process::exit(2);
        };
        match chdk_sd_creator_lib::bootsector::patch_device(path) {
            Ok(kind) => {
                println!("{}", if kind == chdk_sd_creator_lib::bootsector::FatKind::Fat32 { "FAT32" } else { "FAT16" });
            }
            Err(e) => {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
        }
        return;
    }
    chdk_sd_creator_lib::run()
}
