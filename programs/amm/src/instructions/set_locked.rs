//! Admin: freeze or unfreeze the pool.
//!
//! When `Config.locked == true`, the three trade instructions (`swap`,
//! `add_liquidity`, `remove_liquidity`) return [`AmmError::PoolLocked`].
//! Admin instructions are unaffected: an authority can adjust fees and rotate
//! authority even while the pool is locked. See
//! [§Admin Instructions](../../../../docs/toy-amm.spec.md#admin-instructions).

use crate::constants::CONFIG_SEED;
use crate::error::AmmError;
use crate::state::Config;
use anchor_lang::prelude::*;

#[derive(Accounts)]
#[cfg_attr(
    feature = "test-helpers",
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(SetLockedBundle)
)]
pub struct SetLocked<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [CONFIG_SEED, config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump,
    )]
    pub config: Account<'info, Config>,
}

impl SetLocked<'_> {
    pub fn set_locked(&mut self, locked: bool) -> Result<()> {
        let stored = self.config.authority.ok_or(AmmError::AuthorityRenounced)?;
        require_keys_eq!(self.authority.key(), stored, AmmError::Unauthorized);
        self.config.locked = locked;
        Ok(())
    }
}

#[cfg(feature = "test-helpers")]
#[derive(anchor_litesvm::Bundle, Copy, Clone)]
pub struct SetLockedBundle {
    pub authority: Pubkey,
    pub config: Pubkey,
}
