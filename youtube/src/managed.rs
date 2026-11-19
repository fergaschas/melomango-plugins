//! A copy of yt-dlp managed by the plugin, kept up to date on its own.
//!
//! The yt-dlp.exe that winget installs is a single-file build that unpacks itself on
//! every run (~1.3 s per call). The folder build (`yt-dlp_win.zip`) starts in ~0.5 s, so
//! the plugin downloads it from yt-dlp's GitHub releases into its data folder,
//! checking the SHA-256 that yt-dlp publishes:
//!
//! ```text
//! $MELOMANGO_DATA_DIR/yt-dlp/
//!     2026.10.26/yt-dlp.exe, _internal/…   one folder per version
//!     current                              name of the folder in use
//!     last-check                           when updates were last looked for (mtime)
//! ```
//!
//! Updates run in a detached background process (`update-ytdlp`) at most once a day,
//! so no call ever waits for a download. Old versions are deleted once nothing uses them.

use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime},
};

use anyhow::{bail, Context};
use sha2::{Digest, Sha256};

const REPO: &str = "yt-dlp/yt-dlp";
/// Folder build of yt-dlp for this platform, if there is one.
const ASSET: Option<&str> = if cfg!(windows) { Some("yt-dlp_win.zip") } else { None };
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Retry sooner while there is no managed copy yet.
const RETRY_MISSING: Duration = Duration::from_secs(60 * 60);
/// A lock older than this belongs to an update that died.
const STALE_LOCK: Duration = Duration::from_secs(15 * 60);
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;

fn root() -> Option<PathBuf> {
    ASSET?;
    Some(PathBuf::from(env::var_os("MELOMANGO_DATA_DIR")?).join("yt-dlp"))
}

/// Whether this platform and environment can have a managed copy.
pub fn supported() -> bool {
    root().is_some()
}

/// The managed yt-dlp executable, if installed.
pub fn executable() -> Option<PathBuf> {
    let root = root()?;
    let version = fs::read_to_string(root.join("current")).ok()?;
    let exe = root.join(version.trim()).join(exe_name());
    exe.is_file().then_some(exe)
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" }
}

/// Starts a background update if one is due. Never blocks nor fails the current call.
pub fn update_in_background() {
    let Some(root) = root() else { return };
    let due_after = if executable().is_some() { CHECK_EVERY } else { RETRY_MISSING };
    if age(&root.join("last-check")).is_some_and(|a| a < due_after) {
        return;
    }
    if age(&root.join("update.lock")).is_some_and(|a| a < STALE_LOCK) {
        return;
    }
    let Ok(me) = env::current_exe() else { return };
    let mut cmd = Command::new(me);
    cmd.arg("update-ytdlp")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Melomango reads this process's stdout until it closes: the child must not
        // inherit it, or the app would wait for the whole download.
        stop_inheriting_stdio();
        // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
        cmd.creation_flags(0x0800_0000 | 0x0000_0200);
    }
    let _ = cmd.spawn();
}

