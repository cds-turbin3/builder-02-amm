//! `remove_liquidity` happy path: alice burns half her LP and receives
//! proportional shares of both vaults. The lock vault is unaffected; the
//! remaining LP supply still includes MINIMUM_LIQUIDITY.

#![cfg(feature = "test-helpers")]

mod common;

use anchor_litesvm::TestHelpers;
use common::setup;

#[test]
fn remove_returns_proportional_shares_and_leaves_lock_vault_intact() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let alice = world.user("Alice", 10_000, 40_000);
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);
    // Post-deposit state: vaults (1_000, 4_000), alice LP = 1_000,
    // lp_vault LP = 1_000, total supply = 2_000.

    // Burn 500 LP. By floor-on-both-sides:
    //   amount_a = floor(500 * 1_000 / 2_000) = 250
    //   amount_b = floor(500 * 4_000 / 2_000) = 1_000
    world.remove_liquidity(&alice, &pool, 500, 250, 1_000);

    // Alice burned 500 LP, received 250 X + 1_000 Y.
    assert_eq!(
        world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)),
        Some(500),
        "alice LP after burn"
    );
    assert_eq!(
        world.ctx.svm.token_balance(&alice.ata_x),
        Some(9_250),
        "alice X (started 10_000, deposited 1_000, got 250 back)"
    );
    assert_eq!(
        world.ctx.svm.token_balance(&alice.ata_y),
        Some(37_000),
        "alice Y (started 40_000, deposited 4_000, got 1_000 back)"
    );

    // Vaults decreased by the proportional shares.
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(750));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(3_000));

    // Lock vault is untouched; the permanently-locked MINIMUM_LIQUIDITY remains.
    assert_eq!(world.ctx.svm.token_balance(&pool.lp_vault), Some(1_000));
}

/// Slippage protection: if `min_a` or `min_b` is higher than the math's
/// computed share, the handler rejects.
#[test]
fn remove_liquidity_rejects_when_amount_below_min() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);

    // Burning 500 LP returns (250, 1_000). Demanding min_a = 300 must reject.
    world.remove_liquidity_expecting(&alice, &pool, 500, 300, 1_000, "SlippageExceeded");

    // Alice's LP unchanged; vaults unchanged.
    assert_eq!(
        world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)),
        Some(1_000)
    );
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(1_000));
}

/// When the pool is locked, `remove_liquidity` must return `PoolLocked`.
#[test]
fn remove_liquidity_rejects_when_pool_locked() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);
    world.set_locked(&admin, &pool, true);

    world.remove_liquidity_expecting(&alice, &pool, 500, 0, 0, "PoolLocked");
}
