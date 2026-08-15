<!--
CU hand: get-text — a malformed native provider name must fail before any
load, as a malformed text-backend identifier would. This remains a script
probe because dyn has no cu provider registry, error envelope, or get-text wiring.
-->

# Reject a library name containing NUL

The current language has no string escape syntax. In the readable form below,
`␀` visibly marks the position occupied by one actual U+0000 character in the
source buffer passed to `Dyn::eval`; the glyph itself is not language syntax.

```lisp
(do
  (set attempts 0)
  (repeat 1 (set attempts (+ attempts 1)))
  (if (= attempts 1)
    (dlcall "bad␀library" "unused" "i32")
    0))
```

With the marked position encoded as actual NUL, evaluation stops with
`DynError::Library("library name contains interior NUL")` before attempting a
load. Writing `\0` instead is intentionally not supported by the parser.
