(module
  (func (export "run") (result i32)
    i32.const 1

    i32.const 128
    i32.extend8_s
    i32.const -128
    i32.eq
    i32.and

    i32.const 32768
    i32.extend16_s
    i32.const -32768
    i32.eq
    i32.and

    i64.const 128
    i64.extend8_s
    i64.const -128
    i64.eq
    i32.and

    i64.const 32768
    i64.extend16_s
    i64.const -32768
    i64.eq
    i32.and

    i64.const 2147483648
    i64.extend32_s
    i64.const -2147483648
    i64.eq
    i32.and

    f32.const nan
    i32.trunc_sat_f32_s
    i32.const 0
    i32.eq
    i32.and

    f32.const inf
    i32.trunc_sat_f32_u
    i32.const -1
    i32.eq
    i32.and

    f64.const -inf
    i32.trunc_sat_f64_s
    i32.const -2147483648
    i32.eq
    i32.and

    f64.const -42.75
    i32.trunc_sat_f64_u
    i32.const 0
    i32.eq
    i32.and

    f32.const inf
    i64.trunc_sat_f32_s
    i64.const 9223372036854775807
    i64.eq
    i32.and

    f32.const nan
    i64.trunc_sat_f32_u
    i64.const 0
    i64.eq
    i32.and

    f64.const -42.75
    i64.trunc_sat_f64_s
    i64.const -42
    i64.eq
    i32.and

    f64.const inf
    i64.trunc_sat_f64_u
    i64.const -1
    i64.eq
    i32.and

    i32.const 143
    i32.mul))
