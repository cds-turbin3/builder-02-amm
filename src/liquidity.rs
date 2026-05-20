use crate::{AmmMathError, LiquidityQuote, checked_mul_div_floor, integer_sqrt_floor, narrow_u64};

pub const MINIMUM_LIQUIDITY: u64 = 1_000;

pub fn initial_liquidity(amount_a: u64, amount_b: u64) -> Result<LiquidityQuote, AmmMathError> {
    if amount_a == 0 || amount_b == 0 {
        return Err(AmmMathError::ZeroAmount);
    };

    let product = (amount_a as u128)
        .checked_mul(amount_b as u128)
        .ok_or(AmmMathError::Overflow)?;

    let minted = narrow_u64(integer_sqrt_floor(product))?;
    if minted <= MINIMUM_LIQUIDITY {
        return Err(AmmMathError::InsufficientLiquidity);
    }

    let lp_tokens = minted - MINIMUM_LIQUIDITY;

    Ok(LiquidityQuote {
        lp_tokens,
        amount_a,
        amount_b,
    })
}

pub fn add_liquidity(
    reserve_a: u64,
    reserve_b: u64,
    total_lp_supply: u64,
    amount_a: u64,
    amount_b: u64,
) -> Result<LiquidityQuote, AmmMathError> {
    if amount_a == 0 || amount_b == 0 {
        return Err(AmmMathError::ZeroAmount);
    }
    if reserve_a == 0 || reserve_b == 0 {
        return Err(AmmMathError::ZeroReserve);
    }

    let lp_from_a =
        checked_mul_div_floor(amount_a as u128, total_lp_supply as u128, reserve_a as u128)?;
    let lp_from_b =
        checked_mul_div_floor(amount_b as u128, total_lp_supply as u128, reserve_b as u128)?;

    let lp_tokens = narrow_u64(lp_from_a.min(lp_from_b))?;
    if lp_tokens == 0 {
        return Err(AmmMathError::InsufficientLiquidity);
    }

    Ok(LiquidityQuote {
        lp_tokens,
        amount_a,
        amount_b,
    })
}

pub fn remove_liquidity(
    reserve_a: u64,
    reserve_b: u64,
    total_lp_supply: u64,
    lp_tokens: u64,
) -> Result<LiquidityQuote, AmmMathError> {
    if lp_tokens == 0 {
        return Err(AmmMathError::ZeroAmount);
    }
    if reserve_a == 0 || reserve_b == 0 {
        return Err(AmmMathError::ZeroReserve);
    }
    if total_lp_supply == 0 || lp_tokens > total_lp_supply {
        return Err(AmmMathError::InsufficientLiquidity);
    }

    let amount_a = narrow_u64(checked_mul_div_floor(
        lp_tokens as u128,
        reserve_a as u128,
        total_lp_supply as u128,
    )?)?;
    let amount_b = narrow_u64(checked_mul_div_floor(
        lp_tokens as u128,
        reserve_b as u128,
        total_lp_supply as u128,
    )?)?;

    if amount_a == 0 || amount_b == 0 {
        return Err(AmmMathError::InsufficientLiquidity);
    }

    Ok(LiquidityQuote {
        lp_tokens,
        amount_a,
        amount_b,
    })
}

#[cfg(test)]
mod tests {
    mod initial_liquidity {
        use super::super::*;
        use proptest::prelude::*;

        // sqrt(10_000 * 10_000) = 10_000; 10_000 - 1_000 = 9_000
        #[test]
        fn balanced_pool() {
            let q = initial_liquidity(10_000, 10_000).expect("valid balanced pool");
            assert_eq!(q.lp_tokens, 9_000);
            assert_eq!(q.amount_a, 10_000);
            assert_eq!(q.amount_b, 10_000);
        }

        // amount = 0  ->  Err(ZeroAmount)
        #[test]
        fn fails_with_zero_amount() {
            assert!(matches!(
                initial_liquidity(0, 10_000),
                Err(AmmMathError::ZeroAmount)
            ));
            assert!(matches!(
                initial_liquidity(10_000, 0),
                Err(AmmMathError::ZeroAmount)
            ));
        }

        // sqrt(a * b) <= MINIMUM_LIQUIDITY  ->  Err(InsufficientLiquidity)
        #[test]
        fn fails_with_insufficient_liquidity() {
            assert!(matches!(
                initial_liquidity(10, 10),
                Err(AmmMathError::InsufficientLiquidity)
            ));
        }

