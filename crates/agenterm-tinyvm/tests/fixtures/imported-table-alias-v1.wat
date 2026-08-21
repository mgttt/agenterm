(module
  (type $result (func (result i32)))
  (import "host" "a" (table $a 6 6 funcref))
  (import "host" "b" (table $b 6 6 funcref))
  (func $one (type $result) (result i32) i32.const 1)
  (func $two (type $result) (result i32) i32.const 2)
  (func $three (type $result) (result i32) i32.const 3)
  (func $four (type $result) (result i32) i32.const 4)
  (func $five (type $result) (result i32) i32.const 5)
  (func $six (type $result) (result i32) i32.const 6)
  (elem (table $a) (i32.const 0) func $one $two $three $four $five $six)
  (func (export "overlap") (result i32)
    (local $sum i32)
    i32.const 1
    i32.const 0
    i32.const 5
    table.copy $b $a
    i32.const 0
    call_indirect $a (type $result)
    local.set $sum
    local.get $sum
    i32.const 1
    call_indirect $a (type $result)
    i32.add
    local.set $sum
    local.get $sum
    i32.const 2
    call_indirect $a (type $result)
    i32.add
    local.set $sum
    local.get $sum
    i32.const 3
    call_indirect $a (type $result)
    i32.add
    local.set $sum
    local.get $sum
    i32.const 4
    call_indirect $a (type $result)
    i32.add
    local.set $sum
    local.get $sum
    i32.const 5
    call_indirect $a (type $result)
    i32.add))
