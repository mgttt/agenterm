fn main() {
    println!("cargo:rerun-if-changed=assets/agenterm.ico");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/agenterm.ico")
        .compile()
        .expect("failed to embed AgenTerm icon");
}
