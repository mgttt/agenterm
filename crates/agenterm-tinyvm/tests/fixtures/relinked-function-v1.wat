(module
  (type $binary (func (param i32 i32) (result i32)))
  (import "relay" "function" (func $function (type $binary)))
  (func (export "run") (result i32)
    i32.const 40
    i32.const 2
    call $function))