/// `update-ytdlp`: installs the latest release if it is not the current one.
pub fn update() -> anyhow::Result<PathBuf> {
    let root = root().context("este sistema no tiene una versión de yt-dlp que el plugin pueda gestionar")?;
    fs::create_dir_all(&root)?;
    let _lock = Lock::take(&root.join("update.lock"))?;
    touch(&root.join("last-check"))?;

    let agent = ureq::AgentBuilder::new()
        .user_agent(concat!("melomango-youtube/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build();
    let release: serde_json::Value = agent
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .call()
        .context("no se pudo consultar la última versión de yt-dlp")?
        .into_json()?;
    let version = release["tag_name"].as_str().context("GitHub no devolvió la versión de yt-dlp")?;
    if !version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        bail!("versión de yt-dlp no válida: {version}");
    }

    if executable().is_some_and(|exe| exe.parent().is_some_and(|d| d.ends_with(version))) {
        cleanup(&root, version);
        return Ok(executable().unwrap());
    }

    let asset = ASSET.unwrap();
    let base = format!("https://github.com/{REPO}/releases/download/{version}");
    let sums = agent.get(&format!("{base}/SHA2-256SUMS")).call()?.into_string()?;
    let expected = sums
        .lines()
        .find_map(|l| l.strip_suffix(asset).map(str::trim))
        .filter(|h| h.len() == 64)
        .with_context(|| format!("SHA2-256SUMS no incluye {asset}"))?
        .to_ascii_lowercase();

    let mut zip = Vec::new();
    agent
        .get(&format!("{base}/{asset}"))
        .call()
        .with_context(|| format!("no se pudo descargar {asset}"))?
        .into_reader()
        .take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut zip)?;
    if zip.len() as u64 > MAX_DOWNLOAD {
        bail!("{asset} es demasiado grande");
    }
    let actual = hex(&Sha256::digest(&zip));
    if actual != expected {
        bail!("la huella de {asset} no coincide (esperada {expected}, obtenida {actual})");
    }

    let staging = root.join(format!("{version}.tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    extract(&zip, &staging)?;
    let target = root.join(version);
    if target.exists() {
        // Leftover of an interrupted update: nothing points at it.
        let _ = fs::remove_dir_all(&target);
    }
    if target.exists() {
        fs::remove_dir_all(&staging)?;
    } else {
        fs::rename(&staging, &target)?;
    }

    let exe = target.join(exe_name());
    let ok = hidden(&mut Command::new(&exe))
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        bail!("la versión descargada de yt-dlp no arranca");
    }

    // Switch atomically, then drop older versions that are no longer running.
    let tmp = root.join("current.tmp");
    fs::write(&tmp, version)?;
    fs::rename(&tmp, root.join("current"))?;
    cleanup(&root, version);
    Ok(exe)
}

fn extract(zip: &[u8], dest: &Path) -> anyhow::Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).context("zip de yt-dlp no válido")?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(relative) = entry.enclosed_name() else {
            bail!("ruta no válida en el zip: {}", entry.name());
        };
        let path = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut entry, &mut fs::File::create(&path)?)?;
        }
    }
    if !dest.join(exe_name()).is_file() {
        bail!("el zip de yt-dlp no contiene {}", exe_name());
    }
    Ok(())
}

/// Deletes every version folder but `keep`; ones still in use are retried next time.
fn cleanup(root: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        if entry.path().is_dir() && entry.file_name() != keep {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn age(path: &Path) -> Option<Duration> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(SystemTime::now().duration_since(modified).unwrap_or_default())
}

fn touch(path: &Path) -> std::io::Result<()> {
    fs::write(path, b"")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// No console window, no stdin.
pub fn hidden(cmd: &mut Command) -> &mut Command {
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Only one update at a time; removed when dropped.
struct Lock(PathBuf);

impl Lock {
    fn take(path: &Path) -> anyhow::Result<Self> {
        if age(path).is_some_and(|a| a >= STALE_LOCK) {
            let _ = fs::remove_file(path);
        }
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .context("ya se está actualizando yt-dlp")?;
        Ok(Self(path.to_path_buf()))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Marks this process's standard handles as not inheritable, so children spawned with
/// null stdio don't keep the parent's pipes open.
#[cfg(windows)]
fn stop_inheriting_stdio() {
    unsafe extern "system" {
        fn GetStdHandle(n: u32) -> isize;
        fn SetHandleInformation(handle: isize, mask: u32, flags: u32) -> i32;
    }
    const HANDLE_FLAG_INHERIT: u32 = 1;
    // STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE
    for n in [-10i32, -11, -12] {
        unsafe {
            let handle = GetStdHandle(n as u32);
            if handle != 0 && handle != -1 {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_with(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        for (name, data) in files {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut writer, data).unwrap();
        }
        writer.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn extracts_the_folder_build() {
        let dir = env::temp_dir().join(format!("mm-yt-extract-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let zip = zip_with(&[(exe_name(), b"exe"), ("_internal/base_library.zip", b"lib")]);
        extract(&zip, &dir).unwrap();
        assert!(dir.join(exe_name()).is_file());
        assert!(dir.join("_internal/base_library.zip").is_file());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_paths_outside_the_folder() {
        let dir = env::temp_dir().join(format!("mm-yt-evil-{}", std::process::id()));
        let zip = zip_with(&[("../evil.txt", b"x"), (exe_name(), b"exe")]);
        assert!(extract(&zip, &dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(hex(&[0xab, 0x01]), "ab01");
    }
}
