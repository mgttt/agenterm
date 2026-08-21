(module
  (table (export "dispatch") 2 funcref)
  (memory (export "ram") 1)
  (global (export "counter") (mut i32) (i32.const 7))
  (global (export "fixed") i64 (i64.const 9))
  (data (i32.const 0) "A")

  (func (export "read") (result i32)
    i32.const 0
    i32.load8_u
    global.get 0
    i32.add))
