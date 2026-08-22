(module
  (import "tinyarcade:core/v1" "submit_render"
    (func $submit_render (param i32 i32) (result i32)))
  (import "tinyarcade:core/v1" "indexed2d_version"
    (func $indexed2d_version (result i32)))
  (import "tinyarcade:core/v1" "save_state"
    (func $save_state (param i32 i32) (result i32)))
  (import "tinyarcade:core/v1" "load_state"
    (func $load_state (param i32 i32) (result i32)))

  (memory (export "memory") 1 1)

  (func (export "game_abi_version") (result i32) i32.const 1)

  (func $lane_bridge_ok (result i32)
    i32.const -128
    i8x16.splat
    i32.const 510
    i8x16.replace_lane 15
    i8x16.extract_lane_s 0
    i32.const -128
    i32.ne
    if i32.const 0 return end
    i32.const -128
    i8x16.splat
    i32.const 510
    i8x16.replace_lane 15
    i8x16.extract_lane_u 15
    i32.const 254
    i32.ne
    if i32.const 0 return end

    i32.const -32767
    i16x8.splat
    i32.const 33059
    i16x8.replace_lane 6
    i16x8.extract_lane_s 0
    i32.const -32767
    i32.ne
    if i32.const 0 return end
    i32.const -32767
    i16x8.splat
    i32.const 33059
    i16x8.replace_lane 6
    i16x8.extract_lane_u 6
    i32.const 33059
    i32.ne
    if i32.const 0 return end

    i32.const 7
    i32x4.splat
    i32.const 305419896
    i32x4.replace_lane 2
    i32x4.extract_lane 2
    i32.const 305419896
    i32.ne
    if i32.const 0 return end

    i64.const 7
    i64x2.splat
    i64.const 81985529216486895
    i64x2.replace_lane 1
    i64x2.extract_lane 1
    i64.const 81985529216486895
    i64.ne
    if i32.const 0 return end

    f32.const 7.5
    f32x4.splat
    f32.const -13.25
    f32x4.replace_lane 3
    f32x4.extract_lane 3
    f32.const -13.25
    f32.ne
    if i32.const 0 return end

    f64.const 7.5
    f64x2.splat
    f64.const 12345.5
    f64x2.replace_lane 0
    f64x2.extract_lane 0
    f64.const 12345.5
    f64.ne
    if i32.const 0 return end
    i32.const 1)

  (func (export "game_init") (result i32)
    call $indexed2d_version
    i32.const 1
    i32.ne
    if
      i32.const 1
      return
    end

    call $lane_bridge_ok
    i32.eqz
    if
      i32.const 6
      return
    end

    i32.const 64
    v128.const i16x8 30000 -30000 100 -100 32767 -32768 20000 -20000
    v128.const i16x8 10000 -10000 200 -200 1 -1 -25000 25000
    i16x8.add_sat_s
    v128.store

    i32.const 64
    i32.load16_s
    i32.const 32767
    i32.ne
    if
      i32.const 2
      return
    end
    i32.const 66
    i32.load16_s
    i32.const -32768
    i32.ne
    if
      i32.const 3
      return
    end

    i32.const 64
    v128.const i16x8 30000 -30000 100 -100 32767 -32768 20000 -20000
    v128.const i16x8 10000 -10000 200 -200 1 -1 -25000 25000
    i16x8.sub_sat_s
    v128.store

    i32.const 76
    i32.load16_s
    i32.const 32767
    i32.ne
    if
      i32.const 4
      return
    end
    i32.const 78
    i32.load16_s
    i32.const -32768
    i32.ne
    if
      i32.const 5
      return
    end
    i32.const 0)

  (func (export "game_tick") (result i32)
    i32.const 0
    i32.const 21
    call $submit_render
    drop
    i32.const 0)

  (func (export "game_suspend") (result i32)
    i32.const 64
    i32.const 16
    call $save_state)
  (func (export "game_resume") (result i32)
    i32.const 64
    i32.const 16
    call $load_state
    i32.const 16
    i32.ne)

  ;; One valid green 1×1 tinyarcade:indexed2d/v1 frame.
  (data (i32.const 0)
    "TAI2\01\00\10\00\01\00\01\00\01\00\00\00\00\ff\00\ff\00"))
