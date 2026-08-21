(module
  (type $unary (func (param i32) (result i32)))
  (table 1 funcref)
  (elem (i32.const 0) func $add-forty-three)

  (func $count-down (type $unary) (param $remaining i32) (result i32)
    local.get $remaining
    i32.eqz
    if (result i32)
      i32.const 100
    else
      local.get $remaining
      i32.const 1
      i32.sub
      return_call $count-down
    end)

  (func $add-forty-three (type $unary) (param $value i32) (result i32)
    local.get $value
    i32.const 43
    i32.add)

  (func $indirect (type $unary) (param $value i32) (result i32)
    local.get $value
    i32.const 0
    return_call_indirect (type $unary))

  (func (export "run") (result i32)
    i32.const 100000
    call $count-down
    i32.const 0
    call $indirect
    i32.add))
