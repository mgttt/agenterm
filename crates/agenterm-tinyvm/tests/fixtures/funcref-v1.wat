(module
  (type $result (func (result i32)))
  (table 1 5 funcref)
  (elem $refs funcref (ref.null func) (ref.func $forty-two))

  (func $forty-two (type $result) (result i32)
    i32.const 42)

  (func (export "run") (result i32)
    (local $sum i32)

    ref.null func
    ref.is_null
    local.set $sum

    i32.const 0
    ref.func $forty-two
    ref.null func
    i32.const 1
    select (result funcref)
    table.set

    i32.const 0
    call_indirect (type $result)
    local.get $sum
    i32.add
    local.set $sum

    ref.null func
    i32.const 2
    table.grow
    local.get $sum
    i32.add
    local.set $sum

    table.size
    i32.const 3
    i32.eq
    local.get $sum
    i32.add
    local.set $sum

    i32.const 1
    ref.func $forty-two
    i32.const 2
    table.fill

    i32.const 2
    table.get
    ref.is_null
    i32.eqz
    local.get $sum
    i32.add
    local.set $sum

    i32.const 0
    i32.const 1
    i32.const 1
    table.init $refs
    elem.drop $refs

    i32.const 0
    call_indirect (type $result)
    local.get $sum
    i32.add
    i32.const 55
    i32.add))
