# Default shell (`agenterm-bash.exe`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- Product contract
  - [ ] stable AgenTerm-owned executable path, terminal integration, error
    messages, version output, and backend discovery
  - [ ] backed by a real Bash runtime; AgenTerm will not label a partial
    home-grown parser as Bash
  - [ ] remains usable outside AgenTerm as a normal console executable
  - [ ] inside a tab receives scoped `AGENTERM_IPC_ADDRESS`, stable tab ID,
    session name, and workspace metadata without embedding credentials
  - [ ] becomes the new-tab default only after the clean-machine acceptance
    gate passes; `cmd.exe` remains the honest fallback before that point
- Runtime strategy gate
  - [ ] compare an installed-runtime resolver, a redistributable minimal
    Bash bundle, and a native compatibility implementation for license,
    fresh-machine reliability, process model, startup, update size, CJK,
    Ctrl-C/signals, path translation, and security
  - [ ] prefer a small launcher plus verified real Bash distribution unless
    measurements show that deployment or process behavior is unacceptable
  - [ ] runtime resolution is explicit and inspectable through
    `agenterm-bash --runtime-info`; never silently substitute `cmd.exe`
  - [ ] runtime installation/update is checksum-verified, version-pinned,
    transactional, and separate from GUI startup
- Compatibility acceptance
  - [ ] interactive editing, history, completion, UTF-8/CJK, resize,
    bracketed paste, Ctrl-C/Ctrl-D, and correct exit status
  - [ ] quoting, variables, functions, command substitution, pipelines,
    redirection, conditionals, loops, traps, and representative `.sh` files
    execute in the selected real Bash runtime
  - [ ] Windows path and executable launching rules are documented and
    tested without pretending POSIX and Win32 paths are identical
  - [ ] shell exit leaves the AgenTerm tab visible and explicitly closable
