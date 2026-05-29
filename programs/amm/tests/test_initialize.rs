//! Initialize a pool and assert the resulting on-chain state.
//!
//! Cast: one actor (`admin`) playing initializer and authority.
//! Subjects: none; the (mint_x, mint_y) pair lives on the `Scenario`,
//! not as a per-test subject.
//!
//! See `docs/testing/actors-as-first-class-citizens.md` for the
//! methodology.

#![cfg(feature = "test-helpers")]

mod common;

use amm::Config;
use anchor_litesvm::TestHelpers;
use common::{setup, Pool};

#[test]
fn initialize_creates_config_lp_mint_and_vaults() {
    let mut world = setup();
    let admin = world.cast("Admin");
    let pool = Pool::derive(0, world.mint_x, world.mint_y);
    world.alias(pool.config, "Pool");
    world.alias(pool.mint_lp, "MintLP");

    let fee_bps: u16 = 30;
    world.initialize(&admin, &pool, fee_bps, Some(&admin));

    // Config carries the args verbatim and starts unlocked.
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.seed, 0);
    assert_eq!(config.fee_bps, fee_bps);
    assert_eq!(config.authority, Some(admin.pubkey()));
    assert_eq!(config.mint_x, pool.mint_x);
    assert_eq!(config.mint_y, pool.mint_y);
    assert!(!config.locked);

    // LP mint, both reserve vaults, and the lock vault all exist at zero balance.
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(0));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(0));
    assert_eq!(world.ctx.svm.token_balance(&pool.lp_vault), Some(0));
}

/// `fee_bps >= FEE_DENOMINATOR (10_000)` is rejected at init. The handler's
/// `require!((fee_bps as u64) < FEE_DENOMINATOR, AmmError::InvalidFee)` line
/// is the boundary; this test pins the boundary at exactly 10_000 (rejected)
/// and proves a fee that high never reaches Config storage.
#[test]
fn initialize_rejects_invalid_fee_at_denominator() {
    let mut world = setup();
    let admin = world.cast("Admin");
    let pool = Pool::derive(0, world.mint_x, world.mint_y);

    world
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
        .send_err_named("InvalidFee")
        .print_logs_structured();

    // Config was never created.
    assert!(!world.ctx.account_exists(&pool.config));
}
