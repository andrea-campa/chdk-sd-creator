//! macOS implementation, built on `diskutil`.

use super::{Card, FormatLayout, Partition, FAT16_MAX};
use crate::bootsector::{self, FatKind};
use anyhow::{anyhow, bail, Context, Result};
use plist::{Dictionary, Value};
use std::path::Path;
use std::process::Command;

/// Refuse to format anything bigger than this: it is not an SD card.
const MAX_FORMAT_SIZE: u64 = 256_000_000_000;

fn run(cmd: &str, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("running {cmd}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = String::from_utf8_lossy(&out.stdout);
        bail!(
            "{cmd} {}: {}",
            args.join(" "),
            if err.trim().is_empty() { msg } else { err }.trim()
        );
    }
    Ok(out.stdout)
}

fn diskutil_plist(args: &[&str]) -> Result<Dictionary> {
    let out = run("diskutil", args)?;
    Value::from_reader(std::io::Cursor::new(out))?
        .into_dictionary()
        .ok_or_else(|| anyhow!("unexpected diskutil output"))
}

fn s(d: &Dictionary, k: &str) -> String {
    d.get(k)
        .and_then(Value::as_string)
        .unwrap_or_default()
        .to_string()
}
fn b(d: &Dictionary, k: &str) -> bool {
    d.get(k).and_then(Value::as_boolean).unwrap_or(false)
}
fn n(d: &Dictionary, k: &str) -> u64 {
    d.get(k).and_then(Value::as_unsigned_integer).unwrap_or(0)
}

fn check_device_name(dev: &str) -> Result<()> {
    let ok = dev.strip_prefix("disk").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .split('s')
                .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
    });
    if !ok {
        bail!("invalid device name '{dev}'");
    }
    Ok(())
}

/// All partition `Content` types on a whole disk are FAT-like.
fn all_partitions_fat(whole: &str) -> Result<bool> {
    let list = diskutil_plist(&["list", "-plist", whole])?;
    let disks = list
        .get("AllDisksAndPartitions")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("unexpected diskutil list output"))?;
    let Some(disk) = disks.first().and_then(Value::as_dictionary) else {
        return Ok(false);
    };
    let fat = |c: &str| c.contains("FAT") || c == "Microsoft Basic Data";
    match disk.get("Partitions").and_then(Value::as_array) {
        Some(parts) if !parts.is_empty() => Ok(parts
            .iter()
            .filter_map(Value::as_dictionary)
            .all(|p| fat(&s(p, "Content")))),
        // Filesystem directly on the whole disk ("superfloppy").
        _ => Ok(fat(&s(disk, "Content"))),
    }
}

fn card_from_info(info: &Dictionary) -> Result<Option<Card>> {
    if s(info, "FilesystemType") != "msdos" {
        return Ok(None);
    }
    let protocol = s(info, "BusProtocol");
    let removable = b(info, "RemovableMedia")
        || b(info, "RemovableMediaOrExternalDevice")
        || protocol == "Secure Digital"
        || protocol == "Disk Image";
    if !removable {
        return Ok(None);
    }
    let fs_name = s(info, "FilesystemName");
    let fat = ["FAT12", "FAT16", "FAT32"]
        .into_iter()
        .find(|f| fs_name.contains(f))
        .unwrap_or("FAT")
        .to_string();
    let device = s(info, "DeviceIdentifier");
    let whole_disk = s(info, "ParentWholeDisk");
    let mount_point = s(info, "MountPoint");
    let whole = diskutil_plist(&["info", "-plist", &whole_disk])?;
    let disk_size = n(&whole, "Size").max(n(&whole, "TotalSize"));
    // Built-in SD readers report Internal=true, so allow them by protocol.
    let not_internal = !b(&whole, "Internal") || protocol == "Secure Digital";
    let formattable = not_internal
        && disk_size > 0
        && disk_size <= MAX_FORMAT_SIZE
        && all_partitions_fat(&whole_disk)?;
    Ok(Some(Card {
        has_chdk: Path::new(&mount_point).join("DISKBOOT.BIN").exists(),
        locked: !b(info, "WritableVolume"),
        name: s(info, "VolumeName"),
        size: n(info, "TotalSize").max(n(info, "Size")),
        free: n(info, "FreeSpace"),
        device,
        whole_disk,
        mount_point,
        fat,
        disk_size,
        protocol,
        formattable,
        other_partitions: Vec::new(),
    }))
}

pub fn list_cards() -> Result<Vec<Card>> {
    let mut cards = Vec::new();
    for entry in std::fs::read_dir("/Volumes")? {
        let path = entry?.path();
        let Some(p) = path.to_str() else { continue };
        // Volumes diskutil can't describe (network shares, …) are skipped.
        let Ok(info) = diskutil_plist(&["info", "-plist", p]) else {
            continue;
        };
        if let Some(card) = card_from_info(&info)? {
            cards.push(card);
        }
    }
    // The camera only reads the first partition, so offer one entry per card.
    let is_first = |c: &Card| c.device == c.whole_disk || c.device == format!("{}s1", c.whole_disk);
    let (mut firsts, rest): (Vec<Card>, Vec<Card>) = cards.into_iter().partition(is_first);
    let part_no = |dev: &str| {
        dev.rsplit('s')
            .next()
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(0)
    };
    for card in &mut firsts {
        let mut others: Vec<&Card> = rest
            .iter()
            .filter(|o| o.whole_disk == card.whole_disk)
            .collect();
        others.sort_by_key(|o| part_no(&o.device));
        card.other_partitions = others
            .into_iter()
            .map(|o| Partition {
                device: o.device.clone(),
                name: o.name.clone(),
                mount_point: o.mount_point.clone(),
                fat: o.fat.clone(),
                free: o.free,
            })
            .collect();
    }
    firsts.sort_by(|a, b| a.device.cmp(&b.device));
    Ok(firsts)
}

