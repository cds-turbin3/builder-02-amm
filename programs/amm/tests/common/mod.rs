//! Shared scaffolding for the amm program's integration tests.
//!
//! Built around the actors-as-first-class-citizens pattern: a [`Scenario`]
//! owns the SVM context, the two mints, the mint authority, and the
//! structured-log alias table. [`UserAccounts`] is the actor type
//! (signer + label + the two token ATAs); a [`Pool`] fixture carries the
//! PDAs and vault ATAs that characterize a pool. Verbs on `Scenario`
//! (`cast`, `user`, `fresh_pool`, `deposit`, `swap`, `remove_liquidity`,
//! `set_locked`, `update_fee`, `update_authority`) take typed actors and
//! register every derived account in the alias table as a side-effect of
//! running, so the structured log output stays narrative without per-test
//! alias plumbing.
//!
//! Each verb has an `_expecting(..., error)` companion for negative-path
//! tests. The error string is matched as a substring against both the
//! transaction logs and the error field (same matcher `send_err_named`
//! uses), so one signature accepts Anchor error names like `"PoolLocked"`
//! and System messages like `"already in use"`.
//!
//! See `docs/testing/actors-as-first-class-citizens.md` for the
//! methodology and a worked example.

// Each integration-test binary compiles `common` as its own module and
// runs the dead-code lint against just the items *that binary* uses. A
// verb used in `test_swap.rs` but not in `test_initialize.rs` shows up
// as dead from the latter's perspective. The blanket allow is the
// conventional handling for shared test scaffolding.
#![cfg(feature = "test-helpers")]
#![allow(dead_code)]

use amm::{
    AddLiquidityBundle, InitializeBundle, RemoveLiquidityBundle, SetLockedBundle, SwapBundle,
    SwapKind, UpdateAuthorityBundle, UpdateFeeBundle, CONFIG_SEED, LP_MINT_SEED,
};
use anchor_litesvm::{
    AnchorContext, AnchorLiteSVM, Instruction, Keypair, Pubkey, Signer, TestHelpers,
    TransactionResult,
};
use anchor_spl::associated_token::get_associated_token_address;

/// Compiled program bytes. Tests assume `cargo build-sbf -p amm` ran first;
/// the justfile / pre-commit wraps that.
const AMM_BYTES: &[u8] = include_bytes!("../../../../target/deploy/amm.so");

/// Default SOL allocation when minting an actor. Most tests don't care
/// about the exact amount; this is "enough to pay rent and fees for any
/// reasonable scenario". Override via [`Scenario::cast_with_sol`] if a
/// scenario deliberately probes the lamports-side.
pub const DEFAULT_SOL: u64 = 10_000_000_000;

/// Swap direction at the test-API layer. The on-chain instruction takes
/// `a_to_b: bool`; that boolean is a mystery value at the call site
/// (`world.swap(&bob, &pool, kind, true)` doesn't tell a reader which
/// direction the trade goes). This enum is the surface tests use; the
/// `Scenario` verbs convert to the bool when building the ix.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SwapDir {
    /// Spend mint X to receive mint Y.
    AtoB,
    /// Spend mint Y to receive mint X.
    BtoA,
}

impl SwapDir {
    fn a_to_b(self) -> bool {
        matches!(self, SwapDir::AtoB)
    }
}

/// All the pool-shared addresses a fixture needs, derived once from
/// `(program_id, seed, mint_x, mint_y)`.
#[derive(Copy, Clone, Debug)]
pub struct Pool {
    pub seed: u64,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub mint_lp: Pubkey,
    pub config: Pubkey,
    pub vault_x: Pubkey,
    pub vault_y: Pubkey,
    pub lp_vault: Pubkey,
}

