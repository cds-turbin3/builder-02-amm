//! `remove_liquidity` happy path: alice burns half her LP and receives
//! proportional shares of both vaults. The lock vault is unaffected; the
//! remaining LP supply still includes MINIMUM_LIQUIDITY.

#![cfg(feature = "test-helpers")]

mod common;

use amm::RemoveLiquidityBundle;
use anchor_litesvm::{Aliases, TestHelpers, TransactionHelpers};
use common::setup;

#[test]
fn remove_returns_proportional_shares_and_leaves_lock_vault_intact() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let alice = world.make_user(10_000_000_000, 10_000, 40_000);
    world.deposit(&pool, &alice, 1_000, 4_000, 1_000);
    // Post-deposit state: vaults (1_000, 4_000), alice LP = 1_000,
    // lp_vault LP = 1_000, total supply = 2_000.

    // Burn 500 LP. By floor-on-both-sides:
    //   amount_a = floor(500 * 1_000 / 2_000) = 250
    //   amount_b = floor(500 * 4_000 / 2_000) = 1_000
    let ix = world.ctx.program().build_ix(
        RemoveLiquidityBundle {
            user: alice.pubkey(),
            mint_x: pool.mint_x,
            mint_y: pool.mint_y,
            config: pool.config,
            mint_lp: pool.mint_lp,
            vault_x: pool.vault_x,
            vault_y: pool.vault_y,
            user_x: alice.ata_x,
            user_y: alice.ata_y,
            user_lp: alice.ata_lp(&pool.mint_lp),
        },
        amm::instruction::RemoveLiquidity {
            lp_burn: 500,
            min_a: 250,
            min_b: 1_000,
        },
    );
    world
        .ctx
        .svm
        .send_ok(ix, &[&alice.signer])
        .print_logs_structured(&Aliases::default());

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
    let alice = world.make_user(10_000_000_000, 10_000, 40_000);
    world.deposit(&pool, &alice, 1_000, 4_000, 1_000);

    // Burning 500 LP returns (250, 1_000). Demanding min_a = 300 must reject.
    let ix = world.ctx.program().build_ix(
        RemoveLiquidityBundle {
            user: alice.pubkey(),
            mint_x: pool.mint_x,
            mint_y: pool.mint_y,
            config: pool.config,
            mint_lp: pool.mint_lp,
            vault_x: pool.vault_x,
            vault_y: pool.vault_y,
            user_x: alice.ata_x,
            user_y: alice.ata_y,
            user_lp: alice.ata_lp(&pool.mint_lp),
        },
        amm::instruction::RemoveLiquidity {
            lp_burn: 500,
            min_a: 300,
            min_b: 1_000,
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
        "remove must fail when min_a exceeds computed share"
    );
    assert!(
        r.logs().iter().any(|l| l.contains("SlippageExceeded")),
        "expected SlippageExceeded in logs"
    );

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
    let alice = world.make_user(10_000_000_000, 10_000, 40_000);
    world.deposit(&pool, &alice, 1_000, 4_000, 1_000);
    world.set_locked(&admin, &pool, true);

    let ix = world.ctx.program().build_ix(
        RemoveLiquidityBundle {
            user: alice.pubkey(),
            mint_x: pool.mint_x,
            mint_y: pool.mint_y,
            config: pool.config,
            mint_lp: pool.mint_lp,
            vault_x: pool.vault_x,
            vault_y: pool.vault_y,
            user_x: alice.ata_x,
            user_y: alice.ata_y,
            user_lp: alice.ata_lp(&pool.mint_lp),
        },
        amm::instruction::RemoveLiquidity {
            lp_burn: 500,
            min_a: 0,
            min_b: 0,
        },
    );
    let r = world
        .ctx
        .svm
        .send_instruction(ix, &[&alice.signer])
        .unwrap();
    r.print_logs_structured(&Aliases::default());
    assert!(!r.is_success(), "remove must fail on locked pool");
    assert!(
        r.logs().iter().any(|l| l.contains("PoolLocked")),
        "expected PoolLocked in logs"
    );
}
