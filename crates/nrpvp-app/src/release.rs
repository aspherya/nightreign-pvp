//! Where releases come from, and the `release.json` manifest that describes one.
//!
//! Releases are GitHub releases of a **public, binaries-only** repository ([`RELEASES_REPO`]); the source repository stays private.
//! Every release carries three assets: `release.json` (this manifest), the player zip, and the launcher exe. The launcher reads
//! `https://github.com/<repo>/releases/latest/download/release.json` (a plain redirect, no GitHub API, no rate limit), then downloads
//! the zip of that tag and checks its SHA-256. `scripts\publish-release.ps1` writes the manifest and uploads the release.
//!
//! For testing without GitHub: `NRPVP_RELEASE_DIR=<folder holding release.json and the zip>` reads a local folder instead;
//! `NRPVP_RELEASES_REPO=<owner/name>` points at another repository.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const RELEASES_REPO: &str = "notgonnaleo/nightreign-pvp-releases";
/// Seamless Co-op for Nightreign, by Yui. The GitHub repository holds only data files today (no releases); the mod itself is
/// published on Nexus Mods. The launcher installs from a GitHub release if one ever appears, else guides the Nexus download.
pub const SC_GITHUB_REPO: &str = "yuiamoroll/NightreignSeamlessCoopRelease";
pub const SC_NEXUS_URL: &str = "https://www.nexusmods.com/eldenringnightreign/mods/3?tab=files";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamePin {
    /// e.g. "1.03.3"
    pub version: String,
    pub exe_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScPin {
    /// e.g. "1.1.3"
    pub version: String,
    pub nrsc_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    /// e.g. "0.2.0"
    pub version: String,
    /// The GitHub tag, e.g. "v0.2.0".
    pub tag: String,
    /// "YYYY-MM-DD"
    pub published: String,
    /// Patch notes, plain text; blank lines separate paragraphs, lines starting with "- " are list items.
    #[serde(default)]
    pub notes: String,
    pub zip: Asset,
    #[serde(default)]
    pub launcher: Option<Asset>,
    pub game: GamePin,
    pub seamless_coop: ScPin,
    /// Players per match in this build (mm_max_players).
    #[serde(default)]
    pub match_players: Option<u32>,
}

pub enum Source {
    GitHub(String),
    Local(PathBuf),
}

pub fn source() -> Source {
    if let Some(dir) = std::env::var_os("NRPVP_RELEASE_DIR") {
        return Source::Local(PathBuf::from(dir));
    }
    Source::GitHub(
        std::env::var("NRPVP_RELEASES_REPO").unwrap_or_else(|_| RELEASES_REPO.to_string()),
    )
}

pub fn releases_page() -> String {
    match source() {
        Source::GitHub(repo) => format!("https://github.com/{repo}/releases"),
        Source::Local(dir) => dir.display().to_string(),
    }
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!(
            "NightreignPvP-Launcher/",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(Duration::from_secs(15))
        .timeout(None)
        .build()
        .map_err(|e| format!("Could not start the download client: {e}"))
}

/// The latest release's manifest.
pub fn latest() -> Result<Manifest, String> {
    let text = match source() {
        Source::Local(dir) => std::fs::read_to_string(dir.join("release.json"))
            .map_err(|e| format!("No release.json in {}: {e}", dir.display()))?,
        Source::GitHub(repo) => {
            let url = format!("https://github.com/{repo}/releases/latest/download/release.json");
            let resp = client()?
                .get(&url)
                .send()
                .map_err(|e| format!("Could not reach GitHub: {e}"))?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Err("No release has been published yet.".into());
            }
            if !resp.status().is_success() {
                return Err(format!(
                    "GitHub answered {} for the release list.",
                    resp.status()
                ));
            }
            resp.text()
                .map_err(|e| format!("Could not read the release info: {e}"))?
        }
    };
    parse(&text)
}

pub fn parse(text: &str) -> Result<Manifest, String> {
    let m: Manifest =
        serde_json::from_str(text).map_err(|e| format!("The release info is damaged: {e}"))?;
    if m.schema != 1 {
        return Err(format!(
            "This release needs a newer launcher (release format {}). Download the new launcher from the releases page.",
            m.schema
        ));
    }
    Ok(m)
}

/// Downloads (or, for a local source, copies) `asset` of release `m` into `dest`, calling `progress(done, total)`, and checks its
/// SHA-256 against the manifest.
pub fn fetch_asset(
    m: &Manifest,
    asset: &Asset,
    dest: &Path,
    progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let reader: Box<dyn Read> = match source() {
        Source::Local(dir) => Box::new(
            std::fs::File::open(dir.join(&asset.name))
                .map_err(|e| format!("Could not open {}: {e}", asset.name))?,
        ),
        Source::GitHub(repo) => {
            let url = format!(
                "https://github.com/{repo}/releases/download/{}/{}",
                m.tag, asset.name
            );
            Box::new(open_url(&url)?)
        }
    };
    let hash = copy_hashed(reader, dest, asset.size, progress)?;
    if !hash.eq_ignore_ascii_case(&asset.sha256) {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "The download of {} is damaged (checksum differs). Try again.",
            asset.name
        ));
    }
    Ok(())
}

