fn main() {
    // Child mode (see below): only produce the icon resource, letting
    // winresource print its normal `cargo:rustc-link-arg=<resource.res>`
    // line to OUR captured stdout, then exit.
    if std::env::var_os("AGENTERM_BUILDRS_RESOURCE_CHILD").is_some() {
        #[cfg(windows)]
        winresource::WindowsResource::new()
            .set_icon("assets/agenterm.ico")
            .compile()
            .expect("failed to embed AgenTerm icon");
        return;
    }

    println!("cargo:rerun-if-changed=assets/agenterm.ico");
    println!("cargo:rerun-if-changed=assets/skins/fancy/icon.png");
    println!("cargo:rerun-if-changed=assets/skins/fancy/icon.ico");
    // agenterm-com is a no_std/no_main trampoline exporting a custom
    // `mainCRTStartup`. link.exe infers the subsystem from a standard
    // `main`/`WinMain` symbol; with neither present it stops at LNK1561
    // before the custom entry is ever considered, so the subsystem must be
    // stated. Once it is, CONSOLE's default entry name resolves to the
    // exported symbol — no explicit /ENTRY needed.
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

    // Embed the icon into every bin EXCEPT agenterm-com. The icon .rsrc is
    // ~59KiB and the trampoline's staged-size budget is 64KiB total
    // (scripts/artifacts.json, enforced by stage-build.rh in every profile);
    // its code is ~15KiB, so the icon alone is what would blow the budget.
    //
    // winresource only offers compile(), which emits a GLOBAL
    // `cargo:rustc-link-arg=` (applies to all bins, no exclusion). So: rerun
    // this build script as a child process in resource-only mode, capture
    // that line from its stdout, and re-emit it as per-bin
    // `cargo:rustc-link-arg-bin=` for exactly the icon-carrying bins. A new
    // [[bin]] that should carry the icon must be added to this list (missing
    // it costs the icon, nothing else).
    #[cfg(windows)]
    {
        const ICON_BINS: &[&str] = &[
            "agenterm",
            "agenterm-cc",
            "agenterm-con",
        ];
        let me = std::env::current_exe().expect("build script path");
        let output = std::process::Command::new(me)
            .env("AGENTERM_BUILDRS_RESOURCE_CHILD", "1")
            .output()
            .expect("resource child failed to run");
        if !output.status.success() {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            panic!("failed to embed AgenTerm icon (resource child)");
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut resource_path = None;
        for line in stdout.lines() {
            if let Some(path) = line.strip_prefix("cargo:rustc-link-arg=") {
                resource_path = Some(path.trim().to_owned());
            }
            // Every other line is winresource's informational output (RC
            // selection, rc.exe stdout/stderr) — deliberately dropped so no
            // stray global cargo directive reaches cargo.
        }
        let resource_path =
            resource_path.expect("winresource did not emit a resource link-arg");
        for bin in ICON_BINS {
            println!("cargo:rustc-link-arg-bin={bin}={resource_path}");
        }
    }
}
