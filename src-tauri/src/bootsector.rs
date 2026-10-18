//! Making a FAT volume bootable for CHDK.
//!
//! Canon PowerShot firmware loads DISKBOOT.BIN at power-on when the card is
//! write-protected and the volume boot sector carries the "BOOTDISK"
//! signature: offset 0x40 on FAT12/16, offset 0x1E0 on FAT32.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

pub const SIGNATURE: &[u8; 8] = b"BOOTDISK";
const SECTOR: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FatKind {
    Fat16,
    Fat32,
}

impl FatKind {
    pub fn signature_offset(self) -> usize {
        match self {
            FatKind::Fat16 => 0x40,
            FatKind::Fat32 => 0x1E0,
        }
    }
}

/// Identify the FAT flavour from a volume boot sector.
pub fn detect(sector: &[u8]) -> Result<FatKind> {
    if sector.len() < SECTOR || sector[510] != 0x55 || sector[511] != 0xAA {
        bail!("not a FAT boot sector (missing 55 AA signature)");
    }
    if &sector[0x52..0x5A] == b"FAT32   " {
        return Ok(FatKind::Fat32);
    }
    // FAT12 boots like FAT16: same extended BPB layout.
    if &sector[0x36..0x3A] == b"FAT1" {
        return Ok(FatKind::Fat16);
    }
    bail!("unrecognised filesystem (only FAT12/16/32 can be made bootable)");
}

pub fn is_bootable(sector: &[u8]) -> Result<bool> {
    let off = detect(sector)?.signature_offset();
    Ok(&sector[off..off + 8] == SIGNATURE)
}

/// Write the signature into an in-memory boot sector.
pub fn patch(sector: &mut [u8]) -> Result<FatKind> {
    let kind = detect(sector)?;
    let off = kind.signature_offset();
    sector[off..off + 8].copy_from_slice(SIGNATURE);
    Ok(kind)
}

/// Patch the boot sector of a block device (or image file) in place.
/// Raw devices need whole-sector I/O, so read, patch and write 512 bytes.
pub fn patch_device(path: &str) -> Result<FatKind> {
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("opening {path}"))?;
    let mut sector = [0u8; SECTOR];
    f.read_exact(&mut sector).context("reading boot sector")?;
    let kind = patch(&mut sector)?;
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&sector).context("writing boot sector")?;
    // Raw (character) devices are unbuffered and reject the F_FULLFSYNC
    // ioctl behind sync_all ("Inappropriate ioctl for device").
    #[cfg(unix)]
    let is_char_device = {
        use std::os::unix::fs::FileTypeExt;
        f.metadata()?.file_type().is_char_device()
    };
    #[cfg(not(unix))]
    let is_char_device = false;
    if !is_char_device {
        f.sync_all()?;
    }

    // Read it back to be sure the signature landed.
    let mut check = [0u8; SECTOR];
    f.seek(SeekFrom::Start(0))?;
    f.read_exact(&mut check).context("re-reading boot sector")?;
    if !is_bootable(&check)? {
        bail!("wrote the boot sector but the BOOTDISK signature is not there");
    }
    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(kind: FatKind) -> Vec<u8> {
        let mut s = vec![0u8; SECTOR];
        s[510] = 0x55;
        s[511] = 0xAA;
        match kind {
            FatKind::Fat16 => s[0x36..0x3E].copy_from_slice(b"FAT16   "),
            FatKind::Fat32 => s[0x52..0x5A].copy_from_slice(b"FAT32   "),
        }
        s
    }

    #[test]
    fn patches_fat16_at_0x40() {
        let mut s = blank(FatKind::Fat16);
        assert!(!is_bootable(&s).unwrap());
        assert_eq!(patch(&mut s).unwrap(), FatKind::Fat16);
        assert_eq!(&s[0x40..0x48], SIGNATURE);
        assert!(is_bootable(&s).unwrap());
    }

    #[test]
    fn patches_fat32_at_0x1e0() {
        let mut s = blank(FatKind::Fat32);
        assert_eq!(patch(&mut s).unwrap(), FatKind::Fat32);
        assert_eq!(&s[0x1E0..0x1E8], SIGNATURE);
        assert_eq!(&s[0x40..0x48], &[0u8; 8]);
    }

    #[test]
    fn rejects_non_fat() {
        let mut s = vec![0u8; SECTOR];
        assert!(patch(&mut s).is_err());
        s[510] = 0x55;
        s[511] = 0xAA;
        assert!(patch(&mut s).is_err());
    }
}
