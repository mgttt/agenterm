(module
  (type $answer-type (func (result i32)))
  (import "host" "dispatch" (table 1 3 funcref))
  (func (export "run") (result i32)
    i32.const 0
    call_indirect (type $answer-type)))
