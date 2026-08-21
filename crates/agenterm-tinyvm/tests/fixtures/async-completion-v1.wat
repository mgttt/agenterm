(module
  (import "fan:async/v1" "start" (func $start (result i32)))
  (import "fan:async/v1" "completion_poll"
    (func $poll (param i32 i32 i32) (result i32)))
  (import "fan:async/v1" "completion_take"
    (func $take (param i32 i32 i32) (result i32)))
  (import "fan:async/v1" "completion_cancel"
    (func $cancel (param i32) (result i32)))
  (import "tinyarcade:core/v1" "submit_render"
    (func $submit_render (param i32 i32) (result i32)))
  (import "tinyarcade:core/v1" "indexed2d_version"
    (func $indexed2d_version (result i32)))

  (memory (export "memory") 1 1)
  (global $ticket (mut i32) (i32.const 0))

  (func (export "game_abi_version") (result i32)
    i32.const 1)

  (func (export "game_init") (result i32)
    call $indexed2d_version
    i32.const 1
    i32.ne
    if
      i32.const 1
      return
    end
    call $start
    global.set $ticket
    i32.const 0)

  (func (export "game_tick") (result i32)
    global.get $ticket
    i32.const 32
    i32.const 36
    call $poll
    i32.const 1
    i32.eq
    if
      i32.const 32
      i32.load
      i32.const 7
      i32.ne
      if
        i32.const 10
        return
      end
      i32.const 36
      i32.load
      i32.const 4
      i32.ne
      if
        i32.const 11
        return
      end
      global.get $ticket
      i32.const 16
      i32.const 4
      call $take
      i32.const 1
      i32.ne
      if
        i32.const 12
        return
      end
    end
    i32.const 0
    i32.const 21
    call $submit_render
    drop
    i32.const 0)

  (func (export "game_suspend") (result i32)
    i32.const 0)

  (func (export "game_resume") (result i32)
    i32.const 0)

  ;; One valid 1×1 tinyarcade:indexed2d/v1 frame. Completion replaces the
  ;; single RGBA palette entry at byte 16; the pixel remains palette index 0.
  (data (i32.const 0)
    "TAI2\01\00\10\00\01\00\01\00\01\00\00\00\00\00\00\00\00"))
