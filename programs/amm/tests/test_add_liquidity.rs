//! `add_liquidity` happy paths: the first-deposit branch (initial_liquidity,
//! locks MINIMUM_LIQUIDITY into lp_vault) and the subsequent-deposit branch
//! (floor-min formula, no lock-vault mint).

#![cfg(feature = "test-helpers")]

mod common;

use amm::AddLiquidityBundle;
use anchor_litesvm::{Aliases, TestHelpers, TransactionHelpers};
use common::setup;

#[test]
fn first_deposit_mints_to_user_and_locks_minimum_liquidity() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.make_user(10_000_000_000, 10_000, 40_000);

    // (a, b) = (1_000, 4_000) -> sqrt(4_000_000) = 2_000 total LP minted.
    // User receives 2_000 - MINIMUM_LIQUIDITY = 1_000; lp_vault gets 1_000.
    let ix = world.ctx.program().build_ix(
        pool.add_liquidity_bundle(&alice),
        amm::instruction::AddLiquidity {
            amount_a: 1_000,
            amount_b: 4_000,
            min_lp_tokens: 1_000,
        },
    );
    world
        .ctx
        .svm
        .send_ok(ix, &[&alice.signer])
        .print_logs_structured(&Aliases::default());

    // Alice received 1_000 LP, lp_vault holds the locked 1_000.
    assert_eq!(
        world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)),
        Some(1_000),
        "alice LP shares"
    );
    assert_eq!(
        world.ctx.svm.token_balance(&pool.lp_vault),
        Some(1_000),
        "lp_vault holds MINIMUM_LIQUIDITY"
    );

    // Vaults received the contribution exactly.
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(1_000));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(4_000));

    // Alice's token balances decremented by the contribution.
    assert_eq!(world.ctx.svm.token_balance(&alice.ata_x), Some(9_000));
    assert_eq!(world.ctx.svm.token_balance(&alice.ata_y), Some(36_000));
}

#[test]
fn subsequent_deposit_uses_floor_min_formula() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    // Alice opens the pool with (1_000, 4_000). After this, supply = 2_000
    // (1_000 user + 1_000 lp_vault); vaults = (1_000, 4_000).
    let alice = world.make_user(10_000_000_000, 10_000, 40_000);
    world.deposit(&pool, &alice, 1_000, 4_000, 1_000);

    // Bob deposits (500, 2_000), which is ratio-correct (1:4).
    // lp_from_a = floor(500 * 2000 / 1000) = 1_000
    // lp_from_b = floor(2000 * 2000 / 4000) = 1_000
    // min = 1_000 -> bob gets 1_000 LP.
    let bob = world.make_user(10_000_000_000, 5_000, 20_000);
    let ix = world.ctx.program().build_ix(
        pool.add_liquidity_bundle(&bob),
        amm::instruction::AddLiquidity {
            amount_a: 500,
            amount_b: 2_000,
            min_lp_tokens: 1_000,
        },
    );
    world
        .ctx
        .svm
        .send_ok(ix, &[&bob.signer])
        .print_logs_structured(&Aliases::default());

    // Bob received 1_000 LP. Alice's LP is unchanged (no dilution).
    assert_eq!(
        world.ctx.svm.token_balance(&bob.ata_lp(&pool.mint_lp)),
        Some(1_000),
        "bob LP shares"
    );
    assert_eq!(
        world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)),
        Some(1_000),
        "alice LP unchanged"
    );

    // lp_vault unchanged at MINIMUM_LIQUIDITY.
    assert_eq!(world.ctx.svm.token_balance(&pool.lp_vault), Some(1_000));

    // Vaults absorbed bob's contribution.
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(1_500));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(6_000));

    // Bob's user balances decremented.
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_x), Some(4_500));
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_y), Some(18_000));
}

/// Slippage protection: if `min_lp_tokens` exceeds what the math actually
/// mints, the handler rejects before any token movement.
#[test]
fn add_liquidity_rejects_when_lp_below_min() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    // For (1_000, 4_000), the initial-liquidity math mints
    // sqrt(4_000_000) - MINIMUM_LIQUIDITY = 1_000 to the user. Asking for
    // 1_001 must reject.
    let alice = world.make_user(10_000_000_000, 10_000, 40_000);
    let ix = world.ctx.program().build_ix(
        pool.add_liquidity_bundle(&alice),
        amm::instruction::AddLiquidity {
            amount_a: 1_000,
            amount_b: 4_000,
            min_lp_tokens: 1_001,
        },
    );
    let r = world
        .ctx
        .svm
        .send_instruction(ix, &[&alice.signer])
        .unwrap();
    r.print_logs_structured(&Aliases::default());
    assert!(
        !r.is_success(),
        "deposit must fail when min_lp_tokens > minted"
    );
    assert!(
        r.logs().iter().any(|l| l.contains("SlippageExceeded")),
        "expected SlippageExceeded in logs"
    );

    // Alice's tokens unmoved; vaults still empty.
    assert_eq!(world.ctx.svm.token_balance(&alice.ata_x), Some(10_000));
    assert_eq!(world.ctx.svm.token_balance(&alice.ata_y), Some(40_000));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(0));
}

/// When the pool is locked, `add_liquidity` must return `PoolLocked` before
/// any CPI runs (mirrors the established check inside `swap`).
#[test]
fn add_liquidity_rejects_when_pool_locked() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    let alice = world.make_user(10_000_000_000, 10_000, 40_000);
    world.set_locked(&admin, &pool, true);

    let ix = world.ctx.program().build_ix(
        pool.add_liquidity_bundle(&alice),
        amm::instruction::AddLiquidity {
            amount_a: 1_000,
            amount_b: 4_000,
            min_lp_tokens: 0,
        },
    );
    let r = world
        .ctx
        .svm
        .send_instruction(ix, &[&alice.signer])
        .unwrap();
    r.print_logs_structured(&Aliases::default());
    assert!(!r.is_success(), "add_liquidity must fail on locked pool");
    assert!(
        r.logs().iter().any(|l| l.contains("PoolLocked")),
        "expected PoolLocked in logs"
    );
}
