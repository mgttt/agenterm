(module
  (import "host" "mix"
    (func $mix (param i64 f32 f64) (result f64 i64 f32)))
  (func (export "run") (result f64 i64 f32)
    i64.const 40
    f32.const 1.5
    f64.const 2.5
    call $mix))
