//! Self updates from GitHub Releases.
//!
//! On start (at most once a day) a worker thread asks the releases API whether something newer
//! than this build exists. If the user says go, it downloads the matching asset, checks it
//! against the SHA256SUMS file of the same release and swaps it in with a rename, so a failed
//! or interrupted update never leaves a broken binary behind. The new version runs after a
//! restart.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use semver::Version;
use serde::{Deserialize, Serialize};
use servo::EventLoopWaker;
use sha2::{Digest, Sha256};

const RELEASES_API: &str = "https://api.github.com/repos/xRookieFight/drift/releases?per_page=10";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Way above any real release, just stops a misbehaving server from filling the disk.
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
const ARCH: &str = "x86_64";

/// What gets replaced when an update is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Running from an AppImage, replace the AppImage file itself.
    AppImage(PathBuf),
    /// Plain binary, e.g. ~/.local/bin/drift from the tarball or install.sh.
    Binary(PathBuf),
}

impl Target {
    /// Works out how this copy of Drift was installed. Returns None for builds running out of
    /// a cargo target dir, those are updated with git, not by us.
    pub fn detect() -> Option<Target> {
        if let Some(appimage) = std::env::var_os("APPIMAGE") {
            return Some(Target::AppImage(PathBuf::from(appimage)));
        }
        let exe = std::env::current_exe().ok()?;
        if exe.components().any(|c| c.as_os_str() == "target") {
            return None;
        }
        Some(Target::Binary(exe))
    }

    pub fn path(&self) -> &Path {
        match self {
            Target::AppImage(p) | Target::Binary(p) => p,
        }
    }

    fn asset_name(&self, version: &Version) -> String {
        match self {
            Target::AppImage(_) => format!("Drift-{version}-{ARCH}.AppImage"),
            Target::Binary(_) => format!("drift-{version}-{ARCH}-linux.tar.gz"),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Release {
    pub version: String,
    pub page: String,
    #[serde(skip)]
    assets: HashMap<String, String>,
}

pub enum Event {
    Available(Release),
    Ready,
    Failed(String),
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    draft: bool,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Default, Serialize, Deserialize)]
struct CheckState {
    last_check: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn state_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("drift").join("update.json"))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(concat!("drift/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Checks for a newer release in the background, unless one was checked recently.
pub fn spawn_check(target: Target, tx: Sender<Event>, waker: Box<dyn EventLoopWaker>) {
    let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        return;
    };
    let state_file = state_path();
    let last = state_file
        .as_deref()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<CheckState>(&t).ok())
        .unwrap_or_default();
    if now_secs().saturating_sub(last.last_check) < CHECK_INTERVAL.as_secs() {
        return;
    }

    let spawned = thread::Builder::new()
        .name("drift-update-check".into())
        .spawn(move || {
            let result = fetch_releases().map(|list| newest(list, &current, &target));
            match result {
                Ok(found) => {
                    if let Some(path) = state_file {
                        let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
                        let state = CheckState {
                            last_check: now_secs(),
                        };
                        let _ = fs::write(path, serde_json::to_vec(&state).unwrap_or_default());
                    }
                    if let Some(release) = found {
                        let _ = tx.send(Event::Available(release));
                        waker.wake();
                    }
                }
                // offline is normal, try again on the next start
                Err(err) => log::info!("update check failed: {err}"),
            }
        });
    if let Err(err) = spawned {
        log::warn!("could not start update check: {err}");
    }
}

fn fetch_releases() -> Result<Vec<GhRelease>, String> {
    agent()
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json::<Vec<GhRelease>>()
        .map_err(|e| e.to_string())
}

/// Picks the highest version above `current` that ships an asset for this install type.
fn newest(list: Vec<GhRelease>, current: &Version, target: &Target) -> Option<Release> {
    list.into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.trim_start_matches('v')).ok()?;
            Some((version, r))
        })
        .filter(|(v, r)| {
            v > current
                && r.assets.iter().any(|a| a.name == target.asset_name(v))
                && r.assets.iter().any(|a| a.name == "SHA256SUMS")
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(version, r)| Release {
            version: version.to_string(),
            page: r.html_url,
            assets: r
                .assets
                .into_iter()
                .map(|a| (a.name, a.browser_download_url))
                .collect(),
        })
}

/// Downloads, verifies and swaps in `release`. Reports back with Ready or Failed.
pub fn spawn_install(
    release: Release,
    target: Target,
    tx: Sender<Event>,
    waker: Box<dyn EventLoopWaker>,
) {
    let spawned = thread::Builder::new()
        .name("drift-update-install".into())
        .spawn(move || {
            let event = match install(&release, &target) {
                Ok(()) => Event::Ready,
                Err(err) => {
                    log::error!("update failed: {err}");
                    Event::Failed(err)
                }
            };
            let _ = tx.send(event);
            waker.wake();
        });
    if let Err(err) = spawned {
        log::warn!("could not start update install: {err}");
    }
}

