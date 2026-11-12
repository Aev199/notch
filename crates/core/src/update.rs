//! Self-update of the portable EXE from GitHub Releases (same scheme as
//! Papka). A release carries `notch.exe` and `latest.json` (version, SHA-256,
//! notes). The EXE is swapped by renaming: Windows lets a running EXE be
//! renamed, not overwritten. The previous one stays as `notch.old.exe` for
//! one-click rollback.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const REPOSITORY: &str = "Aev199/notch";
pub const EXE_ASSET: &str = "notch.exe";
pub const MANIFEST_ASSET: &str = "latest.json";

/// "v0.1.44" / "0.1.44-dev" → [0, 1, 44].
pub fn parse_version(text: &str) -> Vec<u64> {
    text.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect()
}

pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (a, b) = (parse_version(a), parse_version(b));
    (0..a.len().max(b.len()))
        .map(|i| a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)))
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub version: String,
    pub page: String,
    pub exe_url: String,
    pub manifest_url: String,
}

/// The newest complete (EXE + manifest), final release in GitHub's list.
pub fn pick_release(list: &[u8]) -> io::Result<Option<Candidate>> {
    let releases: Vec<GithubRelease> = serde_json::from_slice(list).map_err(io::Error::other)?;
    Ok(releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let url = |name: &str| r.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
            Some(Candidate {
                exe_url: url(EXE_ASSET)?,
                manifest_url: url(MANIFEST_ASSET)?,
                version: r.tag_name.trim_start_matches(['v', 'V']).to_string(),
                page: r.html_url,
            })
        })
        .max_by(|a, b| compare_versions(&a.version, &b.version)))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    pub sha256: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Update {
    pub version: String,
    pub notes: Vec<String>,
    pub sha256: String,
    /// `https://…` for a release, a local path for a file the browser fetched.
    pub source: String,
    pub page: String,
}

pub fn releases_url() -> String {
    format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=30")
}

/// Turns a picked release and its manifest into an update, if newer than `current`.
pub fn make_update(current: &str, candidate: Candidate, manifest_json: &[u8]) -> io::Result<Option<Update>> {
    if compare_versions(&candidate.version, current) != Ordering::Greater {
        return Ok(None);
    }
    let manifest: Manifest = serde_json::from_slice(manifest_json).map_err(io::Error::other)?;
    if compare_versions(&manifest.version, &candidate.version) != Ordering::Equal || manifest.sha256.len() != 64 {
        return Err(io::Error::other("описание выпуска не совпадает с самим выпуском"));
    }
    Ok(Some(Update {
        version: candidate.version,
        notes: manifest.notes,
        sha256: manifest.sha256.to_ascii_lowercase(),
        source: candidate.exe_url,
        page: candidate.page,
    }))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "notch".into());
    exe.with_file_name(format!("{stem}{suffix}"))
}

/// Where the replaced EXE is kept for rollback.
pub fn previous_path(exe: &Path) -> PathBuf {
    sibling(exe, ".old.exe")
}

/// Puts `bytes` in place of `exe` after checking the hash; the old EXE
/// becomes `previous_path(exe)`. Nothing changes if any step fails.
pub fn install(exe: &Path, bytes: &[u8], sha256: &str) -> io::Result<()> {
    if !sha256_hex(bytes).eq_ignore_ascii_case(sha256.trim()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "контрольная сумма загруженного файла не совпала"));
    }
    if !bytes.starts_with(b"MZ") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "файл — не программа Windows"));
    }
    let staged = sibling(exe, ".new.exe");
    let previous = previous_path(exe);
    let written = (|| {
        let mut f = fs::File::create(&staged)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&staged);
        return Err(e);
    }
    // The kept previous version steps aside instead of being deleted first:
    // if the swap fails it comes back.
    let retired = sibling(exe, ".old.exe.retired");
    let had_previous = previous.exists();
    if had_previous {
        let _ = fs::remove_file(&retired);
        if let Err(e) = fs::rename(&previous, &retired) {
            let _ = fs::remove_file(&staged);
            return Err(e);
        }
    }
    let restore_previous = || {
        if had_previous {
            let _ = fs::rename(&retired, &previous);
        }
    };
    if let Err(e) = fs::rename(exe, &previous) {
        let _ = fs::remove_file(&staged);
        restore_previous();
        return Err(e);
    }
    if let Err(e) = fs::rename(&staged, exe) {
        let _ = fs::rename(&previous, exe);
        let _ = fs::remove_file(&staged);
        restore_previous();
        return Err(e);
    }
    // A running image cannot be deleted; it goes on a later install.
    let _ = fs::remove_file(&retired);
    Ok(())
}

/// Swaps `exe` with the kept previous version (so it can be undone too).
pub fn rollback(exe: &Path) -> io::Result<()> {
    let previous = previous_path(exe);
    if !previous.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "предыдущей версии нет"));
    }
    let swap = sibling(exe, ".swap.exe");
    let _ = fs::remove_file(&swap);
    fs::rename(exe, &swap)?;
    if let Err(e) = fs::rename(&previous, exe) {
        let _ = fs::rename(&swap, exe);
        return Err(e);
    }
    if let Err(e) = fs::rename(&swap, &previous) {
        let _ = fs::remove_file(&swap);
        return Err(e);
    }
    Ok(())
}

