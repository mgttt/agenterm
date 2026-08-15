<!--
CU hand: focus — the descriptor-table limit is process context near terminal
input and focus ownership. This remains a script probe because dyn has no cu
process model, descriptor ownership, target model, or focus command wiring.
-->

# Read the descriptor-table limit

Linux example; the current macOS and Windows host rows keep this probe as a
placeholder.

```lisp
(do
  (set first (dlcall "libc.so.6" "getdtablesize" "i32"))
  (set second (dlcall "libc.so.6" "getdtablesize" "i32"))
  (if (and (> first 0) (= first second))
    first
    -1))
```

The script returns the positive descriptor-table limit when two immediate
reads agree, or `-1` otherwise. It does not assign descriptor or cu focus
authority.
