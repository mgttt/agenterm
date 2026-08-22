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
        // The MSVC runtime is three separable pieces: the C startup files, the
        // VC runtime, and the Universal CRT. They normally have to share a
        // linkage model, with one documented exception -- startup and VC
        // runtime static, UCRT dynamic -- which is exactly the combination
        // this product wants. The UCRT is a Windows component and is present
        // on every supported system; VCRUNTIME140.dll is a redistributable
        // and is not, so a dynamic reference to it makes a clean Windows
        // Server 2016 refuse to start the program.
        //
        // Mixing the models is what fails loudly: leaving msvcrt.lib (the
        // dynamic startup) alongside libvcruntime.lib links, then dies at
        // STATUS_STACK_BUFFER_OVERRUN. The exclusions below are not
        // decoration, they are the half that makes this work. Same directive
        // set as the static_vcruntime crate, inlined rather than taken as a
        // dependency because it is six linker flags and this build script is
        // already the owner of this binary's link line.
        // The release set in every profile, with no debug-CRT variant: Rust
        // links the release CRT on windows-msvc regardless of profile, so
        // naming `libvcruntimed.lib` here leaves `__CxxFrameHandler3`
        // unresolved and the dev build stops linking.
        for library in ["vcruntime.lib", "msvcrt.lib", "libucrt.lib"] {
            println!("cargo:rustc-link-arg=/NODEFAULTLIB:{library}");
        }
        for library in ["libcmt.lib", "libvcruntime.lib", "ucrt.lib"] {
            println!("cargo:rustc-link-arg=/DEFAULTLIB:{library}");
        }
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
