<!--
CU hand: focus — clock ticks are host timing context near bounded focus
observation. This remains a script probe because dyn has no cu wait model,
deadline policy, target session, or focus command wiring.
-->

# Read clock ticks per second with `sysconf`

Linux defines `_SC_CLK_TCK` as selector `2`. The selector is host-specific
script data; macOS uses a different value and library.

```lisp
(do
  (set selector 2)
  (set ticks
    (dlcall "libc.so.6" "sysconf" "i64" "i32" selector))
  (if (> ticks 0)
    ticks
    -1))
```

The result is the host's positive clock-tick rate, or `-1` when `sysconf`
cannot answer. The script does not turn that fact into cu timing policy.
