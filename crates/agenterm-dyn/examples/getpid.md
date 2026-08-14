<!--
CU hand: windows — a PID is the smallest useful identity attached to a window
owner. This remains a script probe because dyn has no cu target/session model,
window enumeration, or libagenterm command wiring.
-->

# Read the current process ID

Linux example; macOS uses `libSystem.B.dylib`, while Windows uses
`kernel32.dll` / `GetCurrentProcessId` with a `u32` result.

```lisp
(do
  (set first (dlcall "libc.so.6" "getpid" "i32"))
  (set second (dlcall "libc.so.6" "getpid" "i32"))
  (if (and (> first 0) (= first second))
    first
    0))
```
