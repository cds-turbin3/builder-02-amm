//! Shared scaffolding for the amm program's integration tests.
//!
//! Built around the actors-as-first-class-citizens pattern: a [`Scenario`]
//! owns the SVM context, the two mints, the mint authority, and the
//! structured-log alias table. [`UserAccounts`] is the actor type
//! (signer + label + the two token ATAs); a [`Pool`] fixture carries
//! the PDAs and vault ATAs that characterize a pool. Both `Pool` and
//! `UserAccounts` live in `amm::test_helpers` so the per-ix bundles can
//! `#[derive(BundleFrom)]` against them; re-exported here so test
//! files see the familiar import path.
//!
//! Verbs on `Scenario` (`cast`, `user`, `fresh_pool`, `initialize`,
//! `deposit`, `swap`, `remove_liquidity`, `set_locked`, `update_fee`,
//! `update_authority`) take typed actors and register every derived
//! account in the alias table as a side-effect of running, so the
//! structured log output stays narrative without per-test alias
//! plumbing.
//!
//! There is no `_expecting` companion for each verb anymore. The
//! [`AnchorContext::tx`](anchor_litesvm::AnchorContext::tx) chain
//! handles the build + send + assert in one statement, so negative-path
//! tests inline the chain at the call site:
//!
//! ```ignore
//! world.ctx
//!     .tx(&[&user.signer])
//!     .build(SwapBundle::from((&pool, &user)), instruction::Swap { kind, a_to_b: dir.a_to_b() })
//!     .send_err_named("PoolLocked")
//!     .print_logs_structured();
//! ```
//!
//! See `docs/testing/actors-as-first-class-citizens.md` for the
//! methodology and a worked example.

#![cfg(feature = "test-helpers")]
#![allow(dead_code)]

use amm::{
    AddLiquidityBundle, InitializeBundle, RemoveLiquidityBundle, SetLockedBundle, SwapBundle,
    SwapKind, UpdateAuthorityBundle, UpdateFeeBundle,
};
use anchor_litesvm::{AnchorContext, AnchorLiteSVM, Keypair, Pubkey, Signer, TestHelpers};

// Pool and UserAccounts live in the program crate alongside the
// bundles (BundleFrom needs that), but tests import them from the
// usual `common::` path.
pub use amm::test_helpers::{Pool, UserAccounts};

/// Compiled program bytes. Tests assume `cargo build-sbf -p amm` ran first;
/// the justfile / pre-commit wraps that.
const AMM_BYTES: &[u8] = include_bytes!("../../../../target/deploy/amm.so");

/// Default SOL allocation when minting an actor.
pub const DEFAULT_SOL: u64 = 10_000_000_000;

/// Swap direction at the test-API layer. The on-chain instruction takes
/// `a_to_b: bool`; that boolean is a mystery value at the call site, so
/// this enum is the surface tests use and `Scenario` verbs convert when
/// building the ix.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SwapDir {
    /// Spend mint X to receive mint Y.
    AtoB,
    /// Spend mint Y to receive mint X.
    BtoA,
}

impl SwapDir {
    pub fn a_to_b(self) -> bool {
        matches!(self, SwapDir::AtoB)
    }
}

/// The stage on which actors perform: owns the `AnchorContext`, the
/// two mints used by every test, and the mint authority that can mint
/// either.
pub struct Scenario {
    pub ctx: AnchorContext,
    pub mint_authority: Keypair,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
}

/// Bootstrap a fresh `Scenario`: program loaded, two SPL Token mints
/// (decimals = 6) created, a `mint_authority` capable of minting either.
pub fn setup() -> Scenario {
    let mut ctx = AnchorLiteSVM::build_with_program(amm::ID, AMM_BYTES);
    let mint_authority = ctx.svm.create_funded_account(DEFAULT_SOL).unwrap();
    let mint_x_kp = ctx.svm.create_token_mint(&mint_authority, 6).unwrap();
    let mint_y_kp = ctx.svm.create_token_mint(&mint_authority, 6).unwrap();
    let (mint_x, mint_y) = (mint_x_kp.pubkey(), mint_y_kp.pubkey());
    ctx.alias(amm::ID, "amm")
        .alias(mint_x, "MintX")
        .alias(mint_y, "MintY");
    Scenario {
        ctx,
        mint_authority,
        mint_x,
        mint_y,
    }
}