        // sqrt(1_001 * 1_001) - MINIMUM_LIQUIDITY = 1  (smallest non-erroring case)
        #[test]
        fn boundary_liquidity_is_exactly_one() {
            let q = initial_liquidity(1_001, 1_001).expect("lp pool");
            assert_eq!(q.lp_tokens, 1);
        }

        // sqrt(1_000 * 1_000) - MINIMUM_LIQUIDITY = 0  ->  Err(InsufficientLiquidity)
        #[test]
        fn boundary_fails_when_there_is_not_enough_to_reserve() {
            assert!(matches!(
                initial_liquidity(1_000, 1_000),
                Err(AmmMathError::InsufficientLiquidity)
            ));
        }

        proptest! {
            // (lp_tokens + MINIMUM_LIQUIDITY)^2  <=  a * b  <  (lp_tokens + MINIMUM_LIQUIDITY + 1)^2
            #[test]
            fn sqrt_property(
                a in 1u64..(1u64 << 30),
                b in 1u64..(1u64 << 30),
            ) {
                let result = initial_liquidity(a, b);
                prop_assume!(result.is_ok());
                let quote = result.unwrap();

                let minted = (quote.lp_tokens as u128) + (MINIMUM_LIQUIDITY as u128);
                let product = (a as u128) * (b as u128);

                prop_assert!(minted * minted <= product);
                prop_assert!((minted + 1) * (minted + 1) > product);
            }

            // initial_liquidity(a, b).lp_tokens = initial_liquidity(b, a).lp_tokens
            #[test]
            fn symmetric_property(
                a in 1u64..(1u64 << 30),
                b in 1u64..(1u64 << 30),
            ) {
                let qa = initial_liquidity(a, b);
                let qb = initial_liquidity(b, a);

                match (qa, qb) {
                    (Ok(q1), Ok(q2)) => prop_assert_eq!(q1.lp_tokens, q2.lp_tokens),
                    (Err(e1), Err(e2)) => prop_assert_eq!(e1, e2),
                    _ => prop_assert!(false, "asymmetric Ok/Err"),
                }
            }
        }
    }

    mod add_liquidity {
        use super::super::*;
        use proptest::prelude::*;

        // floor(100*1000/1000) = floor(100*1000/1000) = 100;  min = 100
        #[test]
        fn balanced_at_ratio() {
            let q = add_liquidity(1000, 1000, 1000, 100, 100).unwrap();
            assert_eq!(q.lp_tokens, 100);
            assert_eq!(q.amount_a, 100);
            assert_eq!(q.amount_b, 100);
        }

        // Pool ratio 1:4;  deposit 100:400 matches ratio -> floor(100*2000/1000) = floor(400*2000/4000) = 200
        #[test]
        fn balanced_non_unit_ratio() {
            let q = add_liquidity(1000, 4000, 2000, 100, 400).unwrap();
            assert_eq!(q.lp_tokens, 200);
        }

        // Excess A (200 vs ratio of 100):  floor(200*1000/1000) = 200, floor(100*1000/1000) = 100;  min = 100
        #[test]
        fn excess_a_takes_min_from_b() {
            let q = add_liquidity(1000, 1000, 1000, 200, 100).unwrap();
            assert_eq!(q.lp_tokens, 100);
        }

        // Excess B (200 vs ratio of 100):  min = 100 (from A side)
        #[test]
        fn excess_b_takes_min_from_a() {
            let q = add_liquidity(1000, 1000, 1000, 100, 200).unwrap();
            assert_eq!(q.lp_tokens, 100);
        }

        // amount = 0  ->  Err(ZeroAmount)
        #[test]
        fn fails_with_zero_amount() {
            assert!(matches!(
                add_liquidity(1000, 1000, 1000, 0, 100),
                Err(AmmMathError::ZeroAmount)
            ));
            assert!(matches!(
                add_liquidity(1000, 1000, 1000, 100, 0),
                Err(AmmMathError::ZeroAmount)
            ));
        }

        // reserve = 0  ->  Err(ZeroReserve)
        #[test]
        fn fails_with_zero_reserve() {
            assert!(matches!(
                add_liquidity(0, 1000, 1000, 100, 100),
                Err(AmmMathError::ZeroReserve)
            ));
            assert!(matches!(
                add_liquidity(1000, 0, 1000, 100, 100),
                Err(AmmMathError::ZeroReserve)
            ));
        }

