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

## How the Release installer handles a missing signed asset

Running `./install.sh` without `--local-build` selects the Release installer.
On macOS, when the signed release archive is not available it will
automatically fall back to the `-unsigned-preview` package and prints a trust
warning that cannot be skipped. If Gatekeeper blocks launch, open
System Settings → Privacy & Security and choose **Open Anyway** for
`~/Applications/AgenTerm.app`.

Only an explicit HTTP 404 or 410 for the signed asset permits this fallback.
Transport, authentication, rate-limit, and server failures stop the install
instead of silently downgrading it.

For a source checkout, use `--local-build target/debug`. Your local build is
unsigned-but-local and still does not change release-channel trust decisions.

Older commands may still set `AGENTERM_ALLOW_UNSIGNED_PREVIEW=1`. It is now a
compatibility acknowledgment only: it does not force the preview, suppress the
warning, skip signed-asset verification, or alter the install record.

For an optimized local build, use:

```bash
./build.sh release-fast
./install.sh --local-build target/release-fast
open ~/Applications/AgenTerm.app
```

Local builds are unsigned bytes produced on your machine. Release checksum,
signature, notarization, Candidate, and Promotion rules remain unchanged.