impl Scenario {
    // -----------------------------------------------------------------
    // Alias-table primitives
    // -----------------------------------------------------------------

    /// Register `pubkey -> label` in the context's alias table. Later
    /// inserts shadow earlier ones, so this also serves as a rename
    /// when an actor's role changes mid-test (e.g. authority rotation).
    pub fn alias(&mut self, pubkey: Pubkey, label: impl Into<String>) {
        self.ctx.alias(pubkey, label);
    }

    // -----------------------------------------------------------------
    // Cast construction
    // -----------------------------------------------------------------

    /// Mint a funded actor with zero token balances.
    pub fn cast(&mut self, label: &str) -> UserAccounts {
        self.user(label, 0, 0)
    }

    /// Mint a funded actor and pre-fund their X / Y balances.
    pub fn user(&mut self, label: &str, x_balance: u64, y_balance: u64) -> UserAccounts {
        self.user_with_sol(label, DEFAULT_SOL, x_balance, y_balance)
    }

    /// Variant of [`Self::user`] with an explicit SOL amount.
    pub fn user_with_sol(
        &mut self,
        label: &str,
        sol: u64,
        x_balance: u64,
        y_balance: u64,
    ) -> UserAccounts {
        let signer = self.ctx.svm.create_funded_account(sol).unwrap();
        let ata_x = self
            .ctx
            .svm
            .create_associated_token_account(&self.mint_x, &signer)
            .unwrap();
        let ata_y = self
            .ctx
            .svm
            .create_associated_token_account(&self.mint_y, &signer)
            .unwrap();
        if x_balance > 0 {
            self.ctx
                .svm
                .mint_to(&self.mint_x, &ata_x, &self.mint_authority, x_balance)
                .unwrap();
        }
        if y_balance > 0 {
            self.ctx
                .svm
                .mint_to(&self.mint_y, &ata_y, &self.mint_authority, y_balance)
                .unwrap();
        }
        self.alias(signer.pubkey(), label);
        UserAccounts {
            signer,
            label: label.to_string(),
            ata_x,
            ata_y,
        }
    }

    /// Mint additional X to `user`'s ATA.
    pub fn mint_to_x(&mut self, user: &UserAccounts, amount: u64) {
        self.ctx
            .svm
            .mint_to(&self.mint_x, &user.ata_x, &self.mint_authority, amount)
            .unwrap();
    }

    pub fn mint_to_y(&mut self, user: &UserAccounts, amount: u64) {
        self.ctx
            .svm
            .mint_to(&self.mint_y, &user.ata_y, &self.mint_authority, amount)
            .unwrap();
    }

    /// Mint directly into the pool's X vault, bypassing `add_liquidity`.
    /// Inflation-attack setup helper.
    pub fn mint_to_vault_x(&mut self, pool: &Pool, amount: u64) {
        self.ctx
            .svm
            .mint_to(&self.mint_x, &pool.vault_x, &self.mint_authority, amount)
            .unwrap();
    }

    pub fn mint_to_vault_y(&mut self, pool: &Pool, amount: u64) {
        self.ctx
            .svm
            .mint_to(&self.mint_y, &pool.vault_y, &self.mint_authority, amount)
            .unwrap();
    }

    // -----------------------------------------------------------------
    // Happy-path verbs (one Tx-chain per verb)
    // -----------------------------------------------------------------
    //
    // No `_expecting` companions: negative-path tests inline the chain
    // and swap the terminator, e.g.
    //
    //   world.ctx.tx(&[&user.signer])
    //       .build(SwapBundle::from((&pool, &user)),
    //              amm::instruction::Swap { kind, a_to_b: dir.a_to_b() })
    //       .send_err_named("PoolLocked")
    //       .print_logs_structured();

