//! Finding SD cards and doing the raw-disk work (unmount, boot sector, format).
//! Only FAT volumes on removable or external media are ever offered.

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(target_os = "macos"))]
mod unsupported;
#[cfg(not(target_os = "macos"))]
pub use unsupported::*;

#[derive(Debug, Clone, Serialize)]
pub struct Card {
    /// Partition device, e.g. "disk4s1".
    pub device: String,
    /// Whole disk, e.g. "disk4".
    pub whole_disk: String,
    pub mount_point: String,
    pub name: String,
    /// "FAT12", "FAT16" or "FAT32".
    pub fat: String,
    pub size: u64,
    pub free: u64,
    pub disk_size: u64,
    /// e.g. "Secure Digital", "USB", "Disk Image".
    pub protocol: String,
    pub has_chdk: bool,
    /// Read-only: the card's lock switch is on (or the media is write-protected).
    pub locked: bool,
    /// Every partition on the disk is FAT, so formatting it cannot hit other data.
    pub formattable: bool,
    /// The card's other mounted FAT partitions, in partition order.
    pub other_partitions: Vec<Partition>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Partition {
    pub device: String,
    pub name: String,
    pub mount_point: String,
    pub fat: String,
    pub free: u64,
}

impl Card {
    /// Where a multi-partition camera keeps CHDK and photos after booting:
    /// the first FAT32 partition, if it isn't the boot partition itself.
    pub fn data_partition(&self) -> Option<&Partition> {
        if self.fat == "FAT32" {
            return None;
        }
        self.other_partitions.iter().find(|p| p.fat == "FAT32")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FormatLayout {
    /// One FAT32 partition over the whole card (Firmware Update method).
    Fat32,
    /// A FAT16 partition the camera can autoboot from (2 GB max; the rest
    /// of a bigger card becomes a second FAT32 partition).
    Fat16,
}

/// Largest card that gets a single FAT16 partition with standard 32 KB clusters.
pub const FAT16_MAX: u64 = 2 * 1024 * 1024 * 1024;
