//! Finding and downloading builds from the CHDK autobuild server.

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const STABLE_URL: &str = "https://mighty-hoernsche.de/";
const TRUNK_URL: &str = "https://mighty-hoernsche.de/trunk/";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Trunk,
}

impl Channel {
    fn page(self) -> &'static str {
        match self {
            Channel::Stable => STABLE_URL,
            Channel::Trunk => TRUNK_URL,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Build {
    pub firmware: String,
    pub version: String,
    pub revision: u32,
    /// The "full" package adds fonts, scripts, grids, curves and docs.
    pub full: bool,
    pub file_name: String,
    pub url: String,
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("chdk-sd-creator/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Extract the builds for one camera from an autobuild index page.
pub fn parse_index(html: &str, page_url: &str, camera_id: &str) -> Vec<Build> {
    let re = Regex::new(&format!(
        r#"href="([^"]*?({id})-(\d{{3}}[a-z])-(\d+\.\d+\.\d+)-(\d+)(-full)?\.zip)""#,
        id = regex::escape(camera_id)
    ))
    .unwrap();
    let base = page_url.trim_end_matches('/');
    let mut builds: Vec<Build> = re
        .captures_iter(html)
        .map(|c| {
            let href = &c[1];
            let url = if href.starts_with("http") {
                href.to_string()
            } else {
                format!("{base}/{}", href.trim_start_matches('/'))
            };
            Build {
                file_name: href.rsplit('/').next().unwrap_or(href).to_string(),
                firmware: c[3].to_string(),
                version: c[4].to_string(),
                revision: c[5].parse().unwrap_or(0),
                full: c.get(6).is_some(),
                url,
            }
        })
        .collect();
    builds.sort_by(|a, b| (&a.firmware, a.full).cmp(&(&b.firmware, b.full)));
    builds.dedup();
    builds
}

pub async fn list_builds(camera_id: &str, channel: Channel) -> Result<Vec<Build>> {
    let page = channel.page();
    let html = client()?
        .get(page)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("contacting {page}"))?
        .text()
        .await?;
    let builds = parse_index(&html, page, camera_id);
    if builds.is_empty() {
        bail!("no builds for {camera_id} found on {page}");
    }
    Ok(builds)
}

/// Download a build into `cache_dir`, reusing a previous download if it is a valid zip.
pub async fn download(
    build: &Build,
    cache_dir: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    std::fs::create_dir_all(cache_dir)?;
    let dest = cache_dir.join(&build.file_name);
    if dest.exists() && crate::install::validate_zip(&dest).is_ok() {
        let len = std::fs::metadata(&dest)?.len();
        progress(len, Some(len));
        return Ok(dest);
    }

    let resp = client()?
        .get(&build.url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("downloading {}", build.url))?;
    let total = resp.content_length();
    let tmp = dest.with_extension("part");
    let mut file = std::fs::File::create(&tmp)?;
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download interrupted")?;
        std::io::Write::write_all(&mut file, &chunk)?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    drop(file);
    std::fs::rename(&tmp, &dest)?;
    crate::install::validate_zip(&dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_index() {
        let html = r#"
            <a href="bins/ixus90_sd790-100c-1.6.1-6355-full.zip">x</a>
            <a href="bins/ixus90_sd790-100c-1.6.1-6355.zip">x</a>
            <a href="bins/ixus90_sd790-100e-1.6.1-6355.zip">x</a>
            <a href="bins/ixus900_sd900-100c-1.6.1-6355.zip">x</a>
            <a href="bins/ixus90_sd790-100c-1.6.1-6355.zip">dup</a>"#;
        let b = parse_index(html, "https://mighty-hoernsche.de/trunk/", "ixus90_sd790");
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].firmware, "100c");
        assert!(!b[0].full && b[1].full);
        assert_eq!(b[0].revision, 6355);
        assert_eq!(
            b[1].url,
            "https://mighty-hoernsche.de/trunk/bins/ixus90_sd790-100c-1.6.1-6355-full.zip"
        );
        assert_eq!(b[2].firmware, "100e");
    }
}
