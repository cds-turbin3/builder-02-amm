//! Initialize a new pool: creates the [`Config`] PDA, the LP mint PDA, and
//! the two protocol-owned vault ATAs. No liquidity is deposited here; the
//! first deposit goes through [`crate::instructions::add_liquidity`], which
//! delegates to [`amm_math::initial_liquidity`] (see
//! [§Initial Liquidity](../../../../docs/toy-amm.spec.md#initial-liquidity)).

use crate::constants::{CONFIG_SEED, LP_MINT_DECIMALS, LP_MINT_SEED};
use crate::error::AmmError;
use crate::state::Config;
use amm_math::FEE_DENOMINATOR;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

#[derive(Accounts)]
#[cfg_attr(
    feature = "test-helpers",
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(InitializeBundle)
)]
#[instruction(seed: u64)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub initializer: Signer<'info>,
    pub mint_x: Box<InterfaceAccount<'info, Mint>>,
    pub mint_y: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = initializer,
        seeds = [LP_MINT_SEED, config.key().as_ref()],
        bump,
        mint::decimals = LP_MINT_DECIMALS,
        mint::authority = config,
    )]
    pub mint_lp: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = initializer,
        associated_token::mint = mint_x,
        associated_token::authority = config,
    )]
    pub vault_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = initializer,
        associated_token::mint = mint_y,
        associated_token::authority = config,
    )]
    pub vault_y: Box<InterfaceAccount<'info, TokenAccount>>,

    // ATA of mint_lp owned by config. The MINIMUM_LIQUIDITY tokens minted on
    // the first deposit go here and stay there forever: no instruction in this
    // program signs a transfer out of this account, so the tokens are
    // effectively burned. See [§Initial Liquidity] "Minimum liquidity lock".
    #[account(
        init,
        payer = initializer,
        associated_token::mint = mint_lp,
        associated_token::authority = config,
    )]
    pub lp_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = initializer,
        seeds = [CONFIG_SEED, seed.to_le_bytes().as_ref()],
        bump,
        space = Config::DISCRIMINATOR.len() + Config::INIT_SPACE,
    )]
    pub config: Box<Account<'info, Config>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> Initialize<'info> {
    pub fn init(
        &mut self,
        seed: u64,
        fee_bps: u16,
        authority: Option<Pubkey>,
        bumps: InitializeBumps,
    ) -> Result<()> {
        // Spec [§Fee Policy]: fee_bps strictly less than FEE_DENOMINATOR.
        require!((fee_bps as u64) < FEE_DENOMINATOR, AmmError::InvalidFee);

        self.config.set_inner(Config {
            seed,
            authority,
            mint_x: self.mint_x.key(),
            mint_y: self.mint_y.key(),
            fee_bps,
            locked: false,
            config_bump: bumps.config,
            lp_bump: bumps.mint_lp,
        });
        Ok(())
    }
}

/// Pubkey bundle for [`Initialize`] tests. Fields match the non-program
/// accounts by name; the three Anchor program fields (`token_program`,
/// `associated_token_program`, `system_program`) auto-fill via
/// `#[derive(BundledPubkeys)]`. Populate `config`, `mint_lp`, `vault_x`,
/// `vault_y`, `lp_vault` with the correct PDA / ATA addresses before
/// building the ix.
#[cfg(feature = "test-helpers")]
#[derive(anchor_litesvm::Bundle, Copy, Clone)]
pub struct InitializeBundle {
    pub initializer: Pubkey,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub mint_lp: Pubkey,
    pub vault_x: Pubkey,
    pub vault_y: Pubkey,
    pub lp_vault: Pubkey,
    pub config: Pubkey,
}
