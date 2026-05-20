//! On-chain constants. Marked `#[constant]` so they surface in the generated IDL.

use anchor_lang::prelude::*;

/// Seed for the [`Config`](crate::state::Config) PDA.
/// Full seeds: `[CONFIG_SEED, seed.to_le_bytes()]`.
#[constant]
pub const CONFIG_SEED: &[u8] = b"config";

/// Seed for the LP mint PDA.
/// Full seeds: `[LP_MINT_SEED, config.key().as_ref()]`.
#[constant]
pub const LP_MINT_SEED: &[u8] = b"lp";

/// Decimals for the LP mint. The spec is silent on LP decimals (only
/// `FEE_DENOMINATOR` and `MINIMUM_LIQUIDITY` are mandated); six is a project
/// choice carried over from the prior project, giving room for fractional shares.
pub const LP_MINT_DECIMALS: u8 = 6;
