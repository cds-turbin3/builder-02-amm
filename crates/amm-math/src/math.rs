//! Low-level integer-math helpers.

use crate::AmmMathError;

/// Narrow a `u128` to `u64`, returning `Err(Overflow)` if it doesn't fit.
pub fn narrow_u64(n: u128) -> Result<u64, AmmMathError> {
    n.try_into().map_err(|_| AmmMathError::Overflow)
}

/// Ceiling division: `ceil(numerator / denominator)`.
///
/// Returns `Err(ZeroDiv)` on zero denominator, `Err(Overflow)`
/// if `numerator + (denominator - 1)` would overflow `u128`.
pub fn div_ceil(numerator: u128, denominator: u128) -> Result<u128, AmmMathError> {
    if denominator == 0 {
        return Err(AmmMathError::ZeroDiv);
    }

    Ok(numerator
        .checked_add(denominator - 1)
        .ok_or(AmmMathError::Overflow)?
        / denominator)
}

/// `floor((a * b) / denominator)` with overflow-checked intermediate.
///
/// The `u128` intermediate prevents overflow during multiplication of two near-`u64` operands.
pub fn checked_mul_div_floor(a: u128, b: u128, denominator: u128) -> Result<u128, AmmMathError> {
    if denominator == 0 {
        return Err(AmmMathError::ZeroDiv);
    }

    Ok(a.checked_mul(b).ok_or(AmmMathError::Overflow)? / denominator)
}

/// `ceil((a * b) / denominator)` with overflow-checked intermediate.
///
/// Composes [`checked_mul_div_floor`]'s overflow guard with [`div_ceil`]'s rounding.
pub fn checked_mul_div_ceil(a: u128, b: u128, denominator: u128) -> Result<u128, AmmMathError> {
    let product = a.checked_mul(b).ok_or(AmmMathError::Overflow)?;
    div_ceil(product, denominator)
}