impl Pool {
    pub fn derive(seed: u64, mint_x: Pubkey, mint_y: Pubkey) -> Self {
        let program_id = amm::ID;
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

    pub fn swap_bundle(&self, user: &UserAccounts) -> SwapBundle {
        SwapBundle {
            user: user.pubkey(),
            mint_x: self.mint_x,
            mint_y: self.mint_y,
            config: self.config,
            vault_x: self.vault_x,
            vault_y: self.vault_y,
            user_x: user.ata_x,
            user_y: user.ata_y,
        }
    }

    pub fn add_liquidity_bundle(&self, user: &UserAccounts) -> AddLiquidityBundle {
        AddLiquidityBundle {
            user: user.pubkey(),
            mint_x: self.mint_x,
            mint_y: self.mint_y,
            config: self.config,
            mint_lp: self.mint_lp,
            vault_x: self.vault_x,
            vault_y: self.vault_y,
            lp_vault: self.lp_vault,
            user_x: user.ata_x,
            user_y: user.ata_y,
            user_lp: user.ata_lp(&self.mint_lp),
        }
    }

    pub fn remove_liquidity_bundle(&self, user: &UserAccounts) -> RemoveLiquidityBundle {
        RemoveLiquidityBundle {
            user: user.pubkey(),
            mint_x: self.mint_x,
            mint_y: self.mint_y,
            config: self.config,
            mint_lp: self.mint_lp,
            vault_x: self.vault_x,
            vault_y: self.vault_y,
            user_x: user.ata_x,
            user_y: user.ata_y,
            user_lp: user.ata_lp(&self.mint_lp),
        }
    }
}

/// A named participant in a scenario: a funded signer, the narrative
/// label that identifies them in the structured-log trace, and their
/// two token ATAs. The LP ATA is created lazily by `add_liquidity`
/// (init_if_needed), so it's not provisioned here.
///
/// One type covers every signer-role in the suite (LPs, traders,
/// admins, attackers). The cast analysis showed that splitting by role
/// would force every verb to decide which input to accept; the savings
/// are negative. Where a "role" matters narratively, the variable name
/// and the label carry it ("alice" the LP vs "bob" the trader vs
/// "admin" the authority); the type stays uniform.
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

/// The stage on which actors perform: owns the `AnchorContext`, the
/// two mints used by every test, and the mint authority that can mint
/// either. The structured-log alias table lives on `ctx.aliases`;
/// `Scenario::alias` delegates to it so verbs read as `world.alias(...)`
/// in tests.
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
    /// Thin delegator over [`AnchorContext::alias`].
    pub fn alias(&mut self, pubkey: Pubkey, label: impl Into<String>) {
        self.ctx.alias(pubkey, label);
    }

    // -----------------------------------------------------------------
    // Cast construction
    // -----------------------------------------------------------------

    /// Mint a funded actor with zero token balances. Used for cast
    /// members who don't transact tokens directly (attackers, auth-only
    /// admins, strangers in negative-path tests). The actor still has
    /// ATAs created (cheap), so they can be promoted to a trader later
    /// via [`Self::mint_to_x`] / [`Self::mint_to_y`].
    pub fn cast(&mut self, label: &str) -> UserAccounts {
        self.user(label, 0, 0)
    }

    /// Mint a funded actor and pre-fund their X / Y balances. The label
    /// identifies them in every structured-log frame they sign.
    pub fn user(&mut self, label: &str, x_balance: u64, y_balance: u64) -> UserAccounts {
        self.user_with_sol(label, DEFAULT_SOL, x_balance, y_balance)
    }

    /// Variant of [`Self::user`] that takes an explicit SOL amount.
    /// Used by scenarios that deliberately probe the lamports-side
    /// (fee accounting, rent edge cases).
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

    /// Mint additional X to `user`'s ATA. Used by tests that promote a
    /// previously-balanceless actor (typically an admin from
    /// `fresh_pool`) into a trader for the duration of one scenario.
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
    /// This is the inflation-attack helper: in production an attacker
    /// would `Token::Transfer` from their ATA into the vault, but the
    /// math is the same and minting is cheaper to set up in a test.
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
    // Happy-path verbs
    // -----------------------------------------------------------------

    /// One-shot: mint an "Admin" actor, derive a pool at `seed=0`, run
    /// `initialize` with the admin as both initializer and authority.
    /// Registers all the pool's PDAs / vaults in the alias table.
    pub fn fresh_pool(&mut self, fee_bps: u16) -> (UserAccounts, Pool) {
        let admin = self.cast("Admin");
        let pool = Pool::derive(0, self.mint_x, self.mint_y);
        self.alias(pool.config, "Pool");
        self.alias(pool.mint_lp, "MintLP");
        self.alias(pool.vault_x, "VaultX");
        self.alias(pool.vault_y, "VaultY");
        self.alias(pool.lp_vault, "LpVault");
        self.initialize(&admin, &pool, fee_bps, Some(&admin));
        (admin, pool)
    }

