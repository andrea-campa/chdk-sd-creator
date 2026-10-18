use super::{Card, FormatLayout};
use anyhow::{bail, Result};

const MSG: &str = "SD card handling is only implemented on macOS so far";

pub fn list_cards() -> Result<Vec<Card>> {
    bail!(MSG)
}

pub fn card(_device: &str) -> Result<Card> {
    bail!(MSG)
}

pub fn writable_card(_device: &str) -> Result<Card> {
    bail!(MSG)
}

pub fn make_bootable(_device: &str) -> Result<crate::bootsector::FatKind> {
    bail!(MSG)
}

pub fn format(_device: &str, _layout: FormatLayout) -> Result<()> {
    bail!(MSG)
}

pub fn eject(_device: &str) -> Result<()> {
    bail!(MSG)
}
