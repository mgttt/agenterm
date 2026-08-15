<!--
CU hand: focus — process alarm state is host timing context near bounded focus
observation. This remains a script probe because dyn has no cu wait model,
deadline policy, target session, or focus command wiring.
-->

# Cancel and observe the process alarm

Linux uses `libc.so.6`; macOS uses `libSystem.B.dylib` with the same
`alarm(u32) -> u32` signature. Passing `0` cancels any pending process alarm.

```lisp
(do
  (set disable 0)
  (set previous
    (dlcall "libc.so.6" "alarm" "u32" "u32" disable))
  (set remaining
    (dlcall "libc.so.6" "alarm" "u32" "u32" disable))
  (if (= remaining 0)
    previous
    -1))
```

The result is the whole seconds that remained before cancellation; normally it
is `0` when no alarm was pending. The second call confirms none remains. This
process-global mutation is script behavior, not cu deadline policy, and needs
no compiled C shim or libffi bridge.
