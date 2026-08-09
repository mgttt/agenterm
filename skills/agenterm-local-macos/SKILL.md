---
name: agenterm-local-macos
description: Build, install, launch, and diagnose AgenTerm from a local macOS checkout. Use when a local AgenTerm build must appear as a native Dock application, when Finder opens a bare agenterm binary through Terminal with Last login output, or when validating the local app bundle without weakening signed Release installation rules.
---

# AgenTerm Local macOS

## Build and install

Run from the repository root:

```bash
./build.sh
./install.sh --local-build target/debug
open ~/Applications/AgenTerm.app
```

Use `target/release-fast` after `./build.sh release-fast`. The installer
validates every required executable and derives the version from
`agenterm cli --version`.

## Install before claiming a GUI fix works

`cargo build` alone changes nothing the user can run. `~/Applications/AgenTerm.app`
is a symlink shell pointing at `~/.local/share/agenterm/current`, so until the
installer moves that link the Dock icon, `open`, and every `~/.local/bin`
command still launch the previously installed build.

A fix verified only by `cargo test` and by running `target/release/agenterm`
directly is **not** verified for the user. Reporting it as fixed will be wrong
in the most confusing possible way: the user runs their normal launch path,
sees the old behaviour, and the code looks correct when you re-read it.

Install, then prove the binary actually carries the change:

```bash
cargo build --release --bin agenterm --bin agenterm-rh
./install.sh --local-build target/release
readlink ~/.local/share/agenterm/current          # must be the new release dir
stat -f "%Sm" ~/Applications/AgenTerm.app/Contents/MacOS/AgenTerm
strings ~/.local/share/agenterm/current/agenterm | grep -c "<a new string literal>"
```

The `strings` check is the one that cannot be fooled by a stale symlink: pick a
message the fix introduced and confirm it is present in the installed bytes.

`--local-build` requires **both** installer-validated executables
(`agenterm`, `agenterm-rh`); building
only `agenterm` fails the installer's validation. CLI verbs ride the main PE
as `agenterm cli <command>` — there is no separate `agenterm-cli`. Live `.rh`
task automation uses `agenterm-rh` (build separately or via `./build.sh`).

An already-running server keeps its loaded version. After installing, close the
window with **stop server and exit** (not *keep server running*), or run
`agenterm cli shutdown`, before testing.

## Know what you cannot verify from the CLI

GUI text selection is mouse-driven. `agenterm cli ui-interact select` selects a
*window*, not terminal text, and `send-mouse` targets the application's xterm
mouse protocol rather than the frontend's selection layer. There is no CLI path
that reproduces a drag- or shift-select gesture.

So a selection or clipboard fix can be verified up to the seam — clipboard
round-trip, selection-text extraction, anchor math — but the final gesture needs
a human. Say that plainly instead of implying the whole path was tested.

Useful seam-level checks that *are* scriptable:

```bash
# Clipboard layer round-trip, including CJK and emoji
printf '' | pbcopy; <drive set_text>; pbpaste

# What the pasteboard really holds (pbpaste lies by omission)
osascript -e 'clipboard info'
```

`osascript -e 'clipboard info'` reporting bytes while `pbpaste | wc -c` reports
`0` is the signature of content that is not plain text, or of `pbpaste` silently
degrading — not of an empty clipboard.

## Diagnose Dock launches

If reopening AgenTerm shows Terminal text such as `Last login` and an executable
path followed by `; exit;`, the Dock item targets a bare Mach-O executable.
Finder routes that item through Terminal because it is not an application
bundle.

Fix the entry identity:

1. Remove the old raw-binary item from the Dock.
2. Run the local installer.
3. Open `~/Applications/AgenTerm.app`.
4. Keep that application in the Dock.

Do not pin `target/debug/agenterm` or `target/release-fast/agenterm`.

## Verify

```bash
test -x ~/Applications/AgenTerm.app/Contents/MacOS/AgenTerm
~/.local/bin/agenterm cli --version
plutil -lint ~/Applications/AgenTerm.app/Contents/Info.plist
```

Launch with `open ~/Applications/AgenTerm.app`. Confirm the GUI remains alive
and no Terminal window is created. Choose **Keep server running** when closing
the window, then click the Dock icon: the existing process must restore and
focus the hidden window. Verify the app uses the AgenTerm icon rather than the
generic executable icon.

Do not treat a second `open ~/Applications/AgenTerm.app` as proof that a Dock
click works. Launch Services wakes the winit loop for `open`, while a Dock
activation may not emit a window event after the only window is hidden. The
macOS backend must hide the application and keep a short `WaitUntil` poll while
hidden. A Dock click unhides the application; that hidden-to-visible state is
the reliable reopen signal even when the click follows the close immediately.
Verify with an actual Dock click and confirm the process ID remains unchanged.

## Diagnose terminal control keys

On macOS, winit can report Enter, Backspace, and Escape with both a named key
and control-character text (`CR`, `DEL`, or `ESC`). Named keys must take
precedence over committed text because the text path intentionally rejects
control characters. Terminal Control combinations such as Ctrl-H must be
encoded before macOS primary-shortcut classification; Command remains the
platform primary shortcut for application commands.

Keep a regression table for Enter (`0d`), Backspace (`7f`), Escape (`1b`), and
Ctrl-H (`08`). For native verification, run a PTY byte reader in a tab and test
physical key presses; synthetic modifier-only events are not proof of a real
Ctrl-H key chord.

## Preserve trust boundaries

`--local-build` installs unsigned bytes built by the user. Keep the default
no-argument installer unchanged: it must download checksums and enforce
Developer ID signatures for stable macOS Release packages. Never use local
mode as a substitute for Release signing, notarization, Candidate
qualification, or Promotion.