fn install(release: &Release, target: &Target) -> Result<(), String> {
    let version = Version::parse(&release.version).map_err(|e| e.to_string())?;
    let name = target.asset_name(&version);
    let asset_url = release
        .assets
        .get(&name)
        .ok_or("release has no asset for this install")?;
    let sums_url = release
        .assets
        .get("SHA256SUMS")
        .ok_or("release has no SHA256SUMS")?;

    let sums = download_text(sums_url)?;
    let expected = parse_sums(&sums)
        .remove(&name)
        .ok_or_else(|| format!("SHA256SUMS does not list {name}"))?;

    let dest = target.path();
    let dir = dest
        .parent()
        .ok_or("install path has no parent directory")?;
    let download = dir.join(format!(".drift-update-{}.download", std::process::id()));
    let staged = dir.join(format!(".drift-update-{}.new", std::process::id()));

    let result = (|| {
        let actual = download_to(asset_url, &download)?;
        if actual != expected {
            return Err(format!("checksum mismatch for {name}"));
        }
        match target {
            Target::AppImage(_) => fs::rename(&download, &staged).map_err(|e| e.to_string())?,
            Target::Binary(_) => extract_binary(&download, &staged)?,
        }
        set_executable(&staged)?;
        // rename is atomic on the same filesystem, and Linux keeps the running binary's
        // inode alive, so swapping the file under ourselves is fine
        fs::rename(&staged, dest).map_err(|e| format!("could not replace {}: {e}", dest.display()))
    })();

    let _ = fs::remove_file(&download);
    let _ = fs::remove_file(&staged);
    result
}

fn download_text(url: &str) -> Result<String, String> {
    agent()
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .map_err(|e| e.to_string())
}

