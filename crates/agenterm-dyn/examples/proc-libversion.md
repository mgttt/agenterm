# Read libproc version with `proc_libversion`

macOS example. Bind caller-owned signed 32-bit output slots as `major` and
`minor`; a zero status writes the installed libproc version.

```lisp
(dlcall "libSystem.B.dylib" "proc_libversion" "i32" "ptr" major "ptr" minor)
```

The embedding host owns both output slots and must initialize and retain them
for the duration of the call.
