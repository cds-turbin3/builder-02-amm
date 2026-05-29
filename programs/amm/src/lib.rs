//! Toy constant-product AMM program.
//!
//! Implements the [toy-amm spec](../../../docs/toy-amm.spec.md). All math is
//! delegated to the [`amm_math`] crate: this program is a thin Anchor wrapper
//! that loads accounts, calls pure functions, and applies the resulting
//! deltas to vault and LP-mint state. See
//! [§Anchor Integration Rule](../../../docs/toy-amm.spec.md#anchor-integration-rule).
//!
//! Module map:
//! - `constants`: PDA seed strings, LP mint decimals.
//! - `error`: `AmmError` mapping `amm_math::AmmMathError` variants to
//!   Anchor `#[error_code]` discriminants.
//! - `state`: `Config` account (seed, mint_x, mint_y, fee_bps, locked, bumps).
//! - `instructions/`: one module per instruction (initialize, add_liquidity,
//!   remove_liquidity, swap).

// clippy::diverging_sub_expression fires inside Anchor's #[program] macro
// expansion (a known false positive on the macro-generated error paths).
// The lint pierces module-level #[allow], so we set it at the crate root.
#![allow(clippy::diverging_sub_expression)]

use anchor_lang::prelude::*;

pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

// Test scaffolding: Pool + UserAccounts fixtures, plus the
// AliasMirror impl on Pool. Gated alongside the per-ix Bundle structs
// in `instructions/*.rs` so the on-chain BPF binary stays clean.
#[cfg(all(not(target_os = "solana"), feature = "test-helpers"))]
pub mod test_helpers;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5");

#[program]
pub mod amm {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        seed: u64,
        fee_bps: u16,
        authority: Option<Pubkey>,
    ) -> Result<()> {
        ctx.accounts.init(seed, fee_bps, authority, ctx.bumps)
    }

    pub fn add_liquidity(
        ctx: Context<AddLiquidity>,
        amount_a: u64,
        amount_b: u64,
        min_lp_tokens: u64,
    ) -> Result<()> {
        ctx.accounts.handle(amount_a, amount_b, min_lp_tokens)
    }

    pub fn remove_liquidity(
        ctx: Context<RemoveLiquidity>,
        lp_burn: u64,
        min_a: u64,
        min_b: u64,
    ) -> Result<()> {
        ctx.accounts.handle(lp_burn, min_a, min_b)
    }

    pub fn swap(ctx: Context<Swap>, kind: SwapKind, a_to_b: bool) -> Result<()> {
        ctx.accounts.swap(kind, a_to_b)
    }

    pub fn update_fee(ctx: Context<UpdateFee>, new_fee_bps: u16) -> Result<()> {
        ctx.accounts.update_fee(new_fee_bps)
    }

    pub fn set_locked(ctx: Context<SetLocked>, locked: bool) -> Result<()> {
        ctx.accounts.set_locked(locked)
    }

    pub fn update_authority(
        ctx: Context<UpdateAuthority>,
        new_authority: Option<Pubkey>,
    ) -> Result<()> {
        ctx.accounts.update_authority(new_authority)
    }
}
