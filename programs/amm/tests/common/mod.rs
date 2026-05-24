//! Shared fixtures for amm integration tests.
//!
//! The shape follows the bundle-as-actors framing (see `feedback_bundles_as_actors`):
//! a [`Pool`] fixture carries the PDAs / ATAs that *characterize a pool*
//! (program-derived state shared by every instruction), while per-test actor
//! keypairs and their user-side ATAs are constructed inline in each test to
//! keep the scenario narrative legible.

#![cfg(feature = "test-helpers")]
#![allow(dead_code)]

use amm::{AddLiquidityBundle, InitializeBundle, RemoveLiquidityBundle, SetLockedBundle, SwapBundle, CONFIG_SEED, LP_MINT_SEED};
use anchor_litesvm::{
    Aliases, AnchorContext, AnchorLiteSVM, Keypair, Pubkey, Signer, TestHelpers, TransactionHelpers,
};
use anchor_spl::associated_token::get_associated_token_address;

/// Compiled program bytes. Tests assume `cargo build-sbf -p amm` ran first;
/// the justfile / pre-commit wraps that.
const AMM_BYTES: &[u8] = include_bytes!("../../../../target/deploy/amm.so");

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

/// Per-user account bundle: a funded signer plus their two token ATAs.
/// The LP ATA is created lazily by `add_liquidity` (init_if_needed), so it's
/// not provisioned here.
pub struct UserAccounts {
    pub signer: Keypair,
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

/// Bootstrap a test world: program loaded, two SPL Token mints created
/// (decimals = 6), a `mint_authority` capable of minting either.
pub fn setup() -> Bootstrap {
    let mut ctx = AnchorLiteSVM::build_with_program(amm::ID, AMM_BYTES);
    let mint_authority = ctx.svm.create_funded_account(10_000_000_000).unwrap();
    let mint_x_kp = ctx.svm.create_token_mint(&mint_authority, 6).unwrap();
    let mint_y_kp = ctx.svm.create_token_mint(&mint_authority, 6).unwrap();
    Bootstrap {
        ctx,
        mint_authority,
        mint_x: mint_x_kp.pubkey(),
        mint_y: mint_y_kp.pubkey(),
        // Pre-seed `amm::ID` so structured logs render program frames as
        // `amm::Swap` instead of `CYbYnHW7…2yf5::Swap`.
        aliases: Aliases::default().with(amm::ID, "amm"),
    }
}

pub struct Bootstrap {
    pub ctx: AnchorContext,
    pub mint_authority: Keypair,
    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    /// Accumulates over the test as actors and pool components are created,
    /// so structured-log output renders pubkeys as the names the test author
    /// reasoned about (Bob, Pool, VaultX...) instead of raw base58.
    pub aliases: Aliases,
}

impl Bootstrap {
    /// Register `pubkey -> name` in the alias table. Later inserts shadow
    /// earlier ones, so this also serves as a rename when an actor's role
    /// changes mid-test (e.g. authority rotation).
    pub fn alias(&mut self, pubkey: Pubkey, name: impl Into<String>) {
        // `Aliases::with` is a consuming builder; mem::take lets us update
        // the field in place without requiring a clone.
        let aliases = std::mem::take(&mut self.aliases);
        self.aliases = aliases.with(pubkey, name);
    }

    /// Create a funded user with x/y ATAs and pre-mint them token balances,
    /// registering the signer pubkey under `name` for log readability.
    pub fn make_user(
        &mut self,
        name: &str,
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
        self.alias(signer.pubkey(), name);
        UserAccounts {
            signer,
            ata_x,
            ata_y,
        }
    }

    /// One-shot helper: derive a pool at `seed=0` and run `initialize` with
    /// `admin` as both initializer and authority. Returns the admin keypair
    /// (signer for future admin instructions) and the pool fixture. The
    /// admin signer is aliased as "Admin" and the pool's PDAs / vaults are
    /// registered too; tests that need a different admin name override via
    /// `world.alias(admin.pubkey(), "...")`.
    pub fn fresh_pool(&mut self, fee_bps: u16) -> (Keypair, Pool) {
        let admin = self.ctx.svm.create_funded_account(10_000_000_000).unwrap();
        let pool = Pool::derive(0, self.mint_x, self.mint_y);
        self.alias(admin.pubkey(), "Admin");
        self.alias(pool.config, "Pool");
        self.alias(pool.mint_lp, "MintLP");
        self.alias(pool.vault_x, "VaultX");
        self.alias(pool.vault_y, "VaultY");
        self.alias(pool.lp_vault, "LpVault");
        self.alias(self.mint_x, "MintX");
        self.alias(self.mint_y, "MintY");
        let ix = self.ctx.program().build_ix(
            InitializeBundle {
                initializer: admin.pubkey(),
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
                authority: Some(admin.pubkey()),
            },
        );
        self.ctx
            .svm
            .send_ok(ix, &[&admin], &self.aliases)
            .print_logs_structured(&self.aliases);
        (admin, pool)
    }

    /// Flip `Config.locked` on the pool. Used by negative-path tests that
    /// want to observe `PoolLocked` errors on trade-path instructions.
    pub fn set_locked(&mut self, admin: &Keypair, pool: &Pool, locked: bool) {
        let ix = self.ctx.program().build_ix(
            SetLockedBundle {
                authority: admin.pubkey(),
                config: pool.config,
            },
            amm::instruction::SetLocked { locked },
        );
        self.ctx
            .svm
            .send_ok(ix, &[admin], &self.aliases)
            .print_logs_structured(&self.aliases);
    }

    /// Run `add_liquidity` on behalf of `user`. Used when a test needs an
    /// existing-pool state set up before exercising another instruction.
    pub fn deposit(
        &mut self,
        pool: &Pool,
        user: &UserAccounts,
        amount_a: u64,
        amount_b: u64,
        min_lp_tokens: u64,
    ) {
        let ix = self.ctx.program().build_ix(
            AddLiquidityBundle {
                user: user.pubkey(),
                mint_x: pool.mint_x,
                mint_y: pool.mint_y,
                config: pool.config,
                mint_lp: pool.mint_lp,
                vault_x: pool.vault_x,
                vault_y: pool.vault_y,
                lp_vault: pool.lp_vault,
                user_x: user.ata_x,
                user_y: user.ata_y,
                user_lp: user.ata_lp(&pool.mint_lp),
            },
            amm::instruction::AddLiquidity {
                amount_a,
                amount_b,
                min_lp_tokens,
            },
        );
        self.ctx
            .svm
            .send_ok(ix, &[&user.signer], &self.aliases)
            .print_logs_structured(&self.aliases);
    }
}
