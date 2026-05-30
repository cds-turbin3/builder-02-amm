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
//!     .print_markdown_pair();
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
use anchor_litesvm::{
    ActorRegistry, AnchorContext, AnchorLiteSVM, Keypair, Pubkey, Signer, TestHelpers,
};
use anchor_spl::associated_token::get_associated_token_address;

// Pool and UserAccounts live in the program crate alongside the
// bundles (BundleFrom needs that), but tests import them from the
// usual `common::` path.
pub use amm::test_helpers::{Pool, UserAccounts};

// The scenario-level Markdown recorder and its rendering types now live in
// anchor-litesvm (shared across test crates); re-export so test files keep the
// familiar `common::{Report, MarkdownBlock}` import path. `print_markdown_pair()`
// documents one transaction; a `Report` documents one whole test.
pub use anchor_litesvm::{MarkdownBlock, Report, ToMarkdown};

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
    /// Deterministic-identity registry for this scenario. Derives every actor's
    /// keypair from its label (so addresses are stable across runs and reports)
    /// and enforces label uniqueness: a duplicate would silently alias two
    /// actors to one address. See [`anchor_litesvm::ActorRegistry`]. Private, so
    /// the only way to populate a `Scenario` is [`setup`] + the actor verbs.
    actors: ActorRegistry,
}

/// Namespace for all derived keypairs, like a Ganache mnemonic: change it and
/// every account in the suite moves in lockstep; keep it and they're stable
/// across runs forever. Bumping the `/vN` suffix is the deliberate "reshuffle".
/// The derivation itself (Ganache-style, label -> keypair) lives in
/// `anchor_litesvm::deterministic_keypair` / `ActorRegistry`.
const KEYPAIR_SEED_DOMAIN: &str = "amm-litesvm/v1";

