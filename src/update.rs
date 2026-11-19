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

/// GET (or download with `-o`) via curl; returns HTTP status and body (empty when `-o` is used).
fn curl(args: &[&str]) -> Result<(u16, Vec<u8>), String> {
    let out = Command::new("curl.exe")
        .args([
            "-sSL",
            "--max-time",
            "120",
            "-w",
            "\n%{http_code}",
            "-H",
            "User-Agent: PingBreath-updater",
        ])
        .args(args)
        .creation_flags(NO_WINDOW)
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "curl failed".into()
        } else {
            err
        });
    }
    let stdout = out.stdout;
    let nl = stdout
        .iter()
        .rposition(|&b| b == b'\n')
        .ok_or_else(|| "curl returned no HTTP status".to_string())?;
    let code: u16 = std::str::from_utf8(&stdout[nl + 1..])
        .unwrap_or("")
        .trim()
        .parse()
        .map_err(|_| "curl returned invalid HTTP status".to_string())?;
    Ok((code, stdout[..nl].to_vec()))
}

fn map_http(code: u16, context: &str) -> String {
    match code {
        404 => format!(
            "{context}: GitHub returned 404. If the repository is private, release metadata is not \
             reachable without auth (tokenless updater). Making Releases public, or publishing a \
             public update manifest, is required."
        ),
        403 | 429 => format!("{context}: GitHub rate limit or forbidden (HTTP {code}). Try again later."),
        401 => format!("{context}: GitHub unauthorized (HTTP 401)."),
        c if (500..600).contains(&c) => format!("{context}: GitHub server error (HTTP {c}). Try again later."),
        c => format!("{context}: HTTP {c}"),
    }
}

fn curl_body(url: &str) -> Result<Vec<u8>, String> {
    let (code, body) = curl(&[url])?;
    if code == 200 {
        Ok(body)
    } else {
        Err(map_http(code, "Could not reach GitHub"))
    }
}

fn curl_download(url: &str, dest: &std::path::Path) -> Result<(), String> {
    let (code, _) = curl(&["-o", &dest.to_string_lossy(), url])?;
    if code == 200 {
        Ok(())
    } else {
        let _ = std::fs::remove_file(dest);
        Err(map_http(code, "Download failed"))
    }
}

fn latest() -> Result<Release, String> {
    let beta = crate::config::load().update_channel.eq_ignore_ascii_case("beta");
    let api = if beta {
        format!("https://api.github.com/repos/{REPO}/releases?per_page=10")
    } else {
        format!("https://api.github.com/repos/{REPO}/releases/latest")
    };
    let body = curl_body(&api)?;
    let j: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    // beta channel: newest non-draft release of any kind (list is newest first)
    let j = if beta {
        j.as_array()
            .and_then(|a| a.iter().find(|r| r["draft"].as_bool() != Some(true)))
            .cloned()
            .ok_or_else(|| "no releases published".to_string())?
    } else {
        j
    };
    let tag = j["tag_name"].as_str().ok_or("release has no tag")?.to_string();
    let wanted = if installed_via_setup() {
        "PingBreath-Setup.exe"
    } else {
        "PingBreath-portable.zip"
    };
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
        sums_url: find("SHA256SUMS.txt").ok_or_else(|| {
            format!("release {tag} has no SHA256SUMS.txt - refusing to update unverified")
        })?,
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
    crate::sys::toast("PingBreath", &format!("Downloading {}...", rel.asset));
    let sums = curl_body(&rel.sums_url).map_err(|e| format!("Could not fetch checksums: {e}"))?;
    let expected = String::from_utf8_lossy(&sums)
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (hash, name) = (it.next()?, it.next()?);
            (name.trim_start_matches('*') == rel.asset).then(|| hash.to_lowercase())
        })
        .next()
        .ok_or_else(|| format!("SHA256SUMS.txt has no entry for {}", rel.asset))?;
    curl_download(&rel.url, dest)?;
    crate::sys::toast("PingBreath", "Verifying download...");
    let actual = sha256_of(dest)?;
    if actual != expected {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "Checksum mismatch - download discarded.\nexpected {expected}\nactual   {actual}"
        ));
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
        let ready = "Update ready - PingBreath will close and restart after the installer finishes.\n\nContinue?";
        if message_box(ready, MB_YESNO | MB_ICONINFORMATION) != IDYES {
            let _ = std::fs::remove_file(&setup);
            return Ok(());
        }
        Command::new(&setup)
            .args(["/SILENT", "/CLOSEAPPLICATIONS", "/NORESTART"])
            .spawn()
            .map_err(|e| format!("Could not start installer: {e}"))?;
    } else {
        let zip = tmp.join("PingBreath-portable.zip");
        download_verified(rel, &zip)?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let ready = "Update ready - PingBreath will close and restart.\n\nContinue?";
        if message_box(ready, MB_YESNO | MB_ICONINFORMATION) != IDYES {
            let _ = std::fs::remove_file(&zip);
            return Ok(());
        }
        // Extract PingBreath.exe, write it next to the running exe (renaming off a legacy
        // NetPulse.exe layout when needed), relaunch with retries, then remove the old exe name.
        // LEGACY(NetPulse): remove in v0.3.1 - the NetPulse.exe branch below.
        let script = "Wait-Process -Id $env:PB_PID -ErrorAction SilentlyContinue; \
                      Start-Sleep -Seconds 1; \
                      Expand-Archive -LiteralPath $env:PB_ZIP -DestinationPath $env:PB_DIR -Force; \
                      $dir = Split-Path -LiteralPath $env:PB_EXE; \
                      $target = Join-Path $dir 'PingBreath.exe'; \
                      Copy-Item -LiteralPath $env:PB_EXE -Destination ($env:PB_EXE + '.bak') -Force; \
                      Copy-Item -LiteralPath (Join-Path $env:PB_DIR 'PingBreath.exe') -Destination $target -Force; \
                      $ok = $false; \
                      for ($i = 0; $i -lt 5; $i++) { \
                        try { Start-Process -FilePath $target; $ok = $true; break } \
                        catch { Start-Sleep -Seconds 1 } \
                      }; \
                      if (-not $ok) { \
                        $log = Join-Path $env:TEMP 'PingBreath-update-fail.log'; \
                        \"Failed to relaunch $target\" | Set-Content -LiteralPath $log; \
                        Add-Type -AssemblyName System.Windows.Forms; \
                        [System.Windows.Forms.MessageBox]::Show( \
                          \"PingBreath updated but could not restart. Start it manually.`n`nLog: $log\", \
                          'PingBreath') \
                      }; \
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
                    "PingBreath {} is available (you have {VERSION}).\n\n\
                     Download and install now? This may take a minute; the app will close and restart.\n\n\
                     Release notes: {}",
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

    #[test]
    fn map_http_distinguishes_private_and_rate_limit() {
        let m404 = map_http(404, "Could not reach GitHub");
        assert!(m404.contains("404"));
        assert!(m404.contains("private") || m404.contains("public"));
        assert!(map_http(429, "Could not reach GitHub").contains("rate limit"));
        assert!(map_http(403, "Could not reach GitHub").contains("403"));
    }
}