        // Tiny deposit rounds to 0 LP tokens  ->  Err(InsufficientLiquidity)
        // floor(1*1000/1_000_001) = 0
        #[test]
        fn fails_when_rounds_to_zero() {
            assert!(matches!(
                add_liquidity(1_000_001, 1_000_001, 1000, 1, 1),
                Err(AmmMathError::InsufficientLiquidity)
            ));
        }

        // Uninitialized pool (total_lp_supply = 0)  ->  Err(InsufficientLiquidity)
        // floor(amount*0/reserve) = 0 for both sides; min = 0
        #[test]
        fn fails_when_supply_is_zero() {
            assert!(matches!(
                add_liquidity(1000, 1000, 0, 100, 100),
                Err(AmmMathError::InsufficientLiquidity)
            ));
        }

        proptest! {
            // lp_tokens * reserve_a  <=  amount_a * total_lp_supply  (no dilution in A)
            // lp_tokens * reserve_b  <=  amount_b * total_lp_supply  (no dilution in B)
            #[test]
            fn no_dilution(
                reserve_a in 1u64..(1u64 << 30),
                reserve_b in 1u64..(1u64 << 30),
                total_lp_supply in 1u64..(1u64 << 30),
                amount_a in 1u64..(1u64 << 30),
                amount_b in 1u64..(1u64 << 30),
            ) {
                let result = add_liquidity(reserve_a, reserve_b, total_lp_supply, amount_a, amount_b);
                prop_assume!(result.is_ok());
                let quote = result.unwrap();

                let lp = quote.lp_tokens as u128;
                prop_assert!(lp * (reserve_a as u128) <= (amount_a as u128) * (total_lp_supply as u128));
                prop_assert!(lp * (reserve_b as u128) <= (amount_b as u128) * (total_lp_supply as u128));
            }

            // lp_tokens = min(floor(amount_a * total_lp_supply / reserve_a),
            //                 floor(amount_b * total_lp_supply / reserve_b))
            #[test]
            fn matches_spec_formula(
                reserve_a in 1u64..(1u64 << 30),
                reserve_b in 1u64..(1u64 << 30),
                total_lp_supply in 1u64..(1u64 << 30),
                amount_a in 1u64..(1u64 << 30),
                amount_b in 1u64..(1u64 << 30),
            ) {
                let result = add_liquidity(reserve_a, reserve_b, total_lp_supply, amount_a, amount_b);
                prop_assume!(result.is_ok());
                let quote = result.unwrap();

                let lp_a = (amount_a as u128 * total_lp_supply as u128) / reserve_a as u128;
                let lp_b = (amount_b as u128 * total_lp_supply as u128) / reserve_b as u128;
                let expected = lp_a.min(lp_b);

                prop_assert_eq!(quote.lp_tokens as u128, expected);
            }
        }
    }

    mod remove_liquidity {
        use super::super::*;
        use proptest::prelude::*;

        // floor(100*1000/1000) = 100 of each
        #[test]
        fn balanced_withdrawal() {
            let q = remove_liquidity(1000, 1000, 1000, 100).unwrap();
            assert_eq!(q.lp_tokens, 100);
            assert_eq!(q.amount_a, 100);
            assert_eq!(q.amount_b, 100);
        }

        // Pool ratio 1:4;  burn 100 of 2000 supply -> floor(100*1000/2000) = 50, floor(100*4000/2000) = 200
        #[test]
        fn non_unit_ratio_withdrawal() {
            let q = remove_liquidity(1000, 4000, 2000, 100).unwrap();
            assert_eq!(q.amount_a, 50);
            assert_eq!(q.amount_b, 200);
        }

        // Burn entire supply -> get all reserves
        #[test]
        fn full_withdrawal_returns_all_reserves() {
            let q = remove_liquidity(1000, 4000, 2000, 2000).unwrap();
            assert_eq!(q.amount_a, 1000);
            assert_eq!(q.amount_b, 4000);
        }

        // lp_tokens = 0  ->  Err(ZeroAmount)
        #[test]
        fn fails_with_zero_lp_tokens() {
            assert!(matches!(
                remove_liquidity(1000, 1000, 1000, 0),
                Err(AmmMathError::ZeroAmount)
            ));
        }

        // reserve = 0  ->  Err(ZeroReserve)
        #[test]
        fn fails_with_zero_reserve() {
            assert!(matches!(
                remove_liquidity(0, 1000, 1000, 100),
                Err(AmmMathError::ZeroReserve)
            ));
            assert!(matches!(
                remove_liquidity(1000, 0, 1000, 100),
                Err(AmmMathError::ZeroReserve)
            ));
        }

