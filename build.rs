// Puts the app icon into deskling.exe, so Explorer, the taskbar, Alt+Tab and the tray show it.
// GPUI loads its windows' icon from the exe's icon resource number 1, which `set_icon` creates.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            // The name Task Manager and "Open with" show; it's the lowercase package name otherwise.
            .set("FileDescription", "Deskling")
            .set("ProductName", "Deskling")
            .compile()
            .expect("couldn't embed the app icon (needs the Windows SDK's rc.exe)");
    }
}
