//! Initialize a pool and assert the resulting on-chain state.
//!
//! Scenario: an admin creates a pool for the (mint_x, mint_y) pair at seed=0
//! with a 30 bps fee, naming themselves as the authority. The handler should
//! init the Config PDA, the LP mint, both reserve vaults, and the
//! permanent-lock vault. None of the vaults receive tokens yet (no deposit).

#![cfg(feature = "test-helpers")]

mod common;

use amm::{Config, InitializeBundle};
use anchor_litesvm::{Signer, TestHelpers, TransactionHelpers};
use common::{setup, Pool};

#[test]
fn initialize_creates_config_lp_mint_and_vaults() {
    let mut world = setup();
    let admin = world.ctx.svm.create_funded_account(10_000_000_000).unwrap();
    let pool = Pool::derive(0, world.mint_x, world.mint_y);
    world.alias(admin.pubkey(), "Admin");
    world.alias(pool.config, "Pool");
    world.alias(pool.mint_lp, "MintLP");

    let fee_bps: u16 = 30;
    let ix = world.ctx.program().build_ix(
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

    world
        .ctx
        .svm
        .send_ok(ix, &[&admin], &world.aliases)
        .print_logs_structured(&world.aliases);

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
    let admin = world.ctx.svm.create_funded_account(10_000_000_000).unwrap();
    let pool = Pool::derive(0, world.mint_x, world.mint_y);
    world.alias(admin.pubkey(), "Admin");

    let ix = world.ctx.program().build_ix(
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
            fee_bps: 10_000,
            authority: Some(admin.pubkey()),
        },
    );
    world
        .ctx
        .svm
        .send_err_named(ix, &[&admin], &world.aliases, "InvalidFee")
        .print_logs_structured(&world.aliases);

    // Config was never created.
    assert!(!world.ctx.account_exists(&pool.config));
}