        // total_lp_supply = 0  ->  Err(InsufficientLiquidity)
        #[test]
        fn fails_when_supply_is_zero() {
            assert!(matches!(
                remove_liquidity(1000, 1000, 0, 100),
                Err(AmmMathError::InsufficientLiquidity)
            ));
        }

        // lp_tokens > total_lp_supply  ->  Err(InsufficientLiquidity)
        #[test]
        fn fails_when_burning_more_than_supply() {
            assert!(matches!(
                remove_liquidity(1000, 1000, 1000, 1001),
                Err(AmmMathError::InsufficientLiquidity)
            ));
        }

        // floor(1 * 1000 / 1_000_001) = 0  ->  Err(InsufficientLiquidity)
        #[test]
        fn fails_when_rounds_to_zero() {
            assert!(matches!(
                remove_liquidity(1000, 1000, 1_000_001, 1),
                Err(AmmMathError::InsufficientLiquidity)
            ));
        }

        proptest! {
            // amount_a * total_lp_supply  <=  lp_tokens * reserve_a  (no overpay in A)
            // amount_b * total_lp_supply  <=  lp_tokens * reserve_b  (no overpay in B)
            #[test]
            fn no_overpay(
                reserve_a in 1u64..(1u64 << 30),
                reserve_b in 1u64..(1u64 << 30),
                total_lp_supply in 1u64..(1u64 << 30),
                lp_tokens in 1u64..(1u64 << 30),
            ) {
                let result = remove_liquidity(reserve_a, reserve_b, total_lp_supply, lp_tokens);
                prop_assume!(result.is_ok());
                let quote = result.unwrap();

                let a = quote.amount_a as u128;
                let b = quote.amount_b as u128;
                prop_assert!(a * (total_lp_supply as u128) <= (lp_tokens as u128) * (reserve_a as u128));
                prop_assert!(b * (total_lp_supply as u128) <= (lp_tokens as u128) * (reserve_b as u128));
            }

            // amount_a = floor(lp_tokens * reserve_a / total_lp_supply)
            // amount_b = floor(lp_tokens * reserve_b / total_lp_supply)
            #[test]
            fn matches_spec_formula(
                reserve_a in 1u64..(1u64 << 30),
                reserve_b in 1u64..(1u64 << 30),
                total_lp_supply in 1u64..(1u64 << 30),
                lp_tokens in 1u64..(1u64 << 30),
            ) {
                let result = remove_liquidity(reserve_a, reserve_b, total_lp_supply, lp_tokens);
                prop_assume!(result.is_ok());
                let quote = result.unwrap();

                let expected_a = (lp_tokens as u128 * reserve_a as u128) / total_lp_supply as u128;
                let expected_b = (lp_tokens as u128 * reserve_b as u128) / total_lp_supply as u128;
                prop_assert_eq!(quote.amount_a as u128, expected_a);
                prop_assert_eq!(quote.amount_b as u128, expected_b);
            }

            // Round-trip: add then remove the minted LP returns at most what was deposited.
            // The user can never extract more value than they deposited (floor on both sides
            // means the pool always retains rounding remainders).
            #[test]
            fn add_then_remove_does_not_extract_value(
                reserve_a in 1u64..(1u64 << 30),
                reserve_b in 1u64..(1u64 << 30),
                total_lp_supply in 1u64..(1u64 << 30),
                amount_a in 1u64..(1u64 << 30),
                amount_b in 1u64..(1u64 << 30),
            ) {
                let add_result = add_liquidity(reserve_a, reserve_b, total_lp_supply, amount_a, amount_b);
                prop_assume!(add_result.is_ok());
                let add_quote = add_result.unwrap();
                let lp_minted = add_quote.lp_tokens;

                let new_reserve_a = reserve_a + amount_a;
                let new_reserve_b = reserve_b + amount_b;
                let new_supply = total_lp_supply + lp_minted;

                let remove_result = remove_liquidity(new_reserve_a, new_reserve_b, new_supply, lp_minted);
                prop_assume!(remove_result.is_ok());
                let remove_quote = remove_result.unwrap();

                prop_assert!(remove_quote.amount_a <= amount_a);
                prop_assert!(remove_quote.amount_b <= amount_b);
            }
        }
    }
}