/// Re-check a device the UI handed us: it must still be a FAT card.
pub fn card(device: &str) -> Result<Card> {
    check_device_name(device)?;
    // Going through the full listing also re-applies its rules (first partition only).
    list_cards()?
        .into_iter()
        .find(|c| c.device == device)
        .ok_or_else(|| anyhow!("{device} is not the first FAT partition of a removable card"))
}

/// Like [`card`], but refuses a locked card.
pub fn writable_card(device: &str) -> Result<Card> {
    let card = card(device)?;
    if card.locked {
        bail!("“{}” is locked. Slide the card’s lock switch up (towards the contacts), then reinsert it.", card.name);
    }
    Ok(card)
}

fn is_permission_denied(e: &anyhow::Error) -> bool {
    e.chain()
        .filter_map(|c| c.downcast_ref::<std::io::Error>())
        .any(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', r"\\").replace('"', "\\\""))
}

/// Re-run this executable as root (via the standard macOS password prompt)
/// just to patch the boot sector.
fn patch_elevated(raw: &str) -> Result<FatKind> {
    let exe = std::env::current_exe()?;
    let shell = format!(
        "{} {} {}",
        sh_quote(&exe.to_string_lossy()),
        crate::PATCH_FLAG,
        sh_quote(raw)
    );
    let script = format!(
        "do shell script {} with administrator privileges with prompt {}",
        applescript_string(&shell),
        applescript_string("CHDK SD Creator needs your password to mark the SD card as bootable.")
    );
    let out = Command::new("osascript").args(["-e", &script]).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("-128") {
            bail!("password prompt was cancelled; the card was not made bootable");
        }
        if err.contains("Operation not permitted") {
            bail!(
                "macOS blocked raw access to the card. Give CHDK SD Creator Full Disk Access, \
                 then run Install again. The CHDK files are already on the card."
            );
        }
        bail!("could not patch boot sector: {}", err.trim());
    }
    match String::from_utf8_lossy(&out.stdout).trim() {
        "FAT32" => Ok(FatKind::Fat32),
        _ => Ok(FatKind::Fat16),
    }
}

pub fn make_bootable(device: &str) -> Result<FatKind> {
    let card = writable_card(device)?;
    let whole = format!("/dev/{}", card.whole_disk);
    let raw = format!("/dev/r{}", card.device);
    // Unmount every partition: while any is mounted (FSKit holds the whole
    // device open), macOS refuses raw writes even as root.
    run("diskutil", &["unmountDisk", &whole])?;
    let result = bootsector::patch_device(&raw).or_else(|e| {
        if is_permission_denied(&e) {
            patch_elevated(&raw)
        } else {
            Err(e)
        }
    });
    // Remount even if patching failed, so the card doesn't vanish.
    let remount = run("diskutil", &["mountDisk", &whole]);
    let kind = result?;
    remount?;
    Ok(kind)
}

pub fn format(device: &str, layout: FormatLayout) -> Result<()> {
    let card = writable_card(device)?;
    if !card.formattable {
        bail!(
            "{} is not a disk this app is willing to format",
            card.whole_disk
        );
    }
    let whole = format!("/dev/{}", card.whole_disk);
    match layout {
        FormatLayout::Fat32 => {
            run(
                "diskutil",
                &["eraseDisk", "FAT32", "CANON_DC", "MBRFormat", &whole],
            )?;
        }
        FormatLayout::Fat16 if card.disk_size <= FAT16_MAX + FAT16_MAX / 20 => {
            run(
                "diskutil",
                &["eraseDisk", "MS-DOS FAT16", "CANON_DC", "MBRFormat", &whole],
            )?;
        }
        FormatLayout::Fat16 => {
            run(
                "diskutil",
                &[
                    "partitionDisk",
                    &whole,
                    "2",
                    "MBR",
                    "MS-DOS FAT16",
                    "CANON_DC",
                    "2000M",
                    "MS-DOS FAT32",
                    "EXTRA",
                    "R",
                ],
            )?;
        }
    }
    Ok(())
}

pub fn eject(device: &str) -> Result<()> {
    let card = card(device)?;
    run("diskutil", &["eject", &format!("/dev/{}", card.whole_disk)])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names() {
        assert!(check_device_name("disk4").is_ok());
        assert!(check_device_name("disk12s1").is_ok());
        assert!(check_device_name("disk4s").is_err());
        assert!(check_device_name("disk").is_err());
        assert!(check_device_name("/dev/disk4").is_err());
        assert!(check_device_name("disk4; rm").is_err());
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/a b/it's"), r"'/a b/it'\''s'");
        assert_eq!(applescript_string(r#"say "hi" \"#), r#""say \"hi\" \\""#);
    }
}
