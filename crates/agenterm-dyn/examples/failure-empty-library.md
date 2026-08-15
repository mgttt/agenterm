<!--
CU hand: get-text — an invalid native provider name must fail before any load,
as an invalid text backend would. This remains a script probe because dyn has
no cu provider registry, error envelope, target context, or get-text wiring.
-->

# Reject an empty library name

```lisp
(do
  (set attempts 0)
  (repeat 1 (set attempts (+ attempts 1)))
  (if (= attempts 1)
    (dlcall "" "unused" "i32")
    0))
```

Evaluation stops with `DynError::Library("library name must not be empty")`
before attempting to load a library. There is no fallback provider or pretend
native result.
