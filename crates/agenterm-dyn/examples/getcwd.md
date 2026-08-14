<!--
CU hand: windows — a target's working directory is useful context near window
and process discovery. This remains a script probe because dyn has no cu target
record, directory decoder, or windows command wiring.
-->

# Read the current working directory

Before evaluation, the Rust host must bind `cwd_buffer` to writable storage of
at least `buffer_len` bytes. Linux uses `libc.so.6`; macOS uses
`libSystem.B.dylib` with the same `getcwd` signature.

```lisp
(do
  (set buffer_len 4096)
  (set result
    (dlcall "libc.so.6" "getcwd" "ptr"
      "ptr" cwd_buffer
      "u64" buffer_len))
  (if result
    result
    0))
```

On success the result is the bound buffer pointer and the host may decode its
NUL-terminated bytes. A null result stays `0`; the script does not invent a
directory or hide the native failure.
