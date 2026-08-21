(module
  (import "bench" "memory_zero" (func $memory_zero (param i32 i32) (result i32)))
  (import "bench" "selected_memory" (func $selected_memory (param i32 i32) (result i32)))
  (import "bench" "selected_copy" (func $selected_copy (param i32 i32) (result i32)))
  (memory (export "memory") 2)
  (func (export "empty"))
  (func (export "scalars")
    (param i32 i64 f32 f64)
    (result i32)
    local.get 1
    drop
    local.get 2
    drop
    local.get 3
    drop
    local.get 0)
  (func (export "touch")
    (param $pointer i32)
    (param $length i32)
    (result i32)
    local.get $length
    i32.eqz
    if (result i32)
      i32.const 0
    else
      local.get $pointer
      i32.load8_u
      local.get $pointer
      local.get $length
      i32.add
      i32.const 1
      i32.sub
      i32.load8_u
      i32.xor
    end)
  (func (export "host_memory_zero") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    call $memory_zero)
  (func (export "host_selected_memory") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    call $selected_memory)
  (func (export "host_selected_copy") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    call $selected_copy))
