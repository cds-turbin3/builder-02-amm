//! Swap one token for the other. One [`Swap`] accounts struct, one `swap`
//! instruction handler in [`crate::amm`], one `SwapKind` enum that selects
//! between exact-input and exact-output at the data layer. The two pure
//! formulas in `amm_math` (`swap_exact_input` / `swap_exact_output`) sit
//! behind that dispatch.
//!
//! See [§Swap Formula (Exact Input)](../../../../docs/toy-amm.spec.md#swap-formula-exact-input)
//! and [§Swap Formula (Exact Output)](../../../../docs/toy-amm.spec.md#swap-formula-exact-output).
//!
//! `a_to_b` selects direction: when true, user sends mint_x and receives mint_y;
//! when false, the reverse. The handler picks the right (reserve_in, reserve_out)
//! pair before calling into `amm_math`.
//!
//! The single-instruction shape is deliberate: it keeps `instruction::Swap`
//! lined up with the `Swap` accounts struct, which is what the BundledPubkeys
//! derive expects (it pairs `crate::instruction::<accounts_ident>` with
//! `crate::accounts::<accounts_ident>`).

use crate::constants::CONFIG_SEED;
use crate::error::AmmError;
use crate::state::Config;
use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

#[derive(Accounts)]
#[cfg_attr(
    feature = "test-helpers",
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(SwapBundle)
)]
pub struct Swap<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    pub mint_x: Box<InterfaceAccount<'info, Mint>>,
    pub mint_y: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        has_one = mint_x,
        has_one = mint_y,
        seeds = [CONFIG_SEED, config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump,
    )]
    pub config: Box<Account<'info, Config>>,

    #[account(mut, associated_token::mint = mint_x, associated_token::authority = config)]
    pub vault_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_y, associated_token::authority = config)]
    pub vault_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_x, associated_token::authority = user)]
    pub user_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_y, associated_token::authority = user)]
    pub user_y: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}

/// Data-layer discriminator between the two swap formulas. Serialized via
/// Anchor's default Borsh encoding as part of the `swap` instruction args.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub enum SwapKind {
    ExactInput { amount_in: u64, min_amount_out: u64 },
    ExactOutput { amount_out: u64, max_amount_in: u64 },
}

impl<'info> Swap<'info> {
    pub fn swap(&mut self, kind: SwapKind, a_to_b: bool) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);

        // Pick reserves based on direction. a_to_b == true means user sends
        // mint_x and receives mint_y, so reserve_in is vault_x.
        let (reserve_in, reserve_out) = if a_to_b {
            (self.vault_x.amount, self.vault_y.amount)
        } else {
            (self.vault_y.amount, self.vault_x.amount)
        };

        // Run the math + slippage check. Returns the (amount_in, amount_out)
        // pair that will actually move, regardless of which formula we used.
        let (amount_in, amount_out) = match kind {
            SwapKind::ExactInput {
                amount_in: target_in,
                min_amount_out,
            } => {
                let quote = amm_math::swap_exact_input(
                    reserve_in,
                    reserve_out,
                    target_in,
                    self.config.fee_bps,
                )
                .map_err(AmmError::from)?;
                require!(
                    quote.amount_out >= min_amount_out,
                    AmmError::SlippageExceeded
                );
                (quote.amount_in, quote.amount_out)
            }
            SwapKind::ExactOutput {
                amount_out: target_out,
                max_amount_in,
            } => {
                let quote = amm_math::swap_exact_output(
                    reserve_in,
                    reserve_out,
                    target_out,
                    self.config.fee_bps,
                )
                .map_err(AmmError::from)?;
                require!(quote.amount_in <= max_amount_in, AmmError::SlippageExceeded);
                (quote.amount_in, quote.amount_out)
            }
        };

        // Pick direction-specific AccountInfos. `decimals_*` come along for
        // the transfer_checked decimals arg.
        let (user_in, user_out, vault_in, vault_out, mint_in, mint_out, decimals_in, decimals_out) =
            if a_to_b {
                (
                    self.user_x.to_account_info(),
                    self.user_y.to_account_info(),
                    self.vault_x.to_account_info(),
                    self.vault_y.to_account_info(),
                    self.mint_x.to_account_info(),
                    self.mint_y.to_account_info(),
                    self.mint_x.decimals,
                    self.mint_y.decimals,
                )
            } else {
                (
                    self.user_y.to_account_info(),
                    self.user_x.to_account_info(),
                    self.vault_y.to_account_info(),
                    self.vault_x.to_account_info(),
                    self.mint_y.to_account_info(),
                    self.mint_x.to_account_info(),
                    self.mint_y.decimals,
                    self.mint_x.decimals,
                )
            };

        // Leg 1: user -> vault_in. User signs.
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: user_in,
                    to: vault_in,
                    mint: mint_in,
                    authority: self.user.to_account_info(),
                },
            ),
            amount_in,
            decimals_in,
        )?;

        // Leg 2: vault_out -> user. Config PDA signs.
        let seed_bytes = self.config.seed.to_le_bytes();
        let bump = [self.config.config_bump];
        let pda_seeds: &[&[u8]] = &[CONFIG_SEED, &seed_bytes, &bump];
        let signer = &[pda_seeds];

        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: vault_out,
                    to: user_out,
                    mint: mint_out,
                    authority: self.config.to_account_info(),
                },
                signer,
            ),
            amount_out,
            decimals_out,
        )?;

        Ok(())
    }
}

/// Pubkey bundle for [`Swap`] tests. The `token_program` field auto-fills
/// with the legacy SPL Token id; for Token-2022 tests, build the ix with
/// `Program::build_ix_with(...)` and override `token_program` in the closure.
///
/// `BundleFrom` lets tests construct the bundle from a `(Pool, UserAccounts)`
/// pair in one call: `SwapBundle::from((&pool, &user))`. The `#[from(u.x)]`
/// overrides handle fields where the source binding's field name doesn't
/// match the bundle's (`user_x` ← `u.ata_x`).
#[cfg(feature = "test-helpers")]
#[derive(anchor_litesvm::Bundle, anchor_litesvm::BundleFrom, Copy, Clone)]
#[from_fixtures(p: crate::test_helpers::Pool, u: crate::test_helpers::UserAccounts)]
pub struct SwapBundle {
    #[from(u.pubkey())]
    pub user: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub config: Pubkey,
    pub vault_x: Pubkey,
    pub vault_y: Pubkey,
    #[from(u.ata_x)]
    pub user_x: Pubkey,
    #[from(u.ata_y)]
    pub user_y: Pubkey,
}
