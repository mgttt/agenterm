(module
  (type $nullary (func (result i32)))
  (func (export "add") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.add)
  (func (export "sub") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.sub)
  (func (export "unary") (param i32) (result i32)
    local.get 0)
  (func (export "mixed") (param i64 f32 f64) (result f64 i64 f32)
    local.get 2
    local.get 0
    local.get 1)
  (func (export "identity_ref") (param funcref) (result funcref)
    local.get 0)
  (func $answer (type $nullary)
    i32.const 43)
  (global (export "answer_ref") funcref (ref.func $answer))
  (elem declare func $answer))
