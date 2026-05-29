//! Test-only fixtures. Carries enough state to populate every bundle
//! in this crate from a `(Pool, UserAccounts)` pair, so integration
//! tests can build instructions in one `Bundle::from(...)` call.
//!
//! Lives in the program crate (rather than in each test's `common/`
//! module) because the per-ix bundle structs (`SwapBundle`,
//! `AddLiquidityBundle`, ...) live here, and `#[derive(BundleFrom)]`
//! needs the source-fixture types to be visible from the same crate
//! the bundle is declared in. Layering aside, that has a side benefit:
//! downstream programs that CPI into this AMM get the test fixtures
//! for free.
//!
//! Gated on `not(target_os = "solana")` + `feature = "test-helpers"`
//! so nothing here escapes into the on-chain BPF binary.

use anchor_lang::prelude::Pubkey;
use anchor_litesvm::{AliasMirror, Keypair, Signer};
use anchor_spl::associated_token::get_associated_token_address;

use crate::{CONFIG_SEED, LP_MINT_SEED};

/// All the pool-shared addresses a fixture needs, derived once from
/// `(program_id, seed, mint_x, mint_y)`.
///
/// `#[derive(AliasMirror)]` emits `Self::alias_all(&self, ctx)` that
/// registers every `Pubkey` field in the structured-log alias table
/// under the label given here. `mint_x` / `mint_y` are skipped because
/// the test harness already aliases them at scenario setup (they're
/// global, not pool-shared); aliasing them again would be redundant.
#[derive(Copy, Clone, Debug, AliasMirror)]
pub struct Pool {
    pub seed: u64,
    #[alias(skip)]
    pub mint_x: Pubkey,
    #[alias(skip)]
    pub mint_y: Pubkey,
    #[alias("MintLP")]
    pub mint_lp: Pubkey,
    #[alias("Pool")]
    pub config: Pubkey,
    #[alias("VaultX")]
    pub vault_x: Pubkey,
    #[alias("VaultY")]
    pub vault_y: Pubkey,
    #[alias("LpVault")]
    pub lp_vault: Pubkey,
}

impl Pool {
    /// Derive every pool-shared PDA / ATA from the inputs.
    pub fn derive(seed: u64, mint_x: Pubkey, mint_y: Pubkey) -> Self {
        let program_id = crate::ID;
        let (config, _) =
            Pubkey::find_program_address(&[CONFIG_SEED, &seed.to_le_bytes()], &program_id);
        let (mint_lp, _) =
            Pubkey::find_program_address(&[LP_MINT_SEED, config.as_ref()], &program_id);
        let vault_x = get_associated_token_address(&config, &mint_x);
        let vault_y = get_associated_token_address(&config, &mint_y);
        let lp_vault = get_associated_token_address(&config, &mint_lp);
        Self {
            seed,
            mint_x,
            mint_y,
            mint_lp,
            config,
            vault_x,
            vault_y,
            lp_vault,
        }
    }
}

/// A named participant in a scenario: a funded signer, the narrative
/// label that identifies them in the structured-log trace, and their
/// two token ATAs. The LP ATA is created lazily by `add_liquidity`
/// (init_if_needed), so it's not provisioned here.
///
/// One type covers every signer-role in the suite (LPs, traders,
/// admins, attackers). Where a "role" matters narratively, the
/// variable name and the label carry it ("alice" the LP vs "bob" the
/// trader vs "admin" the authority); the type stays uniform.
pub struct UserAccounts {
    pub signer: Keypair,
    pub label: String,
    pub ata_x: Pubkey,
    pub ata_y: Pubkey,
}

impl UserAccounts {
    pub fn pubkey(&self) -> Pubkey {
        self.signer.pubkey()
    }

    pub fn ata_lp(&self, mint_lp: &Pubkey) -> Pubkey {
        get_associated_token_address(&self.signer.pubkey(), mint_lp)
    }
}

