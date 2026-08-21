(module
  (type $bounce-type (func (param i32) (result i32)))
  (import "host" "dispatch" (table 2 2 funcref))
  (import "host" "slot" (global i32))
  (func $bounce (type $bounce-type) (param $remaining i32) (result i32)
    local.get $remaining
    i32.eqz
    if (result i32)
      i32.const 0
    else
      local.get $remaining
      i32.const 1
      i32.sub
      i32.const 1
      global.get 0
      i32.sub
      call_indirect (type $bounce-type)
      i32.const 1
      i32.add
    end)
  (elem (global.get 0) func $bounce)
  (export "run" (func $bounce)))
