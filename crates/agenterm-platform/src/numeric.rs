//! Small deterministic floating-point leaves used at native geometry boundaries.
//!
//! These preserve the IEEE-754 behavior of the corresponding Rust operations
//! without requiring a platform C runtime math import.

const F32_SIGN: u32 = 1 << 31;
const F32_FRACTION_BITS: i32 = 23;
const F32_EXPONENT_BIAS: i32 = 127;
const F64_SIGN: u64 = 1 << 63;
const F64_FRACTION_BITS: i32 = 52;
const F64_EXPONENT_BIAS: i32 = 1023;

#[inline(never)]
pub fn trunc_f32(value: f32) -> f32 {
    let bits = value.to_bits();
    let exponent = ((bits >> F32_FRACTION_BITS) & 0xff) as i32 - F32_EXPONENT_BIAS;
    if exponent < 0 {
        return f32::from_bits(bits & F32_SIGN);
    }
    if exponent >= F32_FRACTION_BITS {
        return value;
    }
    let fractional_mask = (1u32 << (F32_FRACTION_BITS - exponent)) - 1;
    f32::from_bits(bits & !fractional_mask)
}

#[inline(never)]
pub fn round_f32(value: f32) -> f32 {
    let bits = value.to_bits();
    let exponent_bits = (bits >> F32_FRACTION_BITS) & 0xff;
    if exponent_bits == 0xff {
        return value;
    }
    let exponent = exponent_bits as i32 - F32_EXPONENT_BIAS;
    if exponent < -1 {
        return f32::from_bits(bits & F32_SIGN);
    }
    if exponent == -1 {
        return f32::from_bits((bits & F32_SIGN) | 1.0f32.to_bits());
    }
    if exponent >= F32_FRACTION_BITS {
        return value;
    }
    let fractional_bits = F32_FRACTION_BITS - exponent;
    let fractional_mask = (1u32 << fractional_bits) - 1;
    if bits & fractional_mask == 0 {
        return value;
    }
    let half = 1u32 << (fractional_bits - 1);
    let rounded_magnitude = ((bits & !F32_SIGN) + half) & !fractional_mask;
    f32::from_bits((bits & F32_SIGN) | rounded_magnitude)
}

#[inline(never)]
pub fn ceil_f32(value: f32) -> f32 {
    let bits = value.to_bits();
    let exponent_bits = (bits >> F32_FRACTION_BITS) & 0xff;
    if exponent_bits == 0xff {
        return value;
    }
    let exponent = exponent_bits as i32 - F32_EXPONENT_BIAS;
    if exponent < 0 {
        if bits & !F32_SIGN == 0 {
            return value;
        }
        return if bits & F32_SIGN == 0 {
            1.0
        } else {
            f32::from_bits(F32_SIGN)
        };
    }
    if exponent >= F32_FRACTION_BITS {
        return value;
    }
    let fractional_bits = F32_FRACTION_BITS - exponent;
    let fractional_mask = (1u32 << fractional_bits) - 1;
    if bits & fractional_mask == 0 {
        return value;
    }
    let truncated = bits & !fractional_mask;
    if bits & F32_SIGN == 0 {
        f32::from_bits(truncated + (1u32 << fractional_bits))
    } else {
        f32::from_bits(truncated)
    }
}

#[inline(never)]
pub fn round_f64(value: f64) -> f64 {
    let bits = value.to_bits();
    let exponent_bits = (bits >> F64_FRACTION_BITS) & 0x7ff;
    if exponent_bits == 0x7ff {
        return value;
    }
    let exponent = exponent_bits as i32 - F64_EXPONENT_BIAS;
    if exponent < -1 {
        return f64::from_bits(bits & F64_SIGN);
    }
    if exponent == -1 {
        return f64::from_bits((bits & F64_SIGN) | 1.0f64.to_bits());
    }
    if exponent >= F64_FRACTION_BITS {
        return value;
    }
    let fractional_bits = F64_FRACTION_BITS - exponent;
    let fractional_mask = (1u64 << fractional_bits) - 1;
    if bits & fractional_mask == 0 {
        return value;
    }
    let half = 1u64 << (fractional_bits - 1);
    let rounded_magnitude = ((bits & !F64_SIGN) + half) & !fractional_mask;
    f64::from_bits((bits & F64_SIGN) | rounded_magnitude)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_leaves_match_standard_ieee_operations() {
        let f32_cases = [
            f32::NEG_INFINITY,
            -8_388_609.0,
            -2.5,
            -1.5,
            -0.5,
            -0.499_999_97,
            -0.0,
            0.0,
            0.499_999_97,
            0.5,
            1.5,
            2.5,
            8_388_609.0,
            f32::INFINITY,
        ];
        for value in f32_cases {
            assert_eq!(round_f32(value).to_bits(), value.round().to_bits());
            assert_eq!(ceil_f32(value).to_bits(), value.ceil().to_bits());
            assert_eq!(trunc_f32(value).to_bits(), value.trunc().to_bits());
        }
        for bits in (0..=u32::MAX).step_by(1_048_573) {
            let value = f32::from_bits(bits);
            if !value.is_nan() {
                assert_eq!(round_f32(value).to_bits(), value.round().to_bits());
                assert_eq!(ceil_f32(value).to_bits(), value.ceil().to_bits());
                assert_eq!(trunc_f32(value).to_bits(), value.trunc().to_bits());
            }
        }

        let f64_cases = [
            f64::NEG_INFINITY,
            -4_503_599_627_370_497.0,
            -2.5,
            -1.5,
            -0.5,
            -0.499_999_999_999_999_94,
            -0.0,
            0.0,
            0.499_999_999_999_999_94,
            0.5,
            1.5,
            2.5,
            4_503_599_627_370_497.0,
            f64::INFINITY,
        ];
        for value in f64_cases {
            assert_eq!(round_f64(value).to_bits(), value.round().to_bits());
        }
    }
}