/// The newest final EXE, for the browser when Notch itself has no way out.
pub fn browser_url() -> String {
    format!("https://github.com/{REPOSITORY}/releases/latest/download/{EXE_ASSET}")
}

/// The newest `notch*.exe` in `folder` (browsers add " (1)" to repeated
/// downloads) that is newer than `current`, with its version.
pub fn find_downloaded(
    folder: &Path,
    current: &str,
    version_of: &dyn Fn(&Path) -> Option<String>,
) -> Option<(PathBuf, String)> {
    fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            name.starts_with("notch") && name.ends_with(".exe") && p.is_file()
        })
        .filter_map(|p| version_of(&p).map(|v| (p, v)))
        .filter(|(_, v)| compare_versions(v, current) == Ordering::Greater)
        .max_by(|a, b| compare_versions(&a.1, &b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("notch-update-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.1.44-dev"), [0, 1, 44]);
        assert_eq!(compare_versions("0.1.10", "0.1.9"), Ordering::Greater);
        assert_eq!(compare_versions("v0.1.9", "0.1.9.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.0", "0.1.11"), Ordering::Less);
    }

    #[test]
    fn picks_newest_complete_final_release() {
        let list = br#"[
          {"tag_name":"v0.1.20","html_url":"p20","prerelease":true,"assets":[{"name":"notch.exe","browser_download_url":"e20"},{"name":"latest.json","browser_download_url":"m20"}]},
          {"tag_name":"v0.1.15","html_url":"p15","assets":[{"name":"notch.exe","browser_download_url":"e15"},{"name":"latest.json","browser_download_url":"m15"}]},
          {"tag_name":"v0.1.16","html_url":"p16","assets":[{"name":"notch.exe","browser_download_url":"e16"}]},
          {"tag_name":"v0.1.12","html_url":"p12","draft":true,"assets":[{"name":"notch.exe","browser_download_url":"e"},{"name":"latest.json","browser_download_url":"m"}]}
        ]"#;
        let c = pick_release(list).unwrap().unwrap();
        assert_eq!((c.version.as_str(), c.exe_url.as_str(), c.manifest_url.as_str()), ("0.1.15", "e15", "m15"));
        assert!(pick_release(b"[]").unwrap().is_none());
        assert!(pick_release(b"{oops").is_err());
    }

    #[test]
    fn manifest_must_match() {
        let c = Candidate { version: "0.1.15".into(), page: "p".into(), exe_url: "e".into(), manifest_url: "m".into() };
        let good = format!(r#"{{"version":"0.1.15","sha256":"{}","notes":["a"]}}"#, "A".repeat(64));
        let u = make_update("0.1.11", c.clone(), good.as_bytes()).unwrap().unwrap();
        assert_eq!(u.sha256, "a".repeat(64));
        assert_eq!(u.notes, ["a"]);
        assert!(make_update("0.1.15", c.clone(), good.as_bytes()).unwrap().is_none());
        let wrong = format!(r#"{{"version":"0.1.14","sha256":"{}"}}"#, "a".repeat(64));
        assert!(make_update("0.1.11", c, wrong.as_bytes()).is_err());
    }

    #[test]
    fn install_and_rollback_swap_files() {
        let dir = temp("swap");
        let exe = dir.join("notch.exe");
        fs::write(&exe, b"MZ old").unwrap();
        let new = b"MZ new".to_vec();
        assert!(install(&exe, &new, &"0".repeat(64)).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"MZ old");
        assert!(install(&exe, b"not exe", &sha256_hex(b"not exe")).is_err());
        install(&exe, &new, &sha256_hex(&new)).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"MZ new");
        assert_eq!(fs::read(previous_path(&exe)).unwrap(), b"MZ old");
        // A second install keeps exactly one previous version.
        let newer = b"MZ newer".to_vec();
        install(&exe, &newer, &sha256_hex(&newer)).unwrap();
        assert_eq!(fs::read(previous_path(&exe)).unwrap(), b"MZ new");
        rollback(&exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"MZ new");
        assert_eq!(fs::read(previous_path(&exe)).unwrap(), b"MZ newer");
        let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(leftovers.len(), 2, "{leftovers:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn finds_newer_download() {
        let dir = temp("downloads");
        for name in ["notch.exe", "notch (1).exe", "Notch (2).exe", "other.exe", "notch.txt"] {
            fs::write(dir.join(name), b"MZ").unwrap();
        }
        let version = |p: &Path| -> Option<String> {
            match p.file_name()?.to_str()? {
                "notch.exe" => Some("0.1.12".into()),
                "notch (1).exe" => Some("0.1.14".into()),
                "Notch (2).exe" => Some("0.1.13".into()),
                _ => Some("9.9.9".into()),
            }
        };
        let (path, v) = find_downloaded(&dir, "0.1.12", &version).unwrap();
        assert_eq!((path.file_name().unwrap().to_str().unwrap(), v.as_str()), ("notch (1).exe", "0.1.14"));
        assert!(find_downloaded(&dir, "0.1.14", &version).is_none());
        fs::remove_dir_all(&dir).unwrap();
    }
}
