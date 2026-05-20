//! Quote structs returned by swap and liquidity functions.

/// Quote for an exact-input swap.
/// Returned by [`crate::swap_exact_input`].
///
/// `fee_amount` is informational and always equals `amount_in - amount_in_after_fee`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapQuote {
    pub amount_in: u64,
    pub amount_in_after_fee: u64,
    pub fee_amount: u64,
    pub amount_out: u64,
    pub new_reserve_in: u64,
    pub new_reserve_out: u64,
}

/// Quote for an exact-output swap.
/// Returned by [`crate::swap_exact_output`].
///
/// Mirror of [`SwapQuote`] for the inverse direction: caller fixes `amount_out`,
/// the library computes the minimum `amount_in` satisfying the invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactOutputQuote {
    pub amount_out: u64,
    pub amount_in_after_fee: u64,
    pub amount_in: u64,
    pub fee_amount: u64,
    pub new_reserve_in: u64,
    pub new_reserve_out: u64,
}

/// Quote for any liquidity operation (initial, add, remove).
/// Returned by [`crate::initial_liquidity`], [`crate::add_liquidity`], and
/// [`crate::remove_liquidity`].
///
/// For add/initial: `lp_tokens` is the amount minted to the user (initial mint
/// excludes the protocol-locked `MINIMUM_LIQUIDITY`). For remove: `lp_tokens`
/// is the amount the user is burning; `amount_a`, `amount_b` are what they receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidityQuote {
    pub lp_tokens: u64,
    pub amount_a: u64,
    pub amount_b: u64,
}
