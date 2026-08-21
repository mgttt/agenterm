(module
  (type $swap (func (param i32 i64) (result i64 i32)))
  (type $counter (func (param i32) (result i32)))
  (type $pair (func (result i32 i64)))

  (func $multi-result (result i32 i64)
    i32.const 40
    i64.const 2)

  (func (export "run") (result i32)
    (local $i32 i32)
    (local $i64 i64)
    (local $counter i32)
    (local $sum i32)

    call $multi-result
    i32.wrap_i64
    i32.add
    local.set $sum

    i32.const 7
    i64.const 35
    block (type $swap)
      local.set $i64
      local.set $i32
      local.get $i64
      local.get $i32
    end
    local.set $i32
    i32.wrap_i64
    local.get $i32
    i32.add
    local.get $sum
    i32.add
    local.set $sum

    i32.const 8
    i64.const 34
    i32.const 0
    if (type $swap)
      local.set $i64
      local.set $i32
      local.get $i64
      local.get $i32
    else
      local.set $i64
      local.set $i32
      local.get $i64
      local.get $i32
    end
    local.set $i32
    i32.wrap_i64
    local.get $i32
    i32.add
    local.get $sum
    i32.add
    local.set $sum

    i32.const 3
    loop (type $counter)
      local.tee $counter
      i32.eqz
      if (result i32)
        i32.const 1
      else
        local.get $counter
        i32.const 1
        i32.sub
        br 1
      end
    end
    local.get $sum
    i32.add
    local.set $sum

    block (type $pair)
      i32.const 10
      i64.const 6
      i32.const 1
      br_if 0
      unreachable
    end
    i32.wrap_i64
    i32.add
    local.get $sum
    i32.add
    local.set $sum

    block (type $pair)
      i32.const 20
      i64.const 22
      i32.const 0
      br_table 0 0
      unreachable
    end
    i32.wrap_i64
    i32.add
    i32.const 42
    i32.eq
    i32.const 1
    i32.sub
    local.get $sum
    i32.add
    local.set $sum

    i32.const 5
    i32.const 0
    if (type $counter)
    end
    i32.const 5
    i32.eq
    i32.const 1
    i32.sub
    local.get $sum
    i32.add
    local.set $sum

    block (result i32)
      i32.const 5
      block (type $counter)
        drop
        i32.const 9
        i32.const 0
        br_table 0 1
      end
    end
    i32.const 9
    i32.eq
    i32.const 1
    i32.sub
    local.get $sum
    i32.add))
