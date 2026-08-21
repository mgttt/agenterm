(module
  (type $result (func (result i32)))
  (memory 1)
  (table 4 funcref)
  (data $data "hello")
  (elem $functions func $forty-two $seven)

  (func $forty-two (type $result) (result i32)
    i32.const 42)
  (func $seven (type $result) (result i32)
    i32.const 7)

  (func (export "run") (result i32)
    i32.const 0
    i32.const 1
    i32.const 3
    memory.init $data
    data.drop $data

    i32.const 1
    i32.const 0
    i32.const 2
    table.init $functions
    elem.drop $functions

    i32.const 0
    i32.const 1
    i32.const 2
    table.copy

    i32.const 0
    i32.load8_u
    i32.const 0
    call_indirect (type $result)
    i32.add))
