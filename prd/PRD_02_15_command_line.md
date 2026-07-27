# Command line (`agenterm-cli.exe`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- Shared grammar
  - target: `-t @id`, `-t %id`, `-t index`, or `-t exact-name`
  - format: `-F FORMAT`; supports `#S`, `#I`, `#W`, `#P` and
    `#{session_name}`, `#{window_*}`, `#{pane_*}`, `#{terminal_title}`,
    and `#{tab_parent_id}`; `list-tab-tree` also supports `#{tab_depth}`
    and `#{tab_has_children}`
  - stable IDs are preferred; numeric indexes may change after closing tabs
- tmux/RMUX-aligned commands
  - Session/server
    - `new-session|new [-s name] [command [args...]]`
    - `attach-session|attach`, `start-server`
    - `list-sessions|ls`, `has-session|has [-t target]`
    - `rename-session|rename name`
    - `kill-session`, `kill-server`
  - Windows mapped to AgenTerm tabs
    - `new-window|neww [-d] [-n name] [--parent target] [-F format]
      [command [args...]]`
    - [x] `new-window -F` can return the new tab's stable ID through
      `#{window_id}` while default numeric-index output remains compatible
    - `list-windows|lsw [-F format]`
    - `select-window|selectw -t target`
    - `next-window|next`, `previous-window|prev`
    - `rename-window|renamew [-t target] name`
    - `kill-window|killw -t target`
  - Single pane per tab
    - `list-panes|lsp [-t target] [-F format]`
    - `send-keys|send [-t target] [-l] key...`
    - `capture-pane|capturep -p [-t target]`
    - `display-message|display -p [-t target] format`
    - `show-options|show`, `list-commands|lscm`
    - [x] `split-window|splitw` returns an explicit unsupported error
- AgenTerm extensions
  - Team tree
    - `list-tab-tree [-F format]`
    - `set-tab-parent -t child --parent parent|root`
    - `show-tab-parent [-t target]`
    - `ui-action new-child [-t parent]`
    - parent cycles fail explicitly
    - closing a parent promotes direct children to its parent
  - State and deterministic waits
    - `list-instances [--json] [--prune]`
    - [x] `server-list [--json] [--prune]` is an offline fleet-discovery
      alias over the same registered-instance records; it never autostarts
      a GUI, automatically removes definitively dead PID records, retains
      live-but-unreachable records for diagnosis, and therefore provides
      the read-side companion to `kill-server`
    - [x] `server-kill` is a client-side alias that canonicalizes to the
      existing `kill-server` operation before IPC dispatch; it preserves
      the same destructive workspace and process-lifecycle semantics
    - global `--address HOST:PORT` targets a discovered server explicitly
    - `active-window|active-tab [-F format]`
    - `inspect|pane-snapshot [-t target]`
    - `dump-cells [-t target] [-r row]`
    - `capture-pane --raw-escaped [-t target]`
    - `scroll-pane [-t target]
      up|down|page-up|page-down|top|bottom [rows]`
    - `read-events --epoch EPOCH --after SEQUENCE [--limit COUNT]`
    - `wait-events --epoch EPOCH --after SEQUENCE --kind KIND
      [--tab @ID] [--timeout-ms MS]`
    - `wait-pane|expect-pane [-t target]
      [--contains text|--dead|--submit-complete]
      [--timeout-ms ms]`
    - `ui-snapshot`, `protocol-info`
    - `workspace-info`, `save-workspace`, `shutdown`
    - [~] `server-list` and `server-kill` establish a `server-*` lifecycle
      namespace; explore health, start, and graceful-shutdown helpers only
      as aliases over typed operations, without creating a second server
      registry or weakening the `kill-server` destruction contract
    - [ ] `shutdown --no-save` escape hatch for instances whose workspace
      destination has become unwritable
    - `wait-ui [--active @id] [--focus surface] [-t target
      --tab-state running|dead] [--modal-kind KIND|none|closed]
      [--modal-target target] [--timeout-ms ms]`
  - Safe scripting
    - `script api [--json]`
    - `script check FILE|- [--profile pure|observe]`
    - `script eval EXPRESSION [--profile pure|observe]`
    - `script run FILE|- [--profile pure|observe] [-- ARGS...]`
  - Composer and tab metadata
    - `show-composer [-t target]`
    - `set-composer [-t target] text|--stdin|--file path`
    - `send-composer [-t target]`
    - `set-tab-note [-t target] text`, `show-tab-note [-t target]`
  - Semantic UI control
    - `focus terminal|composer|tabs [-t target]` (`sidebar` remains an alias)
    - `ui-action new-tab|new-child|edit-tab|toggle-tree|tabs-show|tabs-hide|tabs-toggle|toggle-tabs|tabs-set-width|select-tab|close-tab|confirm|cancel|
      composer-send|copy-selection|open-settings|window-minimize|
      window-maximize|window-restore [-t target]`
    - `ui-action window-resize --width PX --height PX`
    - [x] semantic actions control window state and client size without corrupting the PTY grid
    - [x] `wait-ui` directly waits for modal kind, target, or closed state with a stable timeout code
      - Settings, confirmation, CWD, and proxy surfaces are addressable by
        kind/target; `none` and `closed` both mean no modal, and timeout
        failures expose the stable `ui_wait_timeout` code
  - Visual and terminal diagnostics
    - `screenshot [-o path.png]`
    - `screenshot-pane|screenshot-tab [-t target] [-o path.png]`
    - `send-mouse [-t target] -x col -y row [--button
      left|middle|right|wheel-up|wheel-down] [--action press|release]
      [--protocol auto|sgr|native]`
  - Settings
    - `get-settings`
    - `set-setting terminal.font-family FAMILY`
    - `set-setting terminal.font-size 8..36`
  - Planned scripting
    - `script run [OPTIONS] FILE.rhai|- [--] [ARGS...]`
    - `script eval [OPTIONS] EXPRESSION`
    - `script check FILE.rhai`
    - `script api [--json]`
    - `script cache clear|info`
    - options include `--profile pure|observe|control`, repeated `--cap`,
      scoped resources, `--timeout-ms`, and `--max-output`
    - exit codes distinguish script failure, configuration/capability
      error, timeout, and host/API failure
  - AI fleet launch
    - `new-agent [-d] [-n name] [--parent target] [--program executable]
      [-e NAME=VALUE] [--proxy URL] [--no-proxy hosts] [--yolo]
      [-- codex args...]`
    - `new-window` and `new-session` also accept repeated `-e NAME=VALUE`
    - injected values live only for the child process; snapshots expose
      names, and workspace persistence stores neither names nor values
    - every child receives reserved `AGENTERM_IPC_ADDRESS`,
      `AGENTERM_TAB_ID`, `AGENTERM_SESSION`, and
      `AGENTERM_WORKSPACE_PATH`
    - the default launcher uses the system `codex` command through
      `cmd.exe` so standard npm `.cmd` installations work in ConPTY;
      `--program` is the explicit direct-executable override
    - `--yolo` explicitly maps to Codex
      `--dangerously-bypass-approvals-and-sandbox`; the default remains safe
