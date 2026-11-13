//! Small Windows helpers: single instance, off-screen guard, start-with-Windows, toasts.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use windows::core::w;

pub const NO_WINDOW: u32 = 0x0800_0000;
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "NetPulse";

/// Returns a guard handle (keep it alive for the whole run), or None if another instance runs.
/// With `wait`, retries for ~10 s so a restart can take over from the exiting instance.
pub fn single_instance(wait: bool) -> Option<HANDLE> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // Read the last error immediately: it is how CreateMutexW reports "already exists".
        let (handle, last) = unsafe {
            let h = CreateMutexW(None, true, w!("Local\\NetPulse.SingleInstance"));
            (h, GetLastError())
        };
        let exists = last == ERROR_ALREADY_EXISTS
            || matches!(&handle, Err(e) if e.code() == windows::core::HRESULT::from_win32(ERROR_ALREADY_EXISTS.0));
        if exists {
            if let Ok(h) = handle {
                unsafe {
                    let _ = CloseHandle(h);
                }
            }
            if wait && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            return None;
        }
        return Some(handle.unwrap_or_default()); // on other errors, do not block startup
    }
}

/// True if a window whose top-left is (x, y) would be reachable on some monitor.
pub fn position_visible(x: i32, y: i32) -> bool {
    unsafe {
        let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        x >= vx - 40 && y >= vy && x < vx + vw - 80 && y < vy + vh - 60
    }
}

fn reg(args: &[&str]) -> bool {
    Command::new("reg.exe")
        .args(args)
        .creation_flags(NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The Run key is the source of truth (the installer's "start with Windows" task writes it too).
pub fn startup_enabled() -> bool {
    reg(&["query", RUN_KEY, "/v", RUN_VALUE])
}

pub fn set_startup(on: bool) {
    if on {
        if let Ok(exe) = std::env::current_exe() {
            let value = format!("\"{}\"", exe.display());
            reg(&["add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &value, "/f"]);
        }
    } else {
        reg(&["delete", RUN_KEY, "/v", RUN_VALUE, "/f"]);
    }
}

/// Fire-and-forget Windows toast (via the PowerShell that ships with Windows).
pub fn toast(title: &str, body: &str) {
    let script = "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; \
        $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
        $n = $t.GetElementsByTagName('text'); \
        $n.Item(0).AppendChild($t.CreateTextNode($env:NP_TITLE)) | Out-Null; \
        $n.Item(1).AppendChild($t.CreateTextNode($env:NP_BODY)) | Out-Null; \
        [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe').Show([Windows.UI.Notifications.ToastNotification]::new($t))";
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("NP_TITLE", title)
        .env("NP_BODY", body)
        .creation_flags(NO_WINDOW)
        .spawn();
}

#[cfg(test)]
mod tests {
    #[test]
    fn second_acquire_fails_while_first_is_held() {
        let first = super::single_instance(false);
        assert!(first.is_some(), "first acquire must succeed");
        assert!(super::single_instance(false).is_none(), "second acquire must be refused");
    }
}