    /// Lower-level `initialize`: the caller chooses the seed, fee_bps,
    /// and authority. Used when [`Self::fresh_pool`] is too coarse
    /// (e.g. testing the fee-boundary check). The pool fixture must be
    /// pre-derived; the verb registers its PDAs in the alias table.
    pub fn initialize(
        &mut self,
        initializer: &UserAccounts,
        pool: &Pool,
        fee_bps: u16,
        authority: Option<&UserAccounts>,
    ) {
        let ix = self.ctx.program().build_ix(
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
        );
        self.ctx
            .send_ok(ix, &[&initializer.signer])
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
        let ix = self.build_deposit_ix(user, pool, amount_a, amount_b, min_lp_tokens);
        self.ctx
            .send_ok(ix, &[&user.signer])
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
        let ix = self.build_remove_liquidity_ix(user, pool, lp_burn, min_a, min_b);
        self.ctx
            .send_ok(ix, &[&user.signer])
            .print_logs_structured();
    }

    pub fn swap(&mut self, user: &UserAccounts, pool: &Pool, kind: SwapKind, dir: SwapDir) {
        let ix = self.build_swap_ix(user, pool, kind, dir);
        self.ctx
            .send_ok(ix, &[&user.signer])
            .print_logs_structured();
    }

    pub fn set_locked(&mut self, admin: &UserAccounts, pool: &Pool, locked: bool) {
        let ix = self.build_set_locked_ix(admin, pool, locked);
        self.ctx
            .send_ok(ix, &[&admin.signer])
            .print_logs_structured();
    }

    pub fn update_fee(&mut self, admin: &UserAccounts, pool: &Pool, new_fee_bps: u16) {
        let ix = self.build_update_fee_ix(admin, pool, new_fee_bps);
        self.ctx
            .send_ok(ix, &[&admin.signer])
            .print_logs_structured();
    }

    /// Run `update_authority`. Pass `Some(&new_admin)` to rotate or
    /// `None` to renounce.
    pub fn update_authority(
        &mut self,
        admin: &UserAccounts,
        pool: &Pool,
        new_authority: Option<&UserAccounts>,
    ) {
        let ix = self.build_update_authority_ix(admin, pool, new_authority);
        self.ctx
            .send_ok(ix, &[&admin.signer])
            .print_logs_structured();
    }

    // -----------------------------------------------------------------
    // Negative-path verbs
    // -----------------------------------------------------------------
    //
    // Each takes the same parameters as its happy-path companion plus
    // an `error` substring. The matcher checks both transaction logs
    // and the error field, so it accepts Anchor names (`"PoolLocked"`,
    // `"SlippageExceeded"`) and System messages with one signature.

    pub fn initialize_expecting(
        &mut self,
        initializer: &UserAccounts,
        pool: &Pool,
        fee_bps: u16,
        authority: Option<&UserAccounts>,
        error: &str,
    ) -> TransactionResult {
        let ix = self.ctx.program().build_ix(
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
        );
        self.ctx
            .send_err_named(ix, &[&initializer.signer], error)
            .print_logs_structured()
    }

    pub fn deposit_expecting(
        &mut self,
        user: &UserAccounts,
        pool: &Pool,
        amount_a: u64,
        amount_b: u64,
        min_lp_tokens: u64,
        error: &str,
    ) -> TransactionResult {
        let ix = self.build_deposit_ix(user, pool, amount_a, amount_b, min_lp_tokens);
        self.ctx
            .send_err_named(ix, &[&user.signer], error)
            .print_logs_structured()
    }

    pub fn remove_liquidity_expecting(
        &mut self,
        user: &UserAccounts,
        pool: &Pool,
        lp_burn: u64,
        min_a: u64,
        min_b: u64,
        error: &str,
    ) -> TransactionResult {
        let ix = self.build_remove_liquidity_ix(user, pool, lp_burn, min_a, min_b);
        self.ctx
            .send_err_named(ix, &[&user.signer], error)
            .print_logs_structured()
    }