    /// One-shot: mint an "Admin" actor, derive a pool at `seed=0`, run
    /// `initialize` with the admin as both initializer and authority.
    /// `pool.alias_all` registers every Pubkey field in the alias table.
    pub fn fresh_pool(&mut self, fee_bps: u16) -> (UserAccounts, Pool) {
        let admin = self.cast("Admin");
        let pool = Pool::derive(0, self.mint_x, self.mint_y);
        pool.alias_all(&mut self.ctx);
        self.initialize(&admin, &pool, fee_bps, Some(&admin));
        (admin, pool)
    }

    /// Lower-level `initialize`: caller chooses seed, fee_bps, authority.
    pub fn initialize(
        &mut self,
        initializer: &UserAccounts,
        pool: &Pool,
        fee_bps: u16,
        authority: Option<&UserAccounts>,
    ) {
        self.ctx
            .tx(&[&initializer.signer])
            .build(
                InitializeBundle {
                    initializer: initializer.pubkey(),
                    mint_x: pool.mint_x,
                    mint_y: pool.mint_y,
                    mint_lp: pool.mint_lp,
                    vault_x: pool.vault_x,
                    vault_y: pool.vault_y,
                    lp_vault: pool.lp_vault,
                    config: pool.config,
                },
                amm::instruction::Initialize {
                    seed: pool.seed,
                    fee_bps,
                    authority: authority.map(|a| a.pubkey()),
                },
            )
            .send_ok()
            .print_logs_structured();
    }

    pub fn deposit(
        &mut self,
        user: &UserAccounts,
        pool: &Pool,
        amount_a: u64,
        amount_b: u64,
        min_lp_tokens: u64,
    ) {
        self.ctx
            .tx(&[&user.signer])
            .build(
                AddLiquidityBundle::from((pool, user)),
                amm::instruction::AddLiquidity {
                    amount_a,
                    amount_b,
                    min_lp_tokens,
                },
            )
            .send_ok()
            .print_logs_structured();
    }

    pub fn remove_liquidity(
        &mut self,
        user: &UserAccounts,
        pool: &Pool,
        lp_burn: u64,
        min_a: u64,
        min_b: u64,
    ) {
        self.ctx
            .tx(&[&user.signer])
            .build(
                RemoveLiquidityBundle::from((pool, user)),
                amm::instruction::RemoveLiquidity {
                    lp_burn,
                    min_a,
                    min_b,
                },
            )
            .send_ok()
            .print_logs_structured();
    }

    pub fn swap(&mut self, user: &UserAccounts, pool: &Pool, kind: SwapKind, dir: SwapDir) {
        self.ctx
            .tx(&[&user.signer])
            .build(
                SwapBundle::from((pool, user)),
                amm::instruction::Swap {
                    kind,
                    a_to_b: dir.a_to_b(),
                },
            )
            .send_ok()
            .print_logs_structured();
    }

    pub fn set_locked(&mut self, admin: &UserAccounts, pool: &Pool, locked: bool) {
        self.ctx
            .tx(&[&admin.signer])
            .build(
                SetLockedBundle {
                    authority: admin.pubkey(),
                    config: pool.config,
                },
                amm::instruction::SetLocked { locked },
            )
            .send_ok()
            .print_logs_structured();
    }

    pub fn update_fee(&mut self, admin: &UserAccounts, pool: &Pool, new_fee_bps: u16) {
        self.ctx
            .tx(&[&admin.signer])
            .build(
                UpdateFeeBundle {
                    authority: admin.pubkey(),
                    config: pool.config,
                },
                amm::instruction::UpdateFee { new_fee_bps },
            )
            .send_ok()
            .print_logs_structured();
    }

    /// `Some(&new_admin)` rotates; `None` renounces.
    pub fn update_authority(
        &mut self,
        admin: &UserAccounts,
        pool: &Pool,
        new_authority: Option<&UserAccounts>,
    ) {
        self.ctx
            .tx(&[&admin.signer])
            .build(
                UpdateAuthorityBundle {
                    authority: admin.pubkey(),
                    config: pool.config,
                },
                amm::instruction::UpdateAuthority {
                    new_authority: new_authority.map(|a| a.pubkey()),
                },
            )
            .send_ok()
            .print_logs_structured();
    }
}
