use crate::{AmmMathError, checked_mul_div_floor};

pub const FEE_DENOMINATOR: u64 = 10_000;

pub fn amount_after_fee(amount_in: u64, fee_bps: u16) -> Result<u64, AmmMathError> {
    if (fee_bps as u64) >= FEE_DENOMINATOR {
        return Err(AmmMathError::InvalidFee);
    }

    //floor(amount_in * (FEE_DENOMINATOR - fee_bps) / FEE_DENOMINATOR). Use checked_mul_div_floor to do this safely with u128 intermediates and u64 boundaries.
    let multiplier = (FEE_DENOMINATOR as u128) - (fee_bps as u128); // 10_000 - 30 = 9970
    let widened = checked_mul_div_floor(amount_in as u128, multiplier, FEE_DENOMINATOR as u128)?;
    let narrow: u64 = widened.try_into().map_err(|_| AmmMathError::Overflow)?;
    Ok(narrow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn amount_after_fee_thirty_bps() {
        assert_eq!(amount_after_fee(100, 30).unwrap(), 99); // 0.997 × 100 = 99.7 → 99
    }

    #[test]
    fn amount_after_fee_zero_bps() {
        assert_eq!(amount_after_fee(100, 0).unwrap(), 100);
    }

    #[test]
    fn amount_after_fee_one_bps_below_max() {
        assert_eq!(amount_after_fee(100, 9999).unwrap(), 0); // 0.0001 × 100 = 0.01 → 0
    }

    #[test]
    fn amount_after_fee_invalid_fee_equal_to_denominator() {
        assert!(matches!(
            amount_after_fee(100, 10_000),
            Err(AmmMathError::InvalidFee)
        ));
    }

    #[test]
    fn amount_after_fee_invalid_fee_above_denominator() {
        assert!(matches!(
            amount_after_fee(100, 10_001),
            Err(AmmMathError::InvalidFee)
        ));
    }

    #[test]
    fn amount_after_fee_u64_max_does_not_overflow() {
        // 0.997 * u64::MAX should fit in u64; just verify it returns Ok.
        assert!(amount_after_fee(u64::MAX, 30).is_ok());
    }

    proptest! {
        // amount_after_fee(x, bps) ≤ x
        #[test]
        fn fee_never_adds_value(
            x in 0u64..u64::MAX,
            bps in 0u16..10_000,
        ) {
            let r = amount_after_fee(x, bps).unwrap();
            prop_assert!(r <= x);
        }

        // amount_after_fee(x, 0) = x
        #[test]
        fn zero_fee_is_identity(x in 0u64..u64::MAX) {
            prop_assert_eq!(amount_after_fee(x, 0).unwrap(), x);
        }

        // x - amount_after_fee(x, bps) = ⌊x · bps / 10_000⌋
        #[test]
        fn fee_complement_matches_ceil(
            x in 0u64..u64::MAX,
            bps in 0u16..10_000,
        ) {
            let after = amount_after_fee(x, bps).unwrap() as u128;
            let xx = x as u128;
            let expected_fee = (xx * bps as u128).div_ceil(10_000);
            prop_assert_eq!(xx - after, expected_fee);
        }
    }
}
