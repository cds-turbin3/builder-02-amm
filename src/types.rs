#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapQuote {
    pub amount_in: u64,
    pub amount_in_after_fee: u64,
    pub fee_amount: u64,
    pub amount_out: u64,
    pub new_reserve_in: u64,
    pub new_reserve_out: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactOutputQuote {
    pub amount_out: u64,
    pub amount_in_after_fee: u64,
    pub amount_in: u64,
    pub fee_amount: u64,
    pub new_reserve_in: u64,
    pub new_reserve_out: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidityQuote {
    pub lp_tokens: u64,
    pub amount_a: u64,
    pub amount_b: u64,
}
