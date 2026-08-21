(module
  (import "host" "a" (memory $a 1 3))
  (import "host" "b" (memory $b 1 3))
  (data (memory $a) (i32.const 0) "abcdef")

  (func (export "overlap") (result i32)
    i32.const 1
    i32.const 0
    i32.const 4
    memory.copy $b $a

    i32.const 0
    i32.load8_u $a
    i32.const 1
    i32.load8_u $a
    i32.add
    i32.const 2
    i32.load8_u $a
    i32.add
    i32.const 3
    i32.load8_u $a
    i32.add
    i32.const 4
    i32.load8_u $a
    i32.add
    i32.const 5
    i32.load8_u $a
    i32.add))
