fn main() {
    println!("cargo:rerun-if-changed=assets/agenterm.ico");
    println!("cargo:rerun-if-changed=assets/skins/fancy/icon.png");
    println!("cargo:rerun-if-changed=assets/skins/fancy/icon.ico");
    // agenterm-com is a no_std/no_main trampoline exporting a custom
    // `mainCRTStartup`. link.exe infers the subsystem from a standard
    // `main`/`WinMain` symbol; with neither present the inference fails
    // (LNK1561) before the custom entry is ever considered, so the
    // subsystem must be stated. Once it is, CONSOLE's default entry name
    // resolves to the exported symbol — no explicit /ENTRY needed.
    #[cfg(windows)]
    println!("cargo:rustc-link-arg-bin=agenterm-com=/SUBSYSTEM:CONSOLE");
    // no_std leaves three externs unresolved on MSVC: core's memcpy/memcmp
    // references and the unwind personality __CxxFrameHandler3. They come
    // from the CRT import libs; pulling them via /DEFAULTLIB cannot clash
    // with the bin's own `mainCRTStartup` because default libraries are
    // only searched for still-unresolved symbols, and the entry is already
    // defined in the bin's object file.
    #[cfg(windows)]
    println!("cargo:rustc-link-arg-bin=agenterm-com=/DEFAULTLIB:vcruntime");
    #[cfg(windows)]
    println!("cargo:rustc-link-arg-bin=agenterm-com=/DEFAULTLIB:ucrt");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/agenterm.ico")
        .compile()
        .expect("failed to embed AgenTerm icon");
}
