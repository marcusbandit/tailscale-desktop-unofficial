//! Gives the Windows exe its icon and file details. Other targets need nothing.

fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("packaging/windows/icon.ico")
        .set("ProductName", "Tailscale Desktop (unofficial)")
        .set("FileDescription", "Tailscale Desktop (unofficial)")
        .set(
            "LegalCopyright",
            "MIT license. Not made by or affiliated with Tailscale Inc.",
        );
    // Cross builds without a resource compiler still produce a working exe.
    if let Err(e) = res.compile() {
        println!("cargo:warning=no icon resource in the exe: {e}");
    }
}
