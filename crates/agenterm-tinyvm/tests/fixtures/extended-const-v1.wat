(module
  (type $ret_i32 (func (result i32)))
  (global $answer i32
    (i32.mul
      (i32.sub
        (i32.add (i32.const 40) (i32.const 5))
        (i32.const 3))
      (i32.const 1)))
  (global $wide i64
    (i64.mul
      (i64.sub
        (i64.add (i64.const 20) (i64.const 1))
        (i64.const 4))
      (i64.const 5)))
  (memory 1)
  (data
    (i32.mul
      (i32.add (i32.const 1) (i32.const 1))
      (i32.sub (i32.const 3) (i32.const 2)))
    "A")
  (table 5 funcref)
  (elem (i32.add (i32.const 1) (i32.const 1)) func $seven)
  (func $seven (type $ret_i32) (result i32)
    i32.const 7)
  (func (export "run") (result i32)
    global.get $answer
    global.get $wide
    i32.wrap_i64
    i32.add
    i32.const 2
    i32.load8_u
    i32.add
    i32.const 2
    call_indirect (type $ret_i32)
    i32.add))
