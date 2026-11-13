fn main() {
    slint_build::compile("ui/main.slint").expect("slint compile failed");

    // Embed the exe icon + version info (shows in Explorer, taskbar, Alt-Tab).
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "NetPulse");
        res.set("FileDescription", "NetPulse network quality widget");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed icon: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=ui/main.slint");
}