/// `floor(sqrt(value))` over `u128` using Newton's method.
///
/// The initial estimate `2^ceil(bits / 2)` is the smallest power of 2 at least
/// as large as `sqrt(value)`, which bounds `x + value/x` away from overflow at
/// the `value = u128::MAX` boundary.
pub fn integer_sqrt_floor(value: u128) -> u128 {
    if value == 0 {
        return 0;
    }

    let bits = 128 - value.leading_zeros();
    let mut x = 1u128 << bits.div_ceil(2);

    let mut next = (x + value / x) / 2;
    while next < x {
        x = next;
        next = (x + value / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn narrow_u64_within_range() {
        assert_eq!(narrow_u64(42).unwrap(), 42u64);
        assert_eq!(narrow_u64(u64::MAX as u128).unwrap(), u64::MAX);
    }

    #[test]
    fn narrow_u64_overflow() {
        assert!(matches!(
            narrow_u64(u64::MAX as u128 + 1),
            Err(AmmMathError::Overflow)
        ));
    }

    // ⌈7/3⌉ = 3
    #[test]
    fn div_ceil_rounds_up() {
        assert_eq!(div_ceil(7, 3).unwrap(), 3);
    }

    // ⌈6/3⌉ = 2
    #[test]
    fn div_ceil_exact_division() {
        assert_eq!(div_ceil(6, 3).unwrap(), 2);
    }

    // d = 0  →  Err(ZeroDiv)
    #[test]
    fn div_ceil_zero_denominator_errors() {
        assert!(matches!(div_ceil(4, 0), Err(AmmMathError::ZeroDiv)));
    }

    // n + (d - 1) overflows u128  →  Err(Overflow)
    #[test]
    fn div_ceil_overflow() {
        assert!(matches!(
            div_ceil(u128::MAX, 2),
            Err(AmmMathError::Overflow)
        ));
    }

    // ⌊(a · b) / d⌋  for several (a, b, d); plus d=0 and a·b overflow paths
    #[test]
    fn mul_div_floor() {
        assert_eq!(checked_mul_div_floor(99, 1000, 1099).unwrap(), 90);
        assert_eq!(checked_mul_div_floor(6, 4, 2).unwrap(), 12);
        assert_eq!(checked_mul_div_floor(7, 3, 4).unwrap(), 5);
        assert_eq!(checked_mul_div_floor(0, 42, 5).unwrap(), 0);
        assert!(matches!(
            checked_mul_div_floor(69, 5, 0),
            Err(AmmMathError::ZeroDiv)
        ));
        assert!(matches!(
            checked_mul_div_floor(u128::MAX, 2, 1),
            Err(AmmMathError::Overflow)
        ))
    }

    // ⌈(a · b) / d⌉  for several (a, b, d); plus d=0 and a·b overflow paths
    #[test]
    fn mul_div_ceil() {
        assert_eq!(checked_mul_div_ceil(99, 1000, 1099).unwrap(), 91);
        assert_eq!(checked_mul_div_ceil(6, 4, 2).unwrap(), 12);
        assert_eq!(checked_mul_div_ceil(7, 3, 4).unwrap(), 6);
        assert_eq!(checked_mul_div_ceil(0, 42, 5).unwrap(), 0);
        assert!(matches!(
            checked_mul_div_ceil(1, 9, 0),
            Err(AmmMathError::ZeroDiv)
        ));
        assert!(matches!(
            checked_mul_div_ceil(u128::MAX, 2, 1),
            Err(AmmMathError::Overflow)
        ));
    }

    // ⌊√v⌋  for small v and the boundary v = u128::MAX (expecting 2⁶⁴ - 1)
    #[test]
    fn test_integer_sqrt_floor() {
        assert_eq!(integer_sqrt_floor(0), 0);
        assert_eq!(integer_sqrt_floor(1), 1);
        assert_eq!(integer_sqrt_floor(4), 2);
        assert_eq!(integer_sqrt_floor(5), 2);
        assert_eq!(integer_sqrt_floor(9), 3);
        assert_eq!(integer_sqrt_floor(10), 3);
        assert_eq!(integer_sqrt_floor(15), 3);
        assert_eq!(integer_sqrt_floor(16), 4);
        assert_eq!(integer_sqrt_floor(u128::MAX), ((1u128 << 64) - 1));
    }

    proptest! {
        // r · d ≥ n   where  r = ⌈n/d⌉
        #[test]
        fn div_ceil_no_undershoot(
            n in 0u128..u128::MAX / 2,
            d in 1u128..u128::MAX / 2
        ) {
            let result = div_ceil(n,d).unwrap();
            prop_assert!(result.checked_mul(d).unwrap() >= n);
        }

        // (r - 1) · d < n   when  r > 0
        #[test]
        fn div_ceil_no_overshoot(
            n in 0u128..u128::MAX / 2,
            d in 1u128..u128::MAX / 2,
        ) {
            let result = div_ceil(n,d).unwrap();
            if result > 0 {
                prop_assert!((result-1).checked_mul(d).unwrap() < n);
            }
        }

        // ⌈(k · d) / d⌉ = k
        #[test]
        fn div_ceil_exact_when_divisible(
            k in 0u128..1_000_000,
            d in 1u128..1_000_000,
        ) {
            let n = k * d;
            prop_assert_eq!(div_ceil(n, d).unwrap(), k);
        }

        // r · d ≤ a · b   where  r = ⌊(a · b) / d⌋
        #[test]
        fn mul_div_floor_lowerbound(
            a in 0u128..(1u128 << 60),
            b in 0u128..(1u128 << 60),
            d in 1u128..u128::MAX,
        ) {
            let r = checked_mul_div_floor(a, b, d).unwrap();
            prop_assert!(r * d <= a * b);
        }

        // (r + 1) · d > a · b
        #[test]
        fn mul_div_floor_upperbound(
            a in 0u128..(1u128 << 60),
            b in 0u128..(1u128 << 60),
            d in 1u128..u128::MAX,
        ) {
            let r = checked_mul_div_floor(a, b, d).unwrap();
            prop_assert!((r+1) * d > a * b);
        }

        // ⌊(a · (d · m)) / d⌋ = a · m
        #[test]
        fn mul_div_floor_exactness(
            a in 0u128..(1u128 << 40),
            d in 0u128..(1u128 << 40),
            multiplier in 1u128..(1u128 << 40),
        ) {
            let b = d * multiplier;
            let expected = a * multiplier;
            prop_assert_eq!(checked_mul_div_floor(a, b, d).unwrap(), expected);
        }

        // r · d ≥ a · b   where  r = ⌈(a · b) / d⌉
        #[test]
        fn mul_div_ceil_upperbound(
            a in 0u128..(1u128 << 60),
            b in 0u128..(1u128 << 60),
            d in 1u128..(1u128 << 60),
        ) {
            let r = checked_mul_div_ceil(a, b, d).unwrap();
            prop_assert!(r * d >= a * b);
        }

        // (r - 1) · d < a · b   when  r > 0
        #[test]
        fn mul_div_ceil_lowerbound(
            a in 0u128..(1u128 << 60),
            b in 0u128..(1u128 << 60),
            d in 1u128..(1u128 << 60),
        ) {
            let r = checked_mul_div_ceil(a, b, d).unwrap();
            if r > 0 {
                prop_assert!((r - 1) * d < a * b);
            }
        }

        // r² ≤ value   where  r = ⌊√value⌋
        #[test]
        fn integer_sqrt_floor_lowerbound(value in 0u128..(1u128 << 126)) {
            let r = integer_sqrt_floor(value);
            prop_assert!(r * r <= value);
        }

        // (r + 1)² > value
        #[test]
        fn integer_sqrt_floor_upperbound(value in 0u128..(1u128 << 126)) {
            let r = integer_sqrt_floor(value);
            prop_assert!((r + 1) * (r + 1) > value);
        }
    }
}
