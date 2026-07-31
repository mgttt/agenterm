fn main() {
    // tauri-build requires a Windows resource icon even when bundling is off.
    // Keep this comparison self-contained with a tiny generated ICO in OUT_DIR;
    // it is build metadata, not a product icon or distributed asset.
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let icon = output.join("reference.ico");
    std::fs::write(&icon, reference_icon()).expect("write generated reference icon");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&[]))
            .windows_attributes(tauri_build::WindowsAttributes::new().window_icon_path(icon)),
    )
    .expect("build minimal Tauri reference without application commands");
}

fn reference_icon() -> &'static [u8] {
    &[
        0, 0, 1, 0, 1, 0, // ICONDIR
        1, 1, 0, 0, 1, 0, 32, 0, 48, 0, 0, 0, 22, 0, 0, 0, // entry
        40, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 32, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // BITMAPINFOHEADER
        196, 215, 123, 255, // one BGRA pixel
        0, 0, 0, 0, // AND mask row
    ]
}
