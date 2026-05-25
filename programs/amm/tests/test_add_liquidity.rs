//! `add_liquidity` happy paths: the first-deposit branch (initial_liquidity,
//! locks MINIMUM_LIQUIDITY into lp_vault) and the subsequent-deposit branch
//! (floor-min formula, no lock-vault mint).
//!
//! Cast: a single user (`alice` or `bob`) doing the deposit; `admin`
//! returned by `fresh_pool` only matters when the test exercises the
//! locked-pool case.

#![cfg(feature = "test-helpers")]

mod common;

use anchor_litesvm::TestHelpers;
use common::setup;

#[test]
fn first_deposit_mints_to_user_and_locks_minimum_liquidity() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);

    // (a, b) = (1_000, 4_000) -> sqrt(4_000_000) = 2_000 total LP minted.
    // User receives 2_000 - MINIMUM_LIQUIDITY = 1_000; lp_vault gets 1_000.
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);

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
    let alice = world.user("Alice", 10_000, 40_000);
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);

    // Bob deposits (500, 2_000), which is ratio-correct (1:4).
    // lp_from_a = floor(500 * 2000 / 1000) = 1_000
    // lp_from_b = floor(2000 * 2000 / 4000) = 1_000
    // min = 1_000 -> bob gets 1_000 LP.
    let bob = world.user("Bob", 5_000, 20_000);
    world.deposit(&bob, &pool, 500, 2_000, 1_000);

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
    let alice = world.user("Alice", 10_000, 40_000);
    world.deposit_expecting(&alice, &pool, 1_000, 4_000, 1_001, "SlippageExceeded");

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

    let alice = world.user("Alice", 10_000, 40_000);
    world.set_locked(&admin, &pool, true);

    world.deposit_expecting(&alice, &pool, 1_000, 4_000, 0, "PoolLocked");
}
