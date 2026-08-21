(module
  (type $ret_i32 (func (result i32)))
  (import "host" "base" (global $base i32))
  (import "host" "counter" (global $counter (mut i32)))
  (global $answer i32
    (i32.add (global.get $base) (i32.const 2)))
  (memory 1)
  (data (global.get $base) "A")
  (table 8 funcref)
  (elem (global.get $base) func $seven)
  (func $seven (type $ret_i32) (result i32)
    i32.const 7)
  (func (export "run") (result i32)
    global.get $counter
    global.get $counter
    i32.const 1
    i32.add
    global.set $counter
    global.get $answer
    i32.add
    global.get $base
    i32.load8_u
    i32.add
    global.get $base
    call_indirect (type $ret_i32)
    i32.add))
