//! Admin: rotate the swap fee.
//!
//! Constraints: signer must equal `Config.authority`; the new fee must satisfy
//! `new_fee_bps < amm_math::FEE_DENOMINATOR`. See
//! [§Admin Instructions](../../../../docs/toy-amm.spec.md#admin-instructions).

use crate::constants::CONFIG_SEED;
use crate::error::AmmError;
use crate::state::Config;
use amm_math::FEE_DENOMINATOR;
use anchor_lang::prelude::*;

#[derive(Accounts)]
#[cfg_attr(
    feature = "test-helpers",
    derive(anchor_litesvm::BundledPubkeys),
    bundled_with(UpdateFeeBundle)
)]
pub struct UpdateFee<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [CONFIG_SEED, config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump,
    )]
    pub config: Account<'info, Config>,
}

impl UpdateFee<'_> {
    pub fn update_fee(&mut self, new_fee_bps: u16) -> Result<()> {
        // `Option<Pubkey>` precludes Anchor's `has_one = authority`, so the
        // authority match is enforced here.
        let stored = self.config.authority.ok_or(AmmError::AuthorityRenounced)?;
        require_keys_eq!(self.authority.key(), stored, AmmError::Unauthorized);
        require!((new_fee_bps as u64) < FEE_DENOMINATOR, AmmError::InvalidFee);
        self.config.fee_bps = new_fee_bps;
        Ok(())
    }
}

#[cfg(feature = "test-helpers")]
#[derive(anchor_litesvm::Bundle, Copy, Clone)]
pub struct UpdateFeeBundle {
    pub authority: Pubkey,
    pub config: Pubkey,
}