/// GET `url` (redirects followed); an error unless 2xx.
pub fn open_url(url: &str) -> Result<reqwest::blocking::Response, String> {
    let resp = client()?
        .get(url)
        .send()
        .map_err(|e| format!("Download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Download failed: the server answered {} for {url}",
            resp.status()
        ));
    }
    Ok(resp)
}

/// Streams `reader` into `dest`, returns the SHA-256 (upper-case hex).
pub fn copy_hashed(
    mut reader: impl Read,
    dest: &Path,
    total: u64,
    mut progress: impl FnMut(u64, u64),
) -> Result<String, String> {
    let mut out = std::fs::File::create(dest)
        .map_err(|e| format!("Could not write {}: {e}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])
            .map_err(|e| format!("Could not write {}: {e}", dest.display()))?;
        done += n as u64;
        progress(done, total.max(done));
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(hex(&hasher.finalize()))
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let f =
        std::fs::File::open(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let mut reader = std::io::BufReader::with_capacity(1 << 20, f);
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher).map_err(|e| e.to_string())?;
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// A zip asset of the latest GitHub release of Seamless Co-op for Nightreign, if there is one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScRelease {
    pub tag: String,
    pub name: String,
    pub url: String,
    pub size: u64,
}

pub fn sc_github_release() -> Result<Option<ScRelease>, String> {
    #[derive(Deserialize)]
    struct Rel {
        tag_name: String,
        assets: Vec<RelAsset>,
    }
    #[derive(Deserialize)]
    struct RelAsset {
        name: String,
        browser_download_url: String,
        size: u64,
    }
    let url = format!("https://api.github.com/repos/{SC_GITHUB_REPO}/releases/latest");
    let resp = client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("GitHub answered {}", resp.status()));
    }
    let rel: Rel = resp.json().map_err(|e| e.to_string())?;
    Ok(rel
        .assets
        .into_iter()
        .find(|a| a.name.to_ascii_lowercase().ends_with(".zip"))
        .map(|a| ScRelease {
            tag: rel.tag_name,
            name: a.name,
            url: a.browser_download_url,
            size: a.size,
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "schema": 1, "version": "0.2.0", "tag": "v0.2.0", "published": "2026-11-06",
      "notes": "First public build.\n\n- Solo Battle Royale",
      "zip": { "name": "NightreignPvP.zip", "sha256": "AB", "size": 10 },
      "game": { "version": "1.03.3", "exe_sha256": "95" },
      "seamless_coop": { "version": "1.1.3", "nrsc_sha256": "24" },
      "match_players": 2
    }"#;

    #[test]
    fn parses_a_manifest_without_optional_fields() {
        let m = parse(SAMPLE).unwrap();
        assert_eq!(m.tag, "v0.2.0");
        assert!(m.launcher.is_none());
        assert_eq!(m.match_players, Some(2));
    }

    #[test]
    fn a_newer_schema_asks_for_a_new_launcher() {
        let err = parse(&SAMPLE.replace("\"schema\": 1", "\"schema\": 2")).unwrap_err();
        assert!(err.contains("newer launcher"));
    }

    #[test]
    fn copy_hashed_matches_sha256() {
        let dest = std::env::temp_dir().join(format!("nrpvp-app-hash-{}", std::process::id()));
        let mut seen = 0;
        let h = copy_hashed(&b"abc"[..], &dest, 3, |d, _| seen = d).unwrap();
        assert_eq!(
            h,
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        );
        assert_eq!(seen, 3);
        assert_eq!(sha256_file(&dest).unwrap(), h);
        let _ = std::fs::remove_file(dest);
    }
}
