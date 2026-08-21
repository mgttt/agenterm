(module
  (import "host" "ram" (memory 1 3))
  (export "ram" (memory 0))
  (data (i32.const 0) "A")

  (func (export "run") (result i32)
    i32.const 0
    i32.const 0
    i32.load8_u
    i32.const 1
    i32.add
    i32.store8
    memory.size
    i32.const 100
    i32.mul
    i32.const 0
    i32.load8_u
    i32.add)

  (func (export "grow") (result i32)
    i32.const 1
    memory.grow)

  (func (export "size") (result i32)
    memory.size))