    pub fn swap_expecting(
        &mut self,
        user: &UserAccounts,
        pool: &Pool,
        kind: SwapKind,
        dir: SwapDir,
        error: &str,
    ) -> TransactionResult {
        let ix = self.build_swap_ix(user, pool, kind, dir);
        self.ctx
            .send_err_named(ix, &[&user.signer], error)
            .print_logs_structured()
    }

    pub fn set_locked_expecting(
        &mut self,
        admin: &UserAccounts,
        pool: &Pool,
        locked: bool,
        error: &str,
    ) -> TransactionResult {
        let ix = self.build_set_locked_ix(admin, pool, locked);
        self.ctx
            .send_err_named(ix, &[&admin.signer], error)
            .print_logs_structured()
    }

    pub fn update_fee_expecting(
        &mut self,
        admin: &UserAccounts,
        pool: &Pool,
        new_fee_bps: u16,
        error: &str,
    ) -> TransactionResult {
        let ix = self.build_update_fee_ix(admin, pool, new_fee_bps);
        self.ctx
            .send_err_named(ix, &[&admin.signer], error)
            .print_logs_structured()
    }

    pub fn update_authority_expecting(
        &mut self,
        admin: &UserAccounts,
        pool: &Pool,
        new_authority: Option<&UserAccounts>,
        error: &str,
    ) -> TransactionResult {
        let ix = self.build_update_authority_ix(admin, pool, new_authority);
        self.ctx
            .send_err_named(ix, &[&admin.signer], error)
            .print_logs_structured()
    }

    // -----------------------------------------------------------------
    // Instruction builders (private; shared by happy and negative verbs)
    // -----------------------------------------------------------------

    fn build_deposit_ix(
        &self,
        user: &UserAccounts,
        pool: &Pool,
        amount_a: u64,
        amount_b: u64,
        min_lp_tokens: u64,
    ) -> Instruction {
        self.ctx.program().build_ix(
            pool.add_liquidity_bundle(user),
            amm::instruction::AddLiquidity {
                amount_a,
                amount_b,
                min_lp_tokens,
            },
        )
    }

    fn build_remove_liquidity_ix(
        &self,
        user: &UserAccounts,
        pool: &Pool,
        lp_burn: u64,
        min_a: u64,
        min_b: u64,
    ) -> Instruction {
        self.ctx.program().build_ix(
            pool.remove_liquidity_bundle(user),
            amm::instruction::RemoveLiquidity {
                lp_burn,
                min_a,
                min_b,
            },
        )
    }

    fn build_swap_ix(
        &self,
        user: &UserAccounts,
        pool: &Pool,
        kind: SwapKind,
        dir: SwapDir,
    ) -> Instruction {
        self.ctx.program().build_ix(
            pool.swap_bundle(user),
            amm::instruction::Swap {
                kind,
                a_to_b: dir.a_to_b(),
            },
        )
    }

    fn build_set_locked_ix(&self, admin: &UserAccounts, pool: &Pool, locked: bool) -> Instruction {
        self.ctx.program().build_ix(
            SetLockedBundle {
                authority: admin.pubkey(),
                config: pool.config,
            },
            amm::instruction::SetLocked { locked },
        )
    }

    fn build_update_fee_ix(
        &self,
        admin: &UserAccounts,
        pool: &Pool,
        new_fee_bps: u16,
    ) -> Instruction {
        self.ctx.program().build_ix(
            UpdateFeeBundle {
                authority: admin.pubkey(),
                config: pool.config,
            },
            amm::instruction::UpdateFee { new_fee_bps },
        )
    }

    fn build_update_authority_ix(
        &self,
        admin: &UserAccounts,
        pool: &Pool,
        new_authority: Option<&UserAccounts>,
    ) -> Instruction {
        self.ctx.program().build_ix(
            UpdateAuthorityBundle {
                authority: admin.pubkey(),
                config: pool.config,
            },
            amm::instruction::UpdateAuthority {
                new_authority: new_authority.map(|a| a.pubkey()),
            },
        )
    }
}
