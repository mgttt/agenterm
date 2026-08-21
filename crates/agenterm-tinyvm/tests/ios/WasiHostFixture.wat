(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_close"
    (func $fd_close (param i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit"
    (func $proc_exit (param i32)))
  (memory 1)
  (data (i32.const 0) "\60\00\00\00\05\00\00\00")
  (data (i32.const 64) "slot.bin")
  (data (i32.const 96) "hello")
  (func (export "_start")
    (local $fd i32)
    (if
      (i32.eqz
        (call $path_open
          (i32.const 0) (i32.const 0) (i32.const 64) (i32.const 8)
          (i32.const 9) (i64.const 2097216) (i64.const 0)
          (i32.const 0) (i32.const 32)))
      (then)
      (else unreachable))
    (local.set $fd (i32.load (i32.const 32)))
    (if
      (i32.eqz
        (call $fd_write
          (local.get $fd) (i32.const 0) (i32.const 1) (i32.const 36)))
      (then)
      (else unreachable))
    (if
      (i32.eq (i32.load (i32.const 36)) (i32.const 5))
      (then)
      (else unreachable))
    (if
      (i32.eqz (call $fd_close (local.get $fd)))
      (then)
      (else unreachable))
    (call $proc_exit (i32.const 7))
    unreachable))
