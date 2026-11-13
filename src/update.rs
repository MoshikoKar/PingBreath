//! Self-update from GitHub Releases. Uses the curl.exe / PowerShell that ship with Windows,
//! so no extra HTTP/TLS dependencies are needed.
//!
//! Release assets expected (see build.ps1 / .github/workflows/release.yml):
//!   NetPulse-Setup.exe     installer      -> used when installed via the setup
//!   NetPulse-portable.zip  NetPulse.exe + portable.txt -> used in portable mode

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

pub const REPO: &str = "MoshikoKar/NetPulse";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const NO_WINDOW: u32 = 0x0800_0000;

/// Set by the updater thread once the replacement has been launched; the UI timer then quits.
pub static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);

struct Release {
    version: String,
    url: String,
    notes_url: String,
}

fn parse(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn is_newer(remote: &str, local: &str) -> bool {
    let (mut a, mut b) = (parse(remote), parse(local));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

fn installed_via_setup() -> bool {
    crate::config::exe_dir().is_some_and(|d| d.join("unins000.exe").exists())
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("curl.exe")
        .args(["-sSL", "--fail", "--max-time", "120", "-H", "User-Agent: NetPulse-updater"])
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
    let body = curl(&[&format!("https://api.github.com/repos/{REPO}/releases/latest")])
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    let j: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let tag = j["tag_name"].as_str().ok_or("release has no tag")?.to_string();
    let wanted = if installed_via_setup() { "NetPulse-Setup.exe" } else { "NetPulse-portable.zip" };
    let url = j["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == wanted))
        .and_then(|x| x["browser_download_url"].as_str())
        .ok_or_else(|| format!("release {tag} has no {wanted} asset"))?
        .to_string();
    Ok(Release { version: tag.trim_start_matches('v').to_string(), url, notes_url: j["html_url"].as_str().unwrap_or("").to_string() })
}

fn message_box(text: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (t, c) = (wide(text), wide("NetPulse"));
    unsafe { MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), flags | MB_TOPMOST | MB_SETFOREGROUND) }
}

fn info(text: &str) {
    message_box(text, MB_OK | MB_ICONINFORMATION);
}

fn apply(rel: &Release) -> Result<(), String> {
    let tmp = std::env::temp_dir().join("NetPulse-update");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    if installed_via_setup() {
        let setup = tmp.join("NetPulse-Setup.exe");
        curl(&["-o", setup.to_str().unwrap_or_default(), &rel.url]).map_err(|e| format!("Download failed: {e}"))?;
        Command::new(&setup)
            .args(["/SILENT", "/CLOSEAPPLICATIONS", "/NORESTART"])
            .spawn()
            .map_err(|e| format!("Could not start installer: {e}"))?;
    } else {
        let zip = tmp.join("NetPulse-portable.zip");
        curl(&["-o", zip.to_str().unwrap_or_default(), &rel.url]).map_err(|e| format!("Download failed: {e}"))?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // Wait for this process to exit, swap the exe, relaunch. Paths go via env vars (no quoting issues).
        let script = "Wait-Process -Id $env:NP_PID -ErrorAction SilentlyContinue; \
                      Expand-Archive -LiteralPath $env:NP_ZIP -DestinationPath $env:NP_DIR -Force; \
                      Copy-Item -LiteralPath (Join-Path $env:NP_DIR 'NetPulse.exe') -Destination $env:NP_EXE -Force; \
                      Start-Process -FilePath $env:NP_EXE";
        Command::new("powershell.exe")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-Command", script])
            .env("NP_PID", std::process::id().to_string())
            .env("NP_ZIP", &zip)
            .env("NP_DIR", tmp.join("x"))
            .env("NP_EXE", &exe)
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
                    info(&format!("NetPulse {VERSION} is up to date."));
                }
            }
            Ok(rel) => {
                let q = format!(
                    "NetPulse {} is available (you have {VERSION}).\n\nUpdate now? The app will restart.\n\nRelease notes: {}",
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
