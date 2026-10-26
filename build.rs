//! Embeds the application icon, manifest and version metadata in Windows builds.
//!
//! The window and tray load the embedded icon at runtime, so the executable
//! does not need a separate icon file.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-changed=app.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon("icon.ico");
    res.set_manifest_file("app.manifest");
    res.compile()?;
    Ok(())
}
