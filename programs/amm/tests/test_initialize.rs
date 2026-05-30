//! Initialize a pool and assert the resulting on-chain state.
//!
//! Cast: one actor (`admin`) playing initializer and authority.
//! Subjects: none; the (mint_x, mint_y) pair lives on the `Scenario`,
//! not as a per-test subject.
//!
//! Each test threads a [`Report`]: it narrates intent and snapshots the
//! resulting Config + vault state, and its `check`s double as the assertions.
//! The Markdown lands in `target/md-reports/<slug>.md`.
//!
//! See `docs/testing/actors-as-first-class-citizens.md` for the methodology.

#![cfg(feature = "test-helpers")]

mod common;

use amm::Config;
use anchor_litesvm::TestHelpers;
use common::{setup, MarkdownBlock, Pool, Report};

#[test]
fn initialize_creates_config_lp_mint_and_vaults() {
    let mut md = Report::new(
        "Initialize: creates Config, LP mint, and both vaults",
        "Initializing a pool writes the args verbatim into Config (unlocked), \
         and creates the LP mint plus the X / Y reserve vaults and the lock \
         vault, all at zero balance.",
    );

    let mut world = setup();
    let admin = world.cast("Admin");
    let pool = Pool::derive(0, world.mint_x, world.mint_y);
    world.alias(pool.config, "Pool");
    world.alias(pool.mint_lp, "MintLP");

    let fee_bps: u16 = 30;
    md.step("Action: admin initializes pool at seed 0 with fee_bps = 30");
    world.initialize(&admin, &pool, fee_bps, Some(&admin));

    md.step("After: Config carries the args and starts unlocked");
    md.block("config", world.observe_config(&pool));

    // Config carries the args verbatim and starts unlocked.
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("seed", 0, config.seed);
    md.check("fee_bps", fee_bps, config.fee_bps);
    md.check("authority is admin", Some(admin.pubkey()), config.authority);
    md.check("mint_x recorded", pool.mint_x, config.mint_x);
    md.check("mint_y recorded", pool.mint_y, config.mint_y);
    md.check("starts unlocked", false, config.locked);

    md.step("After: LP mint, both reserve vaults, and the lock vault exist at zero");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.check("vault_x exists, empty", Some(0), world.ctx.svm.token_balance(&pool.vault_x));
    md.check("vault_y exists, empty", Some(0), world.ctx.svm.token_balance(&pool.vault_y));
    md.check("lp_vault exists, empty", Some(0), world.ctx.svm.token_balance(&pool.lp_vault));
}

/// `fee_bps >= FEE_DENOMINATOR (10_000)` is rejected at init. The handler's
/// `require!((fee_bps as u64) < FEE_DENOMINATOR, AmmError::InvalidFee)` line
/// is the boundary; this test pins the boundary at exactly 10_000 (rejected)
/// and proves a fee that high never reaches Config storage.
#[test]
fn initialize_rejects_invalid_fee_at_denominator() {
    let mut md = Report::new(
        "Initialize: rejects fee_bps at the denominator boundary",
        "fee_bps must be strictly less than FEE_DENOMINATOR (10_000). Passing \
         exactly 10_000 must reject with InvalidFee, and Config must never be \
         created.",
    );

    let mut world = setup();
    let admin = world.cast("Admin");
    let pool = Pool::derive(0, world.mint_x, world.mint_y);

    md.step("Action: initialize with fee_bps = 10_000 (the boundary)");
    let rejection = world
        .ctx
        .tx(&[&admin.signer])
        .build(
            amm::InitializeBundle {
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
                fee_bps: 10_000,
                authority: Some(admin.pubkey()),
            },
        )
        .send_err_named("InvalidFee");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: Config was never created");
    md.check("config account absent", false, world.ctx.account_exists(&pool.config));
}