/// Bootstrap a fresh `Scenario`: program loaded, two SPL Token mints
/// (decimals = 6) created, a `mint_authority` capable of minting either.
///
/// Every keypair here is deterministic (see [`ActorRegistry`]), so the full
/// address space (authority, both mints, and the pool PDAs derived from the
/// mints) is identical across runs. That's what lets the emitted reports be
/// committed and diffed without churn.
pub fn setup() -> Scenario {
    // `build_with_program` registers `"amm"` as the program's alias.
    let mut ctx = AnchorLiteSVM::build_with_program(amm::ID, "amm", AMM_BYTES);
    let actors = ActorRegistry::new(KEYPAIR_SEED_DOMAIN);

    // Authority and both mints are non-actor roles (not subject to actor-label
    // uniqueness), so derive them directly from the registry's domain. Seeding
    // the mints is what makes the pool PDAs (config, vaults, LP mint) stable.
    let mint_authority = actors.keypair("authority");
    ctx.svm.airdrop(&mint_authority.pubkey(), DEFAULT_SOL).unwrap();
    let mint_x_kp = actors.keypair("mint:x");
    let mint_y_kp = actors.keypair("mint:y");
    ctx.svm.create_token_mint_at(&mint_authority, &mint_x_kp, 6).unwrap();
    ctx.svm.create_token_mint_at(&mint_authority, &mint_y_kp, 6).unwrap();
    let (mint_x, mint_y) = (mint_x_kp.pubkey(), mint_y_kp.pubkey());
    ctx.alias(mint_x, "MintX").alias(mint_y, "MintY");
    Scenario {
        ctx,
        mint_authority,
        mint_x,
        mint_y,
        actors,
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
        // `create` derives the signer deterministically from the label (so
        // "Alice" is the same pubkey in every run and report) and panics on a
        // duplicate label, which would otherwise silently alias two actors to
        // one address and make every later assertion reason about the wrong
        // account. The "actor:" prefix namespaces actors apart from the mint /
        // authority roles that share the same registry domain.
        let signer = self.actors.create(&format!("actor:{label}"));
        self.ctx.svm.airdrop(&signer.pubkey(), sol).unwrap();
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

    /// Escape hatch: fetch a *second handle* to an actor that already exists,
    /// without minting, funding, or re-registering anything.
    ///
    /// [`user`](Self::user) treats a repeated label as a bug and panics, because
    /// a second *mint* at one label would fork a single identity into two funded
    /// actors and every later assertion would silently reason about the wrong
    /// one. But the rule it enforces ("don't reference a label twice") is the
    /// surface rule; the invariant underneath is "one label = one identity".
    /// Sometimes you legitimately want another `UserAccounts` pointing at that
    /// same identity (a helper that needs Alice's ATAs; an actor playing two
    /// narrative roles). That bends the surface rule while *upholding* the
    /// invariant, exactly the way [`std::mem::replace`] lets you move out of a
    /// borrowed place soundly: the escape is safe because the deeper guarantee
    /// still holds. (Compare Rust's own aliasing model: `user` is the unique
    /// `&mut` that creates; `actor` is a shared `&` to something already there,
    /// which is why it takes `&self`, not `&mut self`, and can coexist with
    /// other handles.)
    ///
    /// Re-derivation, not storage: the keypair comes back through the registry's
    /// `get`, which re-derives the same key
    /// used at creation; the ATAs are pure functions of `(owner, mint)`, so the
    /// returned handle is byte-for-byte the same identity with no on-chain
    /// effect. `get` panics if no actor with this label was created (re-fetching
    /// a nonexistent actor is the real mistake, and would hand back an unfunded
    /// ghost).
    pub fn actor(&self, label: &str) -> UserAccounts {
        let signer = self.actors.get(&format!("actor:{label}"));
        let owner = signer.pubkey();
        UserAccounts {
            signer,
            label: label.to_string(),
            ata_x: get_associated_token_address(&owner, &self.mint_x),
            ata_y: get_associated_token_address(&owner, &self.mint_y),
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
    //       .print_markdown_pair();

    /// One-shot: mint an "Admin" actor, derive a pool at `seed=0`, run
    /// `initialize` with the admin as both initializer and authority.
    /// `pool.alias_all` registers every Pubkey field in the alias table.
    pub fn fresh_pool(&mut self, fee_bps: u16) -> (UserAccounts, Pool) {
        self.fresh_pool_as("Admin", fee_bps)
    }

    /// [`fresh_pool`](Self::fresh_pool) with a caller-chosen label for the admin
    /// actor. "Admin" is a *role*, not a person; when a test rotates authority
    /// or refers to a "former admin", the bare role label leaves the trace
    /// ambiguous (the structured-log legend shows `Admin = <key>` with no hint
    /// of *which* actor). Naming the admin after the persona playing the role
    /// (e.g. `"Admin(Alice)"`) makes both the legend and the `signer=` frame
    /// self-identifying. The label still seeds the keypair, so it must be unique
    /// within the scenario like any other actor.
    pub fn fresh_pool_as(&mut self, admin_label: &str, fee_bps: u16) -> (UserAccounts, Pool) {
        let admin = self.cast(admin_label);
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
            .print_markdown_pair();
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
            .print_markdown_pair();
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
            .print_markdown_pair();
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
            .print_markdown_pair();
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
            .print_markdown_pair();
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
            .print_markdown_pair();
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
            .print_markdown_pair();
    }

    // -----------------------------------------------------------------
    // Observation: frozen, render-ready snapshots for `Report`
    // -----------------------------------------------------------------

    /// A frozen view of the pool's three token vaults at this instant.
    /// Labelled with the same names `Pool` carries via `#[alias(...)]`, so the
    /// report reads "VaultX" not a base58 key (readable AND run-stable).
    ///
    /// Takes `&mut self` because `token_balance` borrows the SVM mutably; the
    /// returned `Balances` owns its data and holds no borrow, so several
    /// `observe_*` calls compose fine in one `Report` chain.
    pub fn observe_pool(&mut self, pool: &Pool) -> Balances {
        Balances::new()
            .row("VaultX", self.ctx.svm.token_balance(&pool.vault_x))
            .row("VaultY", self.ctx.svm.token_balance(&pool.vault_y))
            .row("LpVault", self.ctx.svm.token_balance(&pool.lp_vault))
    }

    /// A frozen view of one actor's X / Y / LP balances. The LP ATA may not
    /// exist yet (it's created lazily by `add_liquidity`); `None` renders as
    /// `—`, distinct from a present-but-empty `Some(0)`.
    pub fn observe_user(&mut self, who: &UserAccounts, pool: &Pool) -> Balances {
        Balances::new()
            .row(format!("{} X", who.label), self.ctx.svm.token_balance(&who.ata_x))
            .row(format!("{} Y", who.label), self.ctx.svm.token_balance(&who.ata_y))
            .row(format!("{} LP", who.label), self.ctx.svm.token_balance(&who.ata_lp(&pool.mint_lp)))
    }

    /// A frozen view of the pool's `Config` account as a field/value table.
    ///
    /// `authority` renders as `set` / `renounced` rather than the raw pubkey:
    /// the key is deterministic now (seeded keypairs), but a base58 string is
    /// noise in a human-facing report, and *which* actor holds it is better said
    /// in prose (`md.note("authority rotated to BobAdmin")`) or pinned by a
    /// `check` on the exact pubkey. This snapshot is for the at-a-glance state.
    pub fn observe_config(&self, pool: &Pool) -> MarkdownBlock {
        let config: amm::Config = self.ctx.get_account(&pool.config).unwrap();
        MarkdownBlock::kv(
            ["field", "value"],
            [
                ("seed".to_string(), config.seed.to_string()),
                ("fee_bps".to_string(), config.fee_bps.to_string()),
                ("locked".to_string(), config.locked.to_string()),
                (
                    "authority".to_string(),
                    if config.authority.is_some() { "set" } else { "renounced" }.to_string(),
                ),
            ],
        )
    }
}

/// A frozen set of labelled token balances, ready to render as a Markdown
/// table. `None` means "the account doesn't exist", rendered `—`; `Some(0)`
/// means "exists, empty", rendered `0`. Keeping that distinction is the whole
/// point of carrying `Option<u64>` this far instead of flattening to `u64`.
pub struct Balances {
    rows: Vec<(String, Option<u64>)>,
}

impl Balances {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn row(mut self, label: impl Into<String>, balance: Option<u64>) -> Self {
        self.rows.push((label.into(), balance));
        self
    }
}

impl ToMarkdown for Balances {
    fn to_markdown(&self) -> MarkdownBlock {
        MarkdownBlock::kv(
            ["account", "balance"],
            self.rows
                .iter()
                .map(|(k, b)| (k.clone(), b.map_or_else(|| "—".into(), |v| v.to_string()))),
        )
    }
}
