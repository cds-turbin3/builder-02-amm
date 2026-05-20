use crate::{
    AmmMathError, ExactOutputQuote, SwapQuote, checked_mul_div_ceil, checked_mul_div_floor,
    narrow_u64,
};

pub const FEE_DENOMINATOR: u64 = 10_000;

pub fn amount_after_fee(amount_in: u64, fee_bps: u16) -> Result<u64, AmmMathError> {
    if (fee_bps as u64) >= FEE_DENOMINATOR {
        return Err(AmmMathError::InvalidFee);
    }

    //floor(amount_in * (FEE_DENOMINATOR - fee_bps) / FEE_DENOMINATOR). Use checked_mul_div_floor to do this safely with u128 intermediates and u64 boundaries.
    let multiplier = (FEE_DENOMINATOR as u128) - (fee_bps as u128); // 10_000 - 30 = 9970
    let widened = checked_mul_div_floor(amount_in as u128, multiplier, FEE_DENOMINATOR as u128)?;
    let narrow = narrow_u64(widened).unwrap();
    Ok(narrow)
}

pub fn swap_exact_input(
    reserve_in: u64,
    reserve_out: u64,
    amount_in: u64,
    fee_bps: u16,
) -> Result<SwapQuote, AmmMathError> {
    if amount_in == 0 {
        return Err(AmmMathError::ZeroAmount);
    } else if reserve_in == 0 || reserve_out == 0 {
        return Err(AmmMathError::ZeroReserve);
    };

    let amount_in_after_fee = amount_after_fee(amount_in, fee_bps)?;
    let fee_amount = amount_in - amount_in_after_fee;
    let denom = (reserve_in as u128) + (amount_in_after_fee as u128);
    let amount_out_u128 =
        checked_mul_div_floor(amount_in_after_fee as u128, reserve_out as u128, denom)?;
    let amount_out = narrow_u64(amount_out_u128)?;

    if amount_out == 0 {
        return Err(AmmMathError::InsufficientOutput);
    }

    let new_reserve_in = (reserve_in as u128)
        .checked_add(amount_in as u128)
        .and_then(|v| u64::try_from(v).ok())
        .ok_or(AmmMathError::Overflow)?;

    let new_reserve_out = reserve_out
        .checked_sub(amount_out)
        .ok_or(AmmMathError::Underflow)?;

    Ok(SwapQuote {
        amount_in,
        amount_in_after_fee,
        fee_amount,
        amount_out,
        new_reserve_in,
        new_reserve_out,
    })
}

