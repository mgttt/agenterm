(module
  (import "host" "source" (func $source (result externref)))
  (import "host" "sink" (func $sink (param externref) (result i32)))
  (global $saved (export "saved") (mut externref) (ref.null extern))

  (func (export "roundtrip") (result i32)
    call $source
    global.set $saved
    global.get $saved
    call $sink)

  (func (export "null_is_null") (result i32)
    ref.null extern
    ref.is_null)

  (func (export "host_is_not_null") (result i32)
    call $source
    ref.is_null
    i32.eqz)

  (func (export "read_saved") (result i32)
    global.get $saved
    call $sink))
