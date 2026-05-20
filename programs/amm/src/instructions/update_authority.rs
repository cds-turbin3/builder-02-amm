//! Admin: rotate or renounce the authority.
//!
//! Passing `None` for `new_authority` renounces, making the pool immutable.
//! Renouncing is a one-way operation: once `Config.authority == None`, no
//! admin instruction can succeed (every one returns `AuthorityRenounced`),
//! so the only way back is a new pool. See
//! [§Admin Instructions](../../../../docs/toy-amm.spec.md#admin-instructions).

use crate::constants::CONFIG_SEED;
use crate::error::AmmError;
use crate::state::Config;
use anchor_lang::prelude::*;

#[derive(Accounts)]
#[cfg_attr(
    feature = "test-helpers",
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(UpdateAuthorityBundle)
)]
pub struct UpdateAuthority<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [CONFIG_SEED, config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump,
    )]
    pub config: Account<'info, Config>,
}

impl UpdateAuthority<'_> {
    pub fn update_authority(&mut self, new_authority: Option<Pubkey>) -> Result<()> {
        let stored = self.config.authority.ok_or(AmmError::AuthorityRenounced)?;
        require_keys_eq!(self.authority.key(), stored, AmmError::Unauthorized);
        self.config.authority = new_authority;
        Ok(())
    }
}

#[cfg(feature = "test-helpers")]
#[derive(anchor_litesvm::Bundle, Copy, Clone)]
pub struct UpdateAuthorityBundle {
    pub authority: Pubkey,
    pub config: Pubkey,
}
