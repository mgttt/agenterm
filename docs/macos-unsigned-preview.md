# AgenTerm macOS unsigned preview

Product owner:
[Delivery and quality](../prd/PRD_02_17_delivery_quality.md).
繁體中文：[macOS 未簽署預覽版](macos-unsigned-preview.zh-Hant.md).

This archive is an **unsigned and unnotarized developer preview**. It is not
the macOS stable channel. Apple has not verified the publisher or scanned this
build through the Apple Notary Service.

Use it only if you understand that overriding macOS security for unchecked
software carries additional risk.

Only when the signed macOS asset returns HTTP `404` or `410` does `install.sh`
automatically fall back to the separately named `-unsigned-preview` archive.
Every other download error, including transport, authentication, rate-limit,
and server failures, fails closed. The installer always prints the trust
warning to stderr before downloading the preview; there is no option that
suppresses the warning.

For a signed release, the installer requires provenance to state
`channel=release`, `signed=true`, and `notarized=true`. It also performs a
strict local Apple code-signature check on every required executable and
requires each signature to identify an Apple Developer ID Application
authority. Signed-release trust is therefore inferred from both the provenance
claims and local executable verification, rather than from either signal alone.

`AGENTERM_ALLOW_UNSIGNED_PREVIEW=1` is retained only as a compatibility
acknowledgment for older install commands. It does not force unsigned bytes,
skip provenance or local signature verification, suppress the warning, or
change the install record.

## Verify the download first

Download these three files for your architecture from the same GitHub Release:

```text
agenterm-…-macos-…-unsigned-preview.zip
agenterm-…-macos-…-unsigned-preview.zip.sha256
agenterm-…-macos-…-unsigned-preview.zip.provenance.json
```

In Terminal, change to the download directory and run:

```sh
shasum -a 256 -c agenterm-*-macos-*-unsigned-preview.zip.sha256
```

The result must say `OK`. The provenance JSON records the exact Git tag,
source commit, architecture, archive SHA-256, `Cargo.lock` hash, artifact
manifest hash, and GitHub Actions build-log URL. The same Release also provides
`agenterm-…-sbom.spdx.json`, the dependency inventory generated from the locked
release source.

The installer performs the same SHA-256 and provenance checks automatically.
For this preview it additionally requires provenance to state
`channel=macos-unsigned-preview`, `signed=false`, and `notarized=false`.

## Open the preview

1. Extract the ZIP.
2. Try to open `agenterm` once. macOS should block it and record the attempted
   app in security settings.
3. Open **Apple menu → System Settings → Privacy & Security**.
4. Scroll to **Security**, find the message about `agenterm`, and choose
   **Open Anyway**.
5. Authenticate when macOS asks, then confirm **Open**.

Apple normally makes **Open Anyway** available for about an hour after the
blocked launch attempt. See Apple’s current instructions:
<https://support.apple.com/guide/mac-help/mh40617/mac>.

The archive contains several executables. macOS may require a separate explicit
approval when you first run a CLI executable.

## Do not disable system-wide protection

Do not use `spctl --master-disable`, do not disable Gatekeeper globally, and do
not recursively remove quarantine attributes from unrelated files. This preview
is intentionally distributed through macOS’s per-app, explicit user-approval
path.

If macOS reports that the software **will damage your computer**, was moved to
Trash, or appears modified rather than merely unidentified, do not override the
warning. Delete the download and report the exact message.

## Report a problem

Include:

- macOS version and Mac architecture;
- the archive filename and SHA-256;
- the `source_commit` from the provenance JSON;
- the exact Gatekeeper message or terminal output;
- whether the GUI or a specific CLI executable failed.

Do not include passwords, tokens, proxy credentials, terminal contents, or
other secrets.
