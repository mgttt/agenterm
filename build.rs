fn main() {
    println!("cargo:rerun-if-changed=assets/agenterm.ico");
    println!("cargo:rerun-if-changed=assets/skins/fancy/icon.png");
    println!("cargo:rerun-if-changed=assets/skins/fancy/icon.ico");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/agenterm.ico")
        .compile()
        .expect("failed to embed AgenTerm icon");
}
