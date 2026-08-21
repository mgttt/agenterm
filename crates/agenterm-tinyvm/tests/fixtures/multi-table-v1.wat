(module
  (type $result (func (result i32)))
  (table $first 1 3 funcref)
  (table $second 2 4 funcref)
  (export "second" (table $second))
  (elem (table $first) (i32.const 0) func $forty-two)
  (elem (table $second) (i32.const 1) func $seven)
  (elem $refs funcref (ref.func $forty-two))

  (func $forty-two (type $result) (result i32)
    i32.const 42)

  (func $seven (type $result) (result i32)
    i32.const 7)

  (func (export "run") (result i32)
    (local $sum i32)

    i32.const 0
    call_indirect $first (type $result)
    local.set $sum

    i32.const 1
    call_indirect $second (type $result)
    local.get $sum
    i32.add
    local.set $sum

    i32.const 0
    i32.const 0
    table.get $first
    table.set $second
    i32.const 0
    call_indirect $second (type $result)
    local.get $sum
    i32.add
    local.set $sum

    i32.const 0
    i32.const 1
    i32.const 1
    table.copy $first $second
    i32.const 0
    call_indirect $first (type $result)
    local.get $sum
    i32.add
    local.set $sum

    i32.const 0
    i32.const 0
    i32.const 1
    table.init $second $refs
    elem.drop $refs
    i32.const 0
    call_indirect $second (type $result)
    local.get $sum
    i32.add
    local.set $sum

    i32.const 0
    ref.null func
    i32.const 0
    table.fill $second

    ref.null func
    i32.const 1
    table.grow $first
    local.get $sum
    i32.add
    table.size $first
    i32.add))
