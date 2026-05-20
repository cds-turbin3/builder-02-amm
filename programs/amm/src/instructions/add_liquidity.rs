//! Add liquidity to an existing pool. Two cases:
//! - First deposit (`mint_lp.supply == 0`): delegate to
//!   [`amm_math::initial_liquidity`] and additionally mint `MINIMUM_LIQUIDITY`
//!   to `lp_vault`, which is owned by `config` and never moved by any
//!   instruction (effectively burned). See
//!   [§Initial Liquidity](../../../../docs/toy-amm.spec.md#initial-liquidity).
//! - Subsequent deposit: delegate to [`amm_math::add_liquidity`], which uses
//!   the floor-min formula to prevent dilution. See
//!   [§Adding Liquidity](../../../../docs/toy-amm.spec.md#adding-liquidity).
//!
//! API is deposit-driven: the caller fixes `(amount_a, amount_b)` and the
//! handler transfers them as-is, minting `min(floor(a*supply/r_a),
//! floor(b*supply/r_b))` LP. The caller is responsible for computing the
//! ratio-correct pair off-chain; this handler does not refund excess.

use crate::constants::{CONFIG_SEED, LP_MINT_SEED};
use crate::error::AmmError;
use crate::state::Config;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{
    mint_to, transfer_checked, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
};

#[derive(Accounts)]
#[cfg_attr(
    feature = "test-helpers",
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(AddLiquidityBundle)
)]
pub struct AddLiquidity<'info> {
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

    // lp_vault is the permanent-lock ATA created in `initialize`. Only the
    // first deposit writes to it (minting MINIMUM_LIQUIDITY); subsequent
    // deposits leave it untouched.
    #[account(mut, associated_token::mint = mint_lp, associated_token::authority = config)]
    pub lp_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_x, associated_token::authority = user)]
    pub user_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = mint_y, associated_token::authority = user)]
    pub user_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint_lp,
        associated_token::authority = user,
    )]
    pub user_lp: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> AddLiquidity<'info> {
    pub fn handle(&mut self, amount_a: u64, amount_b: u64, min_lp_tokens: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);

        let supply = self.mint_lp.supply;
        let is_first = supply == 0;
        let quote = if is_first {
            amm_math::initial_liquidity(amount_a, amount_b).map_err(AmmError::from)?
        } else {
            amm_math::add_liquidity(
                self.vault_x.amount,
                self.vault_y.amount,
                supply,
                amount_a,
                amount_b,
            )
            .map_err(AmmError::from)?
        };

        require!(quote.lp_tokens >= min_lp_tokens, AmmError::SlippageExceeded);

        // Transfer the contributed amounts from user ATAs to the vaults.
        // The user signs; no PDA seeds needed.
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: self.user_x.to_account_info(),
                    to: self.vault_x.to_account_info(),
                    mint: self.mint_x.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            quote.amount_a,
            self.mint_x.decimals,
        )?;
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: self.user_y.to_account_info(),
                    to: self.vault_y.to_account_info(),
                    mint: self.mint_y.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            quote.amount_b,
            self.mint_y.decimals,
        )?;

        // PDA signer for mint_lp's authority (config). Bind the bytes to
        // locals so the &[u8] refs in `pda_seeds` outlive the CPI call.
        let seed_bytes = self.config.seed.to_le_bytes();
        let bump = [self.config.config_bump];
        let pda_seeds: &[&[u8]] = &[CONFIG_SEED, &seed_bytes, &bump];
        let signer = &[pda_seeds];

        // Mint LP shares to the user.
        mint_to(
            CpiContext::new_with_signer(
                self.token_program.key(),
                MintTo {
                    mint: self.mint_lp.to_account_info(),
                    to: self.user_lp.to_account_info(),
                    authority: self.config.to_account_info(),
                },
                signer,
            ),
            quote.lp_tokens,
        )?;

        // First deposit: mint MINIMUM_LIQUIDITY to the permanent-lock vault.
        // The spec [§Initial Liquidity] requires this to land in total_lp_supply
        // (V2 semantics: locked share participates in pool growth) so we use
        // a config-owned vault rather than burning from the user.
        if is_first {
            mint_to(
                CpiContext::new_with_signer(
                    self.token_program.key(),
                    MintTo {
                        mint: self.mint_lp.to_account_info(),
                        to: self.lp_vault.to_account_info(),
                        authority: self.config.to_account_info(),
                    },
                    signer,
                ),
                amm_math::MINIMUM_LIQUIDITY,
            )?;
        }

        Ok(())
    }
}

/// Pubkey bundle for [`AddLiquidity`] tests. The three program fields
/// auto-fill; populate `config`, `mint_lp`, `vault_x`, `vault_y`,
/// `lp_vault`, `user_x`, `user_y`, `user_lp` with the right ATAs / PDAs.
#[cfg(feature = "test-helpers")]
#[derive(anchor_litesvm::Bundle, Copy, Clone)]
pub struct AddLiquidityBundle {
    pub user: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub config: Pubkey,
    pub mint_lp: Pubkey,
    pub vault_x: Pubkey,
    pub vault_y: Pubkey,
    pub lp_vault: Pubkey,
    pub user_x: Pubkey,
    pub user_y: Pubkey,
    pub user_lp: Pubkey,
}
