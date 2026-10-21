//! Embeds the application icon and the version metadata into the executable.
//!
//! The same `icon.ico` that the tray icon loads at runtime is compiled into
//! the binary here, so Explorer shows the right icon for the process and the
//! user never has to keep a loose file next to the executable.

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
