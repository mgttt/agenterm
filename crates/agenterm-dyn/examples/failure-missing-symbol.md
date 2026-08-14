<!--
CU hand: get-text — an unavailable native entry point must fail explicitly,
as an unavailable accessibility text backend would. This remains a script
probe because dyn reports dlcall errors but has no cu error envelope, target
context, accessibility backend, or get-text command wiring.
-->

# Observe a missing-symbol failure

```lisp
(do
  (set attempts 0)
  (repeat 1 (set attempts (+ attempts 1)))
  (if (or (< attempts 1) (> attempts 1))
    0
    (dlcall "libc.so.6" "agenterm_dyn_missing_get_text" "ptr")))
```

Evaluation stops with a `DynError::Symbol` naming the missing symbol. There is
no fallback value and no pretend get-text result.
