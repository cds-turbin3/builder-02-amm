//! `swap` happy paths: exact-input and exact-output, in the `a_to_b == true`
//! direction. Asserts on user/vault balances and that the constant-product
//! invariant `k` did not shrink across the trade.

#![cfg(feature = "test-helpers")]

mod common;

use amm::SwapKind;
use anchor_litesvm::TestHelpers;
use common::{setup, SwapDir};

#[test]
fn exact_input_swap_a_to_b_moves_balances_and_grows_k() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    // Open the pool with reserves (1_000, 4_000).
    let lp = world.user("LP", 10_000, 40_000);
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    // Bob brings 1_000 X; swap 100 of them for Y.
    //   amount_after_fee = floor(100 * 9_970 / 10_000) = 99
    //   amount_out = floor(99 * 4_000 / (1_000 + 99)) = floor(396_000 / 1_099) = 360
    let bob = world.user("Bob", 1_000, 0);
    world.swap(
        &bob,
        &pool,
        SwapKind::ExactInput {
            amount_in: 100,
            min_amount_out: 1,
        },
        SwapDir::AtoB,
    );

    // Bob paid 100 X, received 360 Y.
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_x), Some(900), "bob X");
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_y), Some(360), "bob Y");

    // Vaults now hold the new reserves.
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(1_100));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(3_640));

    // k strictly grew (fee accrued).
    let k_pre = 1_000u128 * 4_000u128;
    let k_post = 1_100u128 * 3_640u128;
    assert!(k_post > k_pre, "k did not grow: {} -> {}", k_pre, k_post);
}

#[test]
fn exact_output_swap_a_to_b_pays_calculated_input() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let lp = world.user("LP", 10_000, 40_000);
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    // Bob wants exactly 360 Y. Required input by ceiling-rounded inverse:
    //   amount_in_after_fee = ceil(360 * 1_000 / (4_000 - 360)) = 99
    //   amount_in = ceil(99 * 10_000 / 9_970) = 100
    let bob = world.user("Bob", 1_000, 0);
    world.swap(
        &bob,
        &pool,
        SwapKind::ExactOutput {
            amount_out: 360,
            max_amount_in: 100,
        },
        SwapDir::AtoB,
    );

    // Bob paid exactly 100 X (at the max_amount_in cap), received exactly 360 Y.
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_x), Some(900), "bob X");
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_y), Some(360), "bob Y");
}

/// Mirror of `exact_input_swap_a_to_b_moves_balances_and_grows_k` in the
/// opposite direction. Verifies that the handler's direction-picking branch
/// (the `if a_to_b` block selecting reserves and account infos) works for
/// both halves, not just the a_to_b side.
#[test]
fn exact_input_swap_b_to_a_picks_reserves_in_reverse() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let lp = world.user("LP", 10_000, 40_000);
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    // Bob brings Y; swap 100 Y for X.
    //   amount_after_fee = floor(100 * 9_970 / 10_000) = 99
    //   amount_out = floor(99 * 1_000 / (4_000 + 99)) = floor(99_000 / 4_099) = 24
    let bob = world.user("Bob", 0, 1_000);
    world.swap(
        &bob,
        &pool,
        SwapKind::ExactInput {
            amount_in: 100,
            min_amount_out: 1,
        },
        SwapDir::BtoA,
    );

    // Bob paid 100 Y, received 24 X.
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_y), Some(900), "bob Y");
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_x), Some(24), "bob X");

    // Vaults reflect: Y up, X down.
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(4_100));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(976));
}

/// Slippage protection on exact-input: if the user's `min_amount_out` is
/// higher than what the swap actually delivers, the handler must reject with
/// `SlippageExceeded` before any token movement. This is the user-side
/// guardrail against reserves moving between quote time and execution time.
#[test]
fn exact_input_swap_rejects_when_amount_out_below_min() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let lp = world.user("LP", 10_000, 40_000);
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    // 100 X actually delivers 360 Y; demanding 500 Y must reject.
    let bob = world.user("Bob", 1_000, 0);
    let bob_x_before = world.ctx.svm.token_balance(&bob.ata_x);
    world
        .ctx
        .tx(&[&bob.signer])
        .build(
            amm::SwapBundle::from((&pool, &bob)),
            amm::instruction::Swap {
                kind: SwapKind::ExactInput {
                    amount_in: 100,
                    min_amount_out: 500,
                },
                a_to_b: SwapDir::AtoB.a_to_b(),
            },
        )
        .send_err_named("SlippageExceeded")
        .print_markdown_pair();

    // Bob's tokens never moved.
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_x), bob_x_before);
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_y), Some(0));
}

/// Slippage protection on exact-output: if the user's `max_amount_in` is
/// lower than the math's required input, the handler must reject.
#[test]
fn exact_output_swap_rejects_when_amount_in_above_max() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let lp = world.user("LP", 10_000, 40_000);
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    // 360 Y output requires 100 X input (per the exact-output math); capping
    // at 50 X must reject.
    let bob = world.user("Bob", 1_000, 0);
    world
        .ctx
        .tx(&[&bob.signer])
        .build(
            amm::SwapBundle::from((&pool, &bob)),
            amm::instruction::Swap {
                kind: SwapKind::ExactOutput {
                    amount_out: 360,
                    max_amount_in: 50,
                },
                a_to_b: SwapDir::AtoB.a_to_b(),
            },
        )
        .send_err_named("SlippageExceeded")
        .print_markdown_pair();
}
