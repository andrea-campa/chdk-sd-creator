//! Camera registry, loaded from the embedded `cameras.toml`.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Camera {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// e.g. "IXUS / ELPH", "PowerShot SX".
    #[serde(default)]
    pub series: String,
    pub year: Option<u16>,
    pub os: String,
    pub processor: Option<String>,
    pub usb_pid: Option<u16>,
    pub firmwares: Vec<String>,
    /// false when only development (trunk) builds exist.
    #[serde(default = "yes")]
    pub stable: bool,
    pub fi2: bool,
    pub fat32_autoboot: bool,
    /// After booting, CHDK switches to the card's first FAT32 partition.
    #[serde(default)]
    pub multipart: bool,
    pub ver_req: String,
    pub max_card_gb: Option<u32>,
    #[serde(default)]
    pub fw_check: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct Registry {
    camera: Vec<Camera>,
}

pub fn all() -> &'static [Camera] {
    static CAMERAS: OnceLock<Vec<Camera>> = OnceLock::new();
    CAMERAS.get_or_init(|| {
        toml::from_str::<Registry>(include_str!("../cameras.toml"))
            .expect("cameras.toml is invalid")
            .camera
    })
}

pub fn find(id: &str) -> anyhow::Result<&'static Camera> {
    all()
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| anyhow::anyhow!("unknown camera '{id}'"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn registry_parses() {
        let cams = super::all();
        assert!(!cams.is_empty());
        assert!(cams.len() > 100, "expected the full CHDK camera list");
        let mut ids: Vec<_> = cams.iter().map(|c| &c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), cams.len(), "duplicate camera ids");
        assert!(cams.iter().all(|c| !c.firmwares.is_empty()));

        let ixus90 = super::find("ixus90_sd790").unwrap();
        assert_eq!(ixus90.name, "IXUS 90 IS"); // from cameras.overrides.toml
        assert_eq!(ixus90.usb_pid, Some(0x3174));
        assert!(!ixus90.fat32_autoboot && ixus90.multipart && ixus90.fi2);
    }
}
