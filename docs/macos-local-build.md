# Build and install AgenTerm locally on macOS

繁體中文：[在 macOS 本機建置與安裝 AgenTerm](macos-local-build.zh-Hant.md).

Use this path when running AgenTerm from a source checkout. It creates a real
application bundle at `~/Applications/AgenTerm.app`; do not pin the bare
`target/debug/agenterm` executable in the Dock.

```bash
./build.sh
./install.sh --local-build target/debug
open ~/Applications/AgenTerm.app
```

After the app opens, keep **AgenTerm.app** in the Dock. The installer also
copies the local build into a versioned directory under
`~/.local/share/agenterm` and refreshes commands under `~/.local/bin`.

## Why plain `./install.sh` may return 404

Running `./install.sh` without `--local-build` selects the Release installer.
It downloads the package for the current version and requires a signed macOS
asset for the stable channel. If that Release asset has not been published,
the installer reports HTTP 404 and exits without changing the active install.

For a source checkout, use `--local-build target/debug`; do not set
`AGENTERM_ALLOW_UNSIGNED_PREVIEW=1`. That environment variable is only an
explicit opt-in for a published unsigned-preview archive and is not needed for
your own local build.

For an optimized local build, use:

```bash
./build.sh release-fast
./install.sh --local-build target/release-fast
open ~/Applications/AgenTerm.app
```

Local builds are unsigned bytes produced on your machine. Release checksum,
signature, notarization, Candidate, and Promotion rules remain unchanged.
