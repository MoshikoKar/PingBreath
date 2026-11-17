//! Self-update from GitHub Releases. Uses the curl.exe / PowerShell that ship with Windows,
//! so no extra HTTP/TLS dependencies are needed.
//!
//! Release assets expected (see build.ps1 / .github/workflows/release.yml):
//!   PingBreath-Setup.exe     installer      -> used when installed via the setup
//!   PingBreath-portable.zip  PingBreath.exe + portable.txt -> used in portable mode
//!
//! LEGACY(NetPulse): remove in v0.3.1 - also publish NetPulse-Setup.exe /
//! NetPulse-portable.zip (zip member NetPulse.exe) for v0.1/v0.2 clients.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

pub const REPO: &str = "MoshikoKar/PingBreath";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
use crate::sys::NO_WINDOW;

/// Set by the updater thread once the replacement has been launched; the UI timer then quits.
pub static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);

struct Release {
    version: String,
    url: String,
    asset: String,
    sums_url: String,
    notes_url: String,
}

fn split_version(v: &str) -> (Vec<u32>, Option<String>) {
    let v = v.trim().trim_start_matches('v');
    let v = v.split('+').next().unwrap_or(v);
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (v, None),
    };
    (core.split('.').map(|p| p.parse().unwrap_or(0)).collect(), pre)
}

/// Numeric semver compare; a release beats its own pre-release (0.2.0 > 0.2.0-beta.1).
fn is_newer(remote: &str, local: &str) -> bool {
    let ((mut a, ap), (mut b, bp)) = (split_version(remote), split_version(local));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (ap, bp) {
            (None, Some(_)) => true,
            (Some(x), Some(y)) => x > y,
            _ => false,
        },
    }
}

fn installed_via_setup() -> bool {
    crate::config::exe_dir().is_some_and(|d| d.join("unins000.exe").exists())
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("curl.exe")
        .args(["-sSL", "--fail", "--max-time", "120", "-H", "User-Agent: PingBreath-updater"])
        .args(args)
        .creation_flags(NO_WINDOW)
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn latest() -> Result<Release, String> {
    let beta = crate::config::load().update_channel.eq_ignore_ascii_case("beta");
    let api = if beta {
        format!("https://api.github.com/repos/{REPO}/releases?per_page=10")
    } else {
        format!("https://api.github.com/repos/{REPO}/releases/latest")
    };
    let body = curl(&[&api]).map_err(|e| format!("Could not reach GitHub: {e}"))?;
    let j: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    // beta channel: newest release of any kind (the list is newest first)
    let j = if beta { j.get(0).cloned().ok_or("no releases published")? } else { j };
    let tag = j["tag_name"].as_str().ok_or("release has no tag")?.to_string();
    let wanted = if installed_via_setup() { "PingBreath-Setup.exe" } else { "PingBreath-portable.zip" };
    let find = |name: &str| {
        j["assets"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["name"] == name))
            .and_then(|x| x["browser_download_url"].as_str())
            .map(str::to_string)
    };
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        url: find(wanted).ok_or_else(|| format!("release {tag} has no {wanted} asset"))?,
        asset: wanted.to_string(),
        sums_url: find("SHA256SUMS.txt")
            .ok_or_else(|| format!("release {tag} has no SHA256SUMS.txt - refusing to update unverified"))?,
        notes_url: j["html_url"].as_str().unwrap_or("").to_string(),
    })
}

/// SHA-256 of a file via certutil (ships with Windows).
fn sha256_of(path: &std::path::Path) -> Result<String, String> {
    let out = Command::new("certutil.exe")
        .args(["-hashfile", &path.to_string_lossy(), "SHA256"])
        .creation_flags(NO_WINDOW)
        .output()
        .map_err(|e| format!("could not run certutil: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.replace(' ', "").to_lowercase())
        .find(|l| l.len() == 64 && l.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| "could not compute SHA-256".to_string())
}

/// Download `rel.url` to `dest` and check it against the release's SHA256SUMS.txt before anything runs.
fn download_verified(rel: &Release, dest: &std::path::Path) -> Result<(), String> {
    let sums = curl(&[&rel.sums_url]).map_err(|e| format!("Could not fetch checksums: {e}"))?;
    let expected = String::from_utf8_lossy(&sums)
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (hash, name) = (it.next()?, it.next()?);
            (name.trim_start_matches('*') == rel.asset).then(|| hash.to_lowercase())
        })
        .next()
        .ok_or_else(|| format!("SHA256SUMS.txt has no entry for {}", rel.asset))?;
    curl(&["-o", &dest.to_string_lossy(), &rel.url]).map_err(|e| format!("Download failed: {e}"))?;
    let actual = sha256_of(dest)?;
    if actual != expected {
        let _ = std::fs::remove_file(dest);
        return Err(format!("Checksum mismatch - download discarded.\nexpected {expected}\nactual   {actual}"));
    }
    Ok(())
}

