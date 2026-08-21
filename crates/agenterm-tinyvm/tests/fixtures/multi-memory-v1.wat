(module
  (memory $first 1 2)
  (memory $second 1 3)
  (export "second" (memory $second))
  (data (memory $first) (i32.const 0) "A")
  (data (memory $second) (i32.const 0) "B")
  (data $passive "C")

  (func (export "run") (result i32)
    (local $sum i32)

    i32.const 1
    i32.const 0
    i32.const 1
    memory.copy $second $first

    i32.const 2
    i32.const 0
    i32.const 1
    memory.init $second $passive
    data.drop $passive

    i32.const 4
    i32.const 1000
    i32.store $second

    i32.const 8
    i32.const 7
    i32.const 2
    memory.fill $second

    i32.const 16
    i64.const 9
    i64.store $second

    i32.const 24
    f32.const 3
    f32.store $second

    i32.const 32
    f64.const 4
    f64.store $second

    i32.const 0
    i32.load8_u $second
    i32.const 1
    i32.load8_u $second
    i32.add
    i32.const 2
    i32.load8_u $second
    i32.add
    i32.const 4
    i32.load $second
    i32.add
    i32.const 8
    i32.load8_u $second
    i32.add
    i32.const 16
    i64.load $second
    i32.wrap_i64
    i32.add
    i32.const 24
    f32.load $second
    i32.trunc_f32_s
    i32.add
    i32.const 32
    f64.load $second
    i32.trunc_f64_s
    i32.add
    local.set $sum

    memory.size $second
    local.get $sum
    i32.add
    local.set $sum

    i32.const 1
    memory.grow $second
    local.get $sum
    i32.add
    memory.size $second
    i32.add))
