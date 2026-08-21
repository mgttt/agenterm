(module
  (import "host" "refs" (table $shared 2 6 externref))
  (table $local (export "local") 3 6 externref)
  (elem $nulls externref (ref.null extern) (ref.null extern))

  (func (export "seed") (param $value externref)
    i32.const 0
    local.get $value
    table.set $local
    i32.const 1
    local.get $value
    table.set $shared)

  (func (export "get_local") (result externref)
    i32.const 0
    table.get $local)

  (func (export "copy_local_to_shared")
    i32.const 0
    i32.const 0
    i32.const 1
    table.copy $shared $local)

  (func (export "grow_local") (param $value externref) (param $delta i32) (result i32)
    local.get $value
    local.get $delta
    table.grow $local)

  (func (export "fill_local") (param $value externref)
    i32.const 1
    local.get $value
    i32.const 2
    table.fill $local)

  (func (export "init_nulls")
    i32.const 1
    i32.const 0
    i32.const 2
    table.init $local $nulls
    elem.drop $nulls))