/// Streams `url` into `path` and returns the hex SHA-256 of what was written.
fn download_to(url: &str, path: &Path) -> Result<String, String> {
    let mut response = agent().get(url).call().map_err(|e| e.to_string())?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .reader();
    let mut file = File::create(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(hex(&hasher.finalize()))
}

/// Pulls the `drift` executable out of a release tarball.
fn extract_binary(tarball: &Path, out: &Path) -> Result<(), String> {
    let file = File::open(tarball).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        if path.file_name().is_some_and(|n| n == "drift") && entry.header().entry_type().is_file() {
            let mut dest = File::create(out).map_err(|e| e.to_string())?;
            io::copy(&mut entry, &mut dest).map_err(|e| e.to_string())?;
            dest.sync_all().map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err("tarball does not contain a drift binary".into())
}

fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

/// Parses `sha256sum` output: `<hex>  <name>` or `<hex> *<name>`, names may carry a `./`.
fn parse_sums(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (hash, name) = line.split_once(char::is_whitespace)?;
            let name = name.trim().trim_start_matches('*').trim_start_matches("./");
            (hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| (name.to_owned(), hash.to_ascii_lowercase()))
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, assets: &[&str]) -> GhRelease {
        GhRelease {
            tag_name: tag.into(),
            html_url: format!("https://github.com/xRookieFight/drift/releases/tag/{tag}"),
            draft: false,
            assets: assets
                .iter()
                .map(|n| GhAsset {
                    name: n.to_string(),
                    browser_download_url: format!("https://example.invalid/{n}"),
                })
                .collect(),
        }
    }

    fn bin() -> Target {
        Target::Binary("/home/u/.local/bin/drift".into())
    }

    #[test]
    fn picks_highest_newer_release_with_assets() {
        let current = Version::parse("0.1.0").unwrap();
        let list = vec![
            release("v0.1.0", &["drift-0.1.0-x86_64-linux.tar.gz", "SHA256SUMS"]),
            release("v0.3.0", &["drift-0.3.0-x86_64-linux.tar.gz"]),
            release("v0.2.1", &["drift-0.2.1-x86_64-linux.tar.gz", "SHA256SUMS"]),
            release("v0.2.0", &["drift-0.2.0-x86_64-linux.tar.gz", "SHA256SUMS"]),
        ];
        let found = newest(list, &current, &bin()).unwrap();
        // 0.3.0 has no checksums, so it is not installable
        assert_eq!(found.version, "0.2.1");
    }

    #[test]
    fn nothing_newer_means_nothing() {
        let current = Version::parse("0.2.0").unwrap();
        let list = vec![release(
            "v0.2.0",
            &["drift-0.2.0-x86_64-linux.tar.gz", "SHA256SUMS"],
        )];
        assert!(newest(list, &current, &bin()).is_none());
    }

    #[test]
    fn appimage_installs_need_the_appimage_asset() {
        let current = Version::parse("0.1.0").unwrap();
        let list = vec![release(
            "v0.2.0",
            &["drift-0.2.0-x86_64-linux.tar.gz", "SHA256SUMS"],
        )];
        let target = Target::AppImage("/home/u/Drift.AppImage".into());
        assert!(newest(list, &current, &target).is_none());
    }

    #[test]
    fn drafts_and_odd_tags_are_skipped() {
        let current = Version::parse("0.1.0").unwrap();
        let mut draft = release("v0.5.0", &["drift-0.5.0-x86_64-linux.tar.gz", "SHA256SUMS"]);
        draft.draft = true;
        let list = vec![draft, release("nightly", &["SHA256SUMS"])];
        assert!(newest(list, &current, &bin()).is_none());
    }

    #[test]
    fn parses_sha256sum_output() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let text =
            format!("{a}  ./drift-0.2.0-x86_64-linux.tar.gz\n{b} *Drift.AppImage\nnot a line\n");
        let sums = parse_sums(&text);
        assert_eq!(sums.get("drift-0.2.0-x86_64-linux.tar.gz"), Some(&a));
        assert_eq!(sums.get("Drift.AppImage"), Some(&"b".repeat(64)));
        assert_eq!(sums.len(), 2);
    }

    #[test]
    fn extracts_binary_from_tarball() {
        let dir = std::env::temp_dir().join(format!("drift-update-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let tarball = dir.join("t.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                File::create(&tarball).unwrap(),
                flate2::Compression::fast(),
            );
            let mut builder = tar::Builder::new(gz);
            for (name, body) in [
                ("drift-0.2.0/README.md", &b"readme"[..]),
                ("drift-0.2.0/drift", b"ELF"),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(body.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, name, body).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
        }
        let out = dir.join("drift.new");
        extract_binary(&tarball, &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"ELF");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dev_builds_are_not_updatable() {
        // the test binary itself lives in target/, which is exactly the dev case
        if std::env::var_os("APPIMAGE").is_none() {
            assert_eq!(Target::detect(), None);
        }
    }

    /// Serves fixed bodies over plain HTTP on localhost, one request per connection.
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
        use std::io::{BufRead, BufReader};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) <= 2 {
                        break;
                    }
                }
                let body = routes
                    .iter()
                    .find(|(p, _)| *p == path)
                    .map(|(_, b)| b.clone());
                let mut stream = stream;
                let _ = match body {
                    Some(body) => {
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        stream
                            .write_all(head.as_bytes())
                            .and_then(|_| stream.write_all(&body))
                    }
                    None => stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    ),
                };
            }
        });
        base
    }

    fn tarball(binary: &[u8]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(binary.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "drift-0.2.0-x86_64-linux/drift", binary)
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn fake_release(base: &str) -> Release {
        let name = "drift-0.2.0-x86_64-linux.tar.gz";
        Release {
            version: "0.2.0".into(),
            page: String::new(),
            assets: HashMap::from([
                (name.to_owned(), format!("{base}/tarball")),
                ("SHA256SUMS".to_owned(), format!("{base}/sums")),
            ]),
        }
    }

    #[test]
    fn installs_a_verified_update_end_to_end() {
        let dir = std::env::temp_dir().join(format!("drift-update-e2e-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("drift");
        fs::write(&exe, b"old build").unwrap();

        let archive = tarball(b"new build");
        let sums = format!(
            "{}  ./drift-0.2.0-x86_64-linux.tar.gz\n",
            hex(&Sha256::digest(&archive))
        );
        let base = serve(vec![("/tarball", archive), ("/sums", sums.into_bytes())]);

        install(&fake_release(&base), &Target::Binary(exe.clone())).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new build");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
            0o755
        );
        // nothing left behind next to the binary
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checksum_mismatch_leaves_the_old_binary_alone() {
        let dir = std::env::temp_dir().join(format!("drift-update-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("drift");
        fs::write(&exe, b"old build").unwrap();

        let sums = format!("{}  drift-0.2.0-x86_64-linux.tar.gz\n", "0".repeat(64));
        let base = serve(vec![
            ("/tarball", tarball(b"evil build")),
            ("/sums", sums.into_bytes()),
        ]);

        let err = install(&fake_release(&base), &Target::Binary(exe.clone())).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert_eq!(fs::read(&exe).unwrap(), b"old build");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
