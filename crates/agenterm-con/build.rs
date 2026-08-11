fn main() {
    println!("cargo:rerun-if-changed=../../assets/agenterm.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/agenterm.ico")
            .compile()
            .expect("failed to embed AgenTerm icon");
    }
}
