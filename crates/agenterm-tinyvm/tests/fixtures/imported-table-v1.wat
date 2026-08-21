(module
  (type $answer-type (func (result i32)))
  (import "host" "dispatch" (table 1 3 funcref))
  (table 1 funcref)
  (global $counter (mut i32) (i32.const 0))
  (func $answer (type $answer-type) (result i32)
    global.get $counter
    i32.const 1
    i32.add
    global.set $counter
    global.get $counter)
  (elem (table 0) (i32.const 0) func $answer)
  (export "dispatch" (table 0))
  (export "local" (table 1))
  (func (export "run") (result i32)
    i32.const 0
    call_indirect (type $answer-type)))
