(module
  (type $answer-type (func (result i32)))
  (table (export "dispatch") 1 3 funcref)
  (func $answer (type $answer-type) (result i32)
    i32.const 42)
  (elem (i32.const 0) func $answer))
