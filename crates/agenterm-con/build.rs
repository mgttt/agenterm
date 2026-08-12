fn main() {
    const ICON: &str = "../../assets/agenterm-con.ico";
    const ICON_BUDGET: u64 = 16 * 1024;
    println!("cargo:rerun-if-changed={ICON}");
    let icon_bytes = std::fs::metadata(ICON)
        .expect("agenterm-con icon is missing")
        .len();
    assert!(
        icon_bytes <= ICON_BUDGET,
        "agenterm-con icon is {icon_bytes} bytes; compact resource budget is {ICON_BUDGET}"
    );
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=dylib=vcruntime");
        println!("cargo:rustc-link-lib=dylib=ucrt");
        println!("cargo:rustc-link-arg-bin=agenterm-con=/ENTRY:agenterm_con_entry");
        // `startup.rs` explicitly walks XI/XC/XP/XT while the PE loader owns
        // XL through the TLS Directory. The default CRT entry is intentionally
        // absent, so link.exe cannot infer that every `.CRT` family is handled.
        println!("cargo:rustc-link-arg-bin=agenterm-con=/IGNORE:4210");
        winresource::WindowsResource::new()
            .set_icon(ICON)
            .compile()
            .expect("failed to embed AgenTerm icon");
    }
}