fn message_box(text: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (t, c) = (wide(text), wide("PingBreath"));
    unsafe { MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), flags | MB_TOPMOST | MB_SETFOREGROUND) }
}

fn info(text: &str) {
    message_box(text, MB_OK | MB_ICONINFORMATION);
}

fn apply(rel: &Release) -> Result<(), String> {
    let tmp = std::env::temp_dir().join("PingBreath-update");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    if installed_via_setup() {
        let setup = tmp.join("PingBreath-Setup.exe");
        download_verified(rel, &setup)?;
        Command::new(&setup)
            .args(["/SILENT", "/CLOSEAPPLICATIONS", "/NORESTART"])
            .spawn()
            .map_err(|e| format!("Could not start installer: {e}"))?;
    } else {
        let zip = tmp.join("PingBreath-portable.zip");
        download_verified(rel, &zip)?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // Extract PingBreath.exe, write it next to the running exe (renaming off a legacy
        // NetPulse.exe layout when needed), relaunch, then remove the old exe name.
        // LEGACY(NetPulse): remove in v0.3.1 - the NetPulse.exe branch below.
        let script = "Wait-Process -Id $env:PB_PID -ErrorAction SilentlyContinue; \
                      Expand-Archive -LiteralPath $env:PB_ZIP -DestinationPath $env:PB_DIR -Force; \
                      $dir = Split-Path -LiteralPath $env:PB_EXE; \
                      $target = Join-Path $dir 'PingBreath.exe'; \
                      Copy-Item -LiteralPath $env:PB_EXE -Destination ($env:PB_EXE + '.bak') -Force; \
                      Copy-Item -LiteralPath (Join-Path $env:PB_DIR 'PingBreath.exe') -Destination $target -Force; \
                      Start-Process -FilePath $target; \
                      if ((Split-Path -Leaf $env:PB_EXE) -ieq 'NetPulse.exe') { \
                        Start-Sleep -Seconds 2; \
                        Remove-Item -LiteralPath $env:PB_EXE -Force -ErrorAction SilentlyContinue \
                      }";
        Command::new("powershell.exe")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-Command", script])
            .env("PB_PID", std::process::id().to_string())
            .env("PB_ZIP", &zip)
            .env("PB_DIR", tmp.join("x"))
            .env("PB_EXE", &exe)
            .creation_flags(NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Could not start updater: {e}"))?;
    }
    QUIT_REQUESTED.store(true, Ordering::Relaxed);
    Ok(())
}

/// Check in the background. `manual` = user asked (always report the result);
/// otherwise stay silent unless a newer version exists.
pub fn check_async(manual: bool) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        match latest() {
            Err(e) => {
                if manual {
                    info(&format!("Update check failed.\n\n{e}"));
                }
            }
            Ok(rel) if !is_newer(&rel.version, VERSION) => {
                if manual {
                    info(&format!("PingBreath {VERSION} is up to date."));
                }
            }
            Ok(rel) => {
                let q = format!(
                    "PingBreath {} is available (you have {VERSION}).\n\nUpdate now? The app will restart.\n\nRelease notes: {}",
                    rel.version, rel.notes_url
                );
                if message_box(&q, MB_YESNO | MB_ICONQUESTION) == IDYES {
                    if let Err(e) = apply(&rel) {
                        info(&format!("Update failed.\n\n{e}"));
                    }
                }
            }
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_not_lexical() {
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.1.1", "0.1"));
    }

    #[test]
    fn prerelease_ordering() {
        assert!(is_newer("0.2.0", "0.2.0-beta.1"));
        assert!(!is_newer("0.2.0-beta.1", "0.2.0"));
        assert!(is_newer("0.2.0-beta.2", "0.2.0-beta.1"));
        assert!(is_newer("0.2.0-beta.1", "0.1.9"));
    }
}
