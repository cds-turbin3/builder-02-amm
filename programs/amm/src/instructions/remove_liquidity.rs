//! Remove liquidity from the pool. Delegates to [`amm_math::remove_liquidity`],
//! which uses floor on both sides (the spec convention favors the pool over
//! the burner; see [§Removing Liquidity](../../../../docs/toy-amm.spec.md#removing-liquidity)).
//!
//! The user supplies `min_a` and `min_b` to bound slippage; the handler must
//! reject if either output falls below the floor.

use crate::constants::{CONFIG_SEED, LP_MINT_SEED};
use crate::error::AmmError;
use crate::state::Config;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{
    burn, transfer_checked, Burn, Mint, TokenAccount, TokenInterface, TransferChecked,
};

#[derive(Accounts)]
#[cfg_attr(
    feature = "test-helpers",
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(RemoveLiquidityBundle)
)]
pub struct RemoveLiquidity<'info> {
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

    #[account(
        mut,
        seeds = [LP_MINT_SEED, config.key().as_ref()],
        bump = config.lp_bump,
    )]
    pub mint_lp: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut, associated_token::mint = mint_x, associated_token::authority = config)]
    pub vault_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_y, associated_token::authority = config)]
    pub vault_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_x, associated_token::authority = user)]
    pub user_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_y, associated_token::authority = user)]
    pub user_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_lp, associated_token::authority = user)]
    pub user_lp: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> RemoveLiquidity<'info> {
    pub fn handle(&mut self, lp_burn: u64, min_a: u64, min_b: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);

        let quote = amm_math::remove_liquidity(
            self.vault_x.amount,
            self.vault_y.amount,
            self.mint_lp.supply,
            lp_burn,
        )
        .map_err(AmmError::from)?;

        require!(quote.amount_a >= min_a, AmmError::SlippageExceeded);
        require!(quote.amount_b >= min_b, AmmError::SlippageExceeded);

        // Burn user's LP. User signs; no PDA needed.
        burn(
            CpiContext::new(
                self.token_program.key(),
                Burn {
                    mint: self.mint_lp.to_account_info(),
                    from: self.user_lp.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            quote.lp_tokens,
        )?;

        // PDA signer for vault -> user transfers (config owns the vaults).
        let seed_bytes = self.config.seed.to_le_bytes();
        let bump = [self.config.config_bump];
        let pda_seeds: &[&[u8]] = &[CONFIG_SEED, &seed_bytes, &bump];
        let signer = &[pda_seeds];

        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: self.vault_x.to_account_info(),
                    to: self.user_x.to_account_info(),
                    mint: self.mint_x.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                signer,
            ),
            quote.amount_a,
            self.mint_x.decimals,
        )?;
        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: self.vault_y.to_account_info(),
                    to: self.user_y.to_account_info(),
                    mint: self.mint_y.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                signer,
            ),
            quote.amount_b,
            self.mint_y.decimals,
        )?;

        Ok(())
    }
}

/// Pubkey bundle for [`RemoveLiquidity`] tests. Same shape as
/// `AddLiquidityBundle` (the two instructions share the same set of
/// non-program accounts); declared as a distinct type so tests don't have to
/// reach across instruction modules.
#[cfg(feature = "test-helpers")]
#[derive(anchor_litesvm::Bundle, anchor_litesvm::BundleFrom, Copy, Clone)]
#[from_fixtures(p: crate::test_helpers::Pool, u: crate::test_helpers::UserAccounts)]
pub struct RemoveLiquidityBundle {
    #[from(u.pubkey())]
    pub user: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub config: Pubkey,
    pub mint_lp: Pubkey,
    pub vault_x: Pubkey,
    pub vault_y: Pubkey,
    #[from(u.ata_x)]
    pub user_x: Pubkey,
    #[from(u.ata_y)]
    pub user_y: Pubkey,
    #[from(u.ata_lp(&p.mint_lp))]
    pub user_lp: Pubkey,
}
