//! Persistent on-chain state.

use anchor_lang::prelude::*;

/// Per-pool configuration. PDA derived as
/// `[CONFIG_SEED, seed.to_le_bytes()]`; `seed` lets several independent pools
/// share the same `(mint_x, mint_y)` pair without colliding.
///
/// `fee_bps` must satisfy `fee_bps < amm_math::FEE_DENOMINATOR` (10_000);
/// see [§Fee Policy](../../../docs/toy-amm.spec.md#fee-policy).
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub seed: u64,
    pub authority: Option<Pubkey>,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub fee_bps: u16,
    pub locked: bool,
    pub config_bump: u8,
    pub lp_bump: u8,
}
