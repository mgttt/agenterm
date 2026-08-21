(module
  (type $binary (func (param i32 i32) (result i32)))
  (type $mixed (func (param i64 f32 f64) (result f64 i64 f32)))
  (type $ref (func (param funcref) (result funcref)))
  (import "provider" "add" (func $add (type $binary)))
  (import "provider" "sub" (func $sub (type $binary)))
  (import "provider" "mixed" (func $mixed (type $mixed)))
  (import "provider" "identity_ref" (func $identity_ref (type $ref)))
  (export "reexport" (func $add))
  (func (export "run") (result i32)
    i32.const 20
    i32.const 22
    call $add)
  (func (export "tail") (result i32)
    i32.const 50
    i32.const 8
    return_call $sub)
  (func (export "typed") (result i32)
    i64.const 42
    f32.const 3.5
    f64.const 4.5
    call $mixed
    drop
    drop
    i32.trunc_f64_s))
