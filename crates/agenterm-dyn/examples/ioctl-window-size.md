<!--
CU hand: windows — terminal rows and columns are window-adjacent geometry.
This remains a script probe because dyn can call ioctl but does not own a cu
window target, allocate typed native structs, or publish geometry results.
-->

# Probe terminal window size with `ioctl`

Before evaluation, the Rust host must bind `winsize` to writable storage for a
native `struct winsize`. `terminal_fd` below is file descriptor 0; a host may
substitute another open terminal descriptor. Linux `TIOCGWINSZ` is decimal
`21523`. macOS uses `libSystem.B.dylib` and `TIOCGWINSZ` decimal `1074295912`
(`0x40087468`).

```lisp
(do
  (set terminal_fd 0)
  (set request 21523)
  (set status
    (dlcall "libc.so.6" "ioctl" "i32"
      "i32" terminal_fd
      "u64" request
      "ptr" winsize))
  (if (= status 0)
    winsize
    status))
```

On macOS, keep the same bound `winsize` pointer but use the Darwin library and
request value:

```lisp
(do
  (set terminal_fd 0)
  (set request 1074295912)
  (set status
    (dlcall "libSystem.B.dylib" "ioctl" "i32"
      "i32" terminal_fd
      "u64" request
      "ptr" winsize))
  (if (= status 0)
    winsize
    status))
```

The return value is the bound pointer on success and the native status on
failure; the embedding Rust code reads the rows and columns from that storage.
