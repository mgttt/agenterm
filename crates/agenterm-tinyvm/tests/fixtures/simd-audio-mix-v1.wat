(module
  (memory (export "memory") 1)

  ;; Mix eight signed 16-bit PCM samples with standard saturating arithmetic.
  ;; The function keeps v128 internal so an ordinary i32 host ABI can drive it.
  (func (export "mix") (param $left i32) (param $right i32) (param $output i32)
    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i16x8.add_sat_s
    v128.store)

  ;; Remove eight signed 16-bit PCM lanes with the matching saturation rules.
  (func (export "subtract") (param $left i32) (param $right i32) (param $output i32)
    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i16x8.sub_sat_s
    v128.store)

  ;; Exercise the standard whole-vector mask core used by sprite composition,
  ;; packed flags and audio routing. Results occupy six consecutive vectors:
  ;; and, or, xor, and-not, not-left and bitselect(left, right, mask).
  (func (export "logic")
      (param $left i32) (param $right i32) (param $mask i32) (param $output i32)
    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    v128.and
    v128.store

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    v128.or
    v128.store offset=16

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    v128.xor
    v128.store offset=32

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    v128.andnot
    v128.store offset=48

    local.get $output
    local.get $left
    v128.load
    v128.not
    v128.store offset=64

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    local.get $mask
    v128.load
    v128.bitselect
    v128.store offset=80)

  (func (export "any") (param $input i32) (result i32)
    local.get $input
    v128.load
    v128.any_true)

  ;; Wrapping integer lane arithmetic used by packed counters, coordinates,
  ;; fixed-point state and deterministic game simulation. Results are ordered
  ;; i8 add/sub; i16 add/sub/mul; i32 add/sub/mul; i64 add/sub/mul.
  (func (export "lanes") (param $left i32) (param $right i32) (param $output i32)
    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i8x16.add
    v128.store

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i8x16.sub
    v128.store offset=16

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i16x8.add
    v128.store offset=32

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i16x8.sub
    v128.store offset=48

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i16x8.mul
    v128.store offset=64

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i32x4.add
    v128.store offset=80

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i32x4.sub
    v128.store offset=96

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i32x4.mul
    v128.store offset=112

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i64x2.add
    v128.store offset=128

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i64x2.sub
    v128.store offset=144

    local.get $output
    local.get $left
    v128.load
    local.get $right
    v128.load
    i64x2.mul
    v128.store offset=160)

  ;; Scalar/vector lane bridge emitted by portable C/Rust SIMD frontends.
  ;; Six splats, six replacements and all signed/unsigned/numeric extraction
  ;; result families are serialized so independent engines can compare bytes.
  (func (export "bridge") (param $output i32)
    local.get $output
    i32.const -128
    i8x16.splat
    v128.store

    local.get $output
    i32.const 33059
    i16x8.splat
    v128.store offset=16

    local.get $output
    i32.const 305419896
    i32x4.splat
    v128.store offset=32

    local.get $output
    i64.const 81985529216486895
    i64x2.splat
    v128.store offset=48

    local.get $output
    f32.const -13.25
    f32x4.splat
    v128.store offset=64

    local.get $output
    f64.const 12345.5
    f64x2.splat
    v128.store offset=80

    local.get $output
    v128.const i32x4 0 0 0 0
    i32.const 510
    i8x16.replace_lane 15
    v128.store offset=96

    local.get $output
    v128.const i32x4 0 0 0 0
    i32.const 33059
    i16x8.replace_lane 6
    v128.store offset=112

    local.get $output
    v128.const i32x4 0 0 0 0
    i32.const 305419896
    i32x4.replace_lane 2
    v128.store offset=128

    local.get $output
    v128.const i32x4 0 0 0 0
    i64.const 81985529216486895
    i64x2.replace_lane 1
    v128.store offset=144

    local.get $output
    v128.const i32x4 0 0 0 0
    f32.const -13.25
    f32x4.replace_lane 3
    v128.store offset=160

    local.get $output
    v128.const i32x4 0 0 0 0
    f64.const 12345.5
    f64x2.replace_lane 0
    v128.store offset=176

    local.get $output
    i32.const -128
    i8x16.splat
    i8x16.extract_lane_s 7
    i32.store offset=192

    local.get $output
    i32.const -128
    i8x16.splat
    i8x16.extract_lane_u 7
    i32.store offset=196

    local.get $output
    i32.const -32767
    i16x8.splat
    i16x8.extract_lane_s 5
    i32.store offset=200

    local.get $output
    i32.const -32767
    i16x8.splat
    i16x8.extract_lane_u 5
    i32.store offset=204

    local.get $output
    i32.const 305419896
    i32x4.splat
    i32x4.extract_lane 3
    i32.store offset=208

    local.get $output
    i32.const 0
    i32.store offset=212

    local.get $output
    i64.const 81985529216486895
    i64x2.splat
    i64x2.extract_lane 1
    i64.store offset=216

    local.get $output
    f32.const -13.25
    f32x4.splat
    f32x4.extract_lane 2
    f32.store offset=224

    local.get $output
    i32.const 0
    i32.store offset=228

    local.get $output
    f64.const 12345.5
    f64x2.splat
    f64x2.extract_lane 1
    f64.store offset=232)
)
