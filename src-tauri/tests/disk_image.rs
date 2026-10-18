//! End-to-end check against a mounted disk image (no real card needed).
//! Run with: CHDK_TEST_DEVICE=diskNs1 cargo test --test disk_image -- --ignored --nocapture

use chdk_sd_creator_lib::{autobuild, cameras, disks, install};
use std::path::Path;

#[test]
#[ignore]
fn install_on_disk_image() {
    let device = std::env::var("CHDK_TEST_DEVICE").expect("set CHDK_TEST_DEVICE");
    let cards = disks::list_cards().unwrap();
    let card = cards
        .iter()
        .find(|c| c.device == device)
        .expect("image not listed as a card");
    println!("card: {card:?}");
    assert_eq!(card.protocol, "Disk Image");
    assert!(card.formattable);

    let cam = cameras::find("ixus90_sd790").unwrap();
    let builds =
        tauri::async_runtime::block_on(autobuild::list_builds(&cam.id, autobuild::Channel::Stable))
            .unwrap();
    for fw in &cam.firmwares {
        assert!(
            builds.iter().any(|b| &b.firmware == fw && b.full),
            "no full build for {fw}"
        );
    }
    let build = builds
        .iter()
        .find(|b| b.firmware == "100c" && b.full)
        .unwrap();
    let cache = std::env::temp_dir().join("chdk-sd-creator-test");
    let zip =
        tauri::async_runtime::block_on(autobuild::download(build, &cache, |_, _| {})).unwrap();

    let n = install::extract(&zip, Path::new(&card.mount_point), |_, _| {}).unwrap();
    println!("extracted {n} entries from {}", build.file_name);
    for f in ["DISKBOOT.BIN", "PS.FI2", "CHDK/MODULES/lua.flt"] {
        assert!(Path::new(&card.mount_point).join(f).exists(), "{f} missing");
    }
    install::write_ver_req(Path::new(&card.mount_point), &cam.ver_req).unwrap();
    assert!(disks::card(&device).unwrap().has_chdk);

    // Formatting wipes it again.
    disks::format(&device, disks::FormatLayout::Fat32).unwrap();
    let after = disks::list_cards().unwrap();
    let c = after
        .iter()
        .find(|c| c.whole_disk == card.whole_disk)
        .expect("card gone after format");
    println!("after FAT32 format: {c:?}");
    assert_eq!(c.fat, "FAT32");
    assert!(!c.has_chdk);
    disks::format(&c.device, disks::FormatLayout::Fat16).unwrap();
    let after = disks::list_cards().unwrap();
    let c = after
        .iter()
        .find(|c| c.whole_disk == card.whole_disk)
        .expect("card gone after format");
    println!("after FAT16 format: {c:?}");
    assert_eq!(c.fat, "FAT16");
}

/// Read-only: print what the card list would show.
#[test]
#[ignore]
fn print_cards() {
    for c in disks::list_cards().unwrap() {
        println!(
            "{} {} {} others={:?}",
            c.device, c.name, c.fat, c.other_partitions
        );
    }
}

/// Two-partition card (FAT16 boot + FAT32 data), like a big card formatted for autoboot.
/// Run with CHDK_TEST_DEVICE=diskNs1 (the FAT16 partition).
#[test]
#[ignore]
fn two_partition_card() {
    let device = std::env::var("CHDK_TEST_DEVICE").expect("set CHDK_TEST_DEVICE");
    let card = disks::card(&device).unwrap();
    println!("{card:?}");
    assert_eq!(card.fat, "FAT16");
    let data = card
        .data_partition()
        .expect("no FAT32 data partition found")
        .clone();
    assert_eq!(data.fat, "FAT32");
    // The second partition must not be offered as a card of its own.
    assert!(disks::list_cards()
        .unwrap()
        .iter()
        .all(|c| c.device != data.device));

    let cam = cameras::find("ixus90_sd790").unwrap();
    assert!(cam.multipart);
    let builds =
        tauri::async_runtime::block_on(autobuild::list_builds(&cam.id, autobuild::Channel::Stable))
            .unwrap();
    let build = builds
        .iter()
        .find(|b| b.firmware == "100c" && b.full)
        .unwrap();
    let cache = std::env::temp_dir().join("chdk-sd-creator-test");
    let zip =
        tauri::async_runtime::block_on(autobuild::download(build, &cache, |_, _| {})).unwrap();
    for mount in [&card.mount_point, &data.mount_point] {
        install::extract(&zip, Path::new(mount), |_, _| {}).unwrap();
        let m = Path::new(mount);
        assert!(
            m.join("CHDK/MODULES/FSELECT.FLT").exists(),
            "FSELECT.FLT missing on {mount}"
        );
        let leftovers: Vec<_> = std::fs::read_dir(m.join("CHDK/MODULES"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("._"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "AppleDouble files left: {leftovers:?}"
        );
    }
}

/// Prints the Full Disk Access probe result for whatever launched this test.
#[test]
#[ignore]
fn print_full_disk_access() {
    println!(
        "full disk access: {:?}",
        chdk_sd_creator_lib::full_disk_access_status()
    );
}