pub fn swap_exact_output(
    reserve_in: u64,
    reserve_out: u64,
    amount_out: u64,
    fee_bps: u16,
) -> Result<ExactOutputQuote, AmmMathError> {
    if amount_out == 0 {
        return Err(AmmMathError::ZeroAmount);
    }
    if reserve_in == 0 || reserve_out == 0 {
        return Err(AmmMathError::ZeroReserve);
    }
    if amount_out >= reserve_out {
        return Err(AmmMathError::InsufficientLiquidity);
    }
    if (fee_bps as u64) >= FEE_DENOMINATOR {
        return Err(AmmMathError::InvalidFee);
    }

    // ⌈amount_out · reserve_in / (reserve_out - amount_out)⌉
    let amount_in_after_fee_u128 = checked_mul_div_ceil(
        amount_out as u128,
        reserve_in as u128,
        (reserve_out - amount_out) as u128,
    )?;
    let amount_in_after_fee = narrow_u64(amount_in_after_fee_u128)?;

    // ⌈amount_in_after_fee · FEE_DENOMINATOR / (FEE_DENOMINATOR - fee_bps)⌉
    let amount_in_u128 = checked_mul_div_ceil(
        amount_in_after_fee as u128,
        FEE_DENOMINATOR as u128,
        (FEE_DENOMINATOR as u128) - (fee_bps as u128),
    )?;
    let amount_in = narrow_u64(amount_in_u128)?;

    let fee_amount = amount_in - amount_in_after_fee;

    let new_reserve_in_u128 = (reserve_in as u128)
        .checked_add(amount_in as u128)
        .ok_or(AmmMathError::Overflow)?;
    let new_reserve_in = narrow_u64(new_reserve_in_u128)?;

    let new_reserve_out = reserve_out
        .checked_sub(amount_out)
        .ok_or(AmmMathError::Underflow)?;

    Ok(ExactOutputQuote {
        amount_out,
        amount_in_after_fee,
        amount_in,
        fee_amount,
        new_reserve_in,
        new_reserve_out,
    })
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

    #[test]
    fn swap_exact_input_invalid_fee_propagates() {
        assert!(matches!(
            swap_exact_input(1000, 1000, 100, 10_000),
            Err(AmmMathError::InvalidFee)
        ));
    }

    // ⌈90·1000/(1000-90)⌉ = 99;  ⌈99·10000/9970⌉ = 100;  fee = 1
    #[test]
    fn swap_exact_output_thirty_bps() {
        let q = swap_exact_output(1000, 1000, 90, 30).unwrap();
        assert_eq!(q.amount_in_after_fee, 99);
        assert_eq!(q.amount_in, 100);
        assert_eq!(q.fee_amount, 1);
        assert_eq!(q.new_reserve_in, 1100);
        assert_eq!(q.new_reserve_out, 910);
    }

    // amount_out = 0  →  Err(ZeroAmount)
    #[test]
    fn swap_exact_output_zero_amount_errors() {
        assert!(matches!(
            swap_exact_output(1000, 1000, 0, 30),
            Err(AmmMathError::ZeroAmount)
        ));
    }

    // reserve_in = 0  →  Err(ZeroReserve)
    #[test]
    fn swap_exact_output_zero_reserve_in_errors() {
        assert!(matches!(
            swap_exact_output(0, 1000, 90, 30),
            Err(AmmMathError::ZeroReserve)
        ));
    }

    // reserve_out = 0  →  Err(ZeroReserve)
    #[test]
    fn swap_exact_output_zero_reserve_out_errors() {
        assert!(matches!(
            swap_exact_output(1000, 0, 90, 30),
            Err(AmmMathError::ZeroReserve)
        ));
    }

    // amount_out = reserve_out  →  Err(InsufficientLiquidity)  (drains pool)
    #[test]
    fn swap_exact_output_drain_errors() {
        assert!(matches!(
            swap_exact_output(1000, 1000, 1000, 30),
            Err(AmmMathError::InsufficientLiquidity)
        ));
    }

    // amount_out > reserve_out  →  Err(InsufficientLiquidity)
    #[test]
    fn swap_exact_output_exceeds_reserve_errors() {
        assert!(matches!(
            swap_exact_output(1000, 1000, 1001, 30),
            Err(AmmMathError::InsufficientLiquidity)
        ));
    }

    // fee_bps = FEE_DENOMINATOR  →  Err(InvalidFee)
    #[test]
    fn swap_exact_output_invalid_fee_errors() {
        assert!(matches!(
            swap_exact_output(1000, 1000, 90, 10_000),
            Err(AmmMathError::InvalidFee)
        ));
    }

    // Round-trip: input from exact-input ≥ input quoted by exact-output for same amount_out
    // (the exact-output ceiling rounds up; exact-input floor rounds down on the inverse direction)
    #[test]
    fn swap_exact_output_roundtrip_against_exact_input() {
        let exact_out = swap_exact_output(1000, 1000, 90, 30).unwrap();
        let exact_in = swap_exact_input(1000, 1000, exact_out.amount_in, 30).unwrap();
        // Paying exact_out.amount_in gets you at least the requested amount_out.
        assert!(exact_in.amount_out >= 90);
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

        #[test]
        fn reserve_identity(
            reserve_in in 1u64..(1u64 << 40),
            reserve_out in  1u64..(1u64 << 40),
            amount_in in  100u64..(1u64 << 40),
            bp in 10u16..70u16,
        ) {
            let quote = swap_exact_input(
                reserve_in,
                reserve_out,
                amount_in,
                bp,
            ).expect("valid quote");
            prop_assert_eq!(
                quote.new_reserve_in, reserve_in + amount_in);
            prop_assert_eq!(
                quote.new_reserve_out, reserve_out - quote.amount_out);
        }

        #[test]
        fn k_did_not_shrink(
            reserve_in in 1u64..(1u64 << 40),
            reserve_out in  1u64..(1u64 << 40),
            amount_in in  100u64..(1u64 << 40),
            bp in 10u16..10_000,
        ) {
            let quote = swap_exact_input(
                reserve_in,
                reserve_out,
                amount_in,
                bp,
            ).expect("valid quote");

            let old_k = (reserve_in as u128) * (reserve_out as u128);
            let new_k = (quote.new_reserve_in as u128) * (quote.new_reserve_out as u128);

            prop_assert!(new_k >= old_k);
        }

        #[test]
        fn pre_fee_check(
            reserve_in in 1u64..(1u64 << 40),
            reserve_out in  1u64..(1u64 << 40),
            amount_in in  100u64..(1u64 << 40),
            bp in 10u16..10_000,
        ) {
            let quote = swap_exact_input(
                reserve_in,
                reserve_out,
                amount_in,
                bp,
            ).expect("valid quote");

            let lhs = (reserve_in as u128 + quote.amount_in_after_fee as u128)
                    * (quote.new_reserve_out as u128);
             let rhs = (reserve_in as u128) * (reserve_out as u128);

            prop_assert!(lhs >= rhs);
        }

        // new_reserve_in = reserve_in + amount_in;  new_reserve_out = reserve_out - amount_out
        #[test]
        fn exact_output_reserve_identity(
            reserve_in  in 1u64..(1u64 << 30),
            reserve_out in 1000u64..(1u64 << 30),
            amount_out  in 1u64..1000,
            fee_bps     in 0u16..1000,
        ) {
            let quote = swap_exact_output(reserve_in, reserve_out, amount_out, fee_bps)
                .expect("valid quote");
            prop_assert_eq!(quote.new_reserve_in,  reserve_in  + quote.amount_in);
            prop_assert_eq!(quote.new_reserve_out, reserve_out - amount_out);
        }

        // new_k ≥ old_k
        #[test]
        fn exact_output_k_did_not_shrink(
            reserve_in  in 1u64..(1u64 << 30),
            reserve_out in 1000u64..(1u64 << 30),
            amount_out  in 1u64..1000,
            fee_bps     in 0u16..1000,
        ) {
            let quote = swap_exact_output(reserve_in, reserve_out, amount_out, fee_bps)
                .expect("valid quote");

            let old_k = (reserve_in as u128) * (reserve_out as u128);
            let new_k = (quote.new_reserve_in as u128) * (quote.new_reserve_out as u128);

            prop_assert!(new_k >= old_k);
        }

        // (reserve_in + amount_in_after_fee) · new_reserve_out  ≥  reserve_in · reserve_out
        #[test]
        fn exact_output_pre_fee_invariant(
            reserve_in  in 1u64..(1u64 << 30),
            reserve_out in 1000u64..(1u64 << 30),
            amount_out  in 1u64..1000,
            fee_bps     in 0u16..1000,
        ) {
            let quote = swap_exact_output(reserve_in, reserve_out, amount_out, fee_bps)
                .expect("valid quote");

            let lhs = (reserve_in as u128 + quote.amount_in_after_fee as u128)
                * (quote.new_reserve_out as u128);
            let rhs = (reserve_in as u128) * (reserve_out as u128);

            prop_assert!(lhs >= rhs);
        }
    }
}
