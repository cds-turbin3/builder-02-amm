//! `swap` happy paths: exact-input and exact-output, in the `a_to_b == true`
//! direction. Asserts on user/vault balances and that the constant-product
//! invariant `k` did not shrink across the trade.
//!
//! Each test threads a [`Report`]: intent + before/after snapshots, with
//! `check`s doubling as the assertions. Markdown lands in
//! `target/md-reports/<slug>.md`.

#![cfg(feature = "test-helpers")]

mod common;

use amm::SwapKind;
use anchor_litesvm::TestHelpers;
use common::{setup, MarkdownBlock, Report, SwapDir};

#[test]
fn exact_input_swap_a_to_b_moves_balances_and_grows_k() {
    let mut md = Report::new(
        "Swap (exact-input, X→Y): balances move and k grows",
        "Into a (1000, 4000) pool, Bob swaps 100 X for Y. With fee_bps = 30: \
         amount_after_fee = floor(100·9970/10000) = 99; amount_out = \
         floor(99·4000/1099) = 360. The constant product k must strictly grow \
         (the fee stays in the reserves).",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let lp = world.user("LP", 10_000, 40_000);
    md.step("Setup: LP opens the pool with reserves (1000, 4000)");
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    let bob = world.user("Bob", 1_000, 0);
    md.step("Before: Bob holds 1000 X, 0 Y");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("bob", &world.observe_user(&bob, &pool));

    md.step("Action: Bob swaps exactly 100 X in");
    world.swap(
        &bob,
        &pool,
        SwapKind::ExactInput { amount_in: 100, min_amount_out: 1 },
        SwapDir::AtoB,
    );

    md.step("After: Bob paid 100 X, received 360 Y; reserves shifted");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("bob", &world.observe_user(&bob, &pool));

    md.check("bob X (1000 − 100)", Some(900), world.ctx.svm.token_balance(&bob.ata_x));
    md.check("bob Y received", Some(360), world.ctx.svm.token_balance(&bob.ata_y));
    md.check("vault_x (1000 + 100)", Some(1_100), world.ctx.svm.token_balance(&pool.vault_x));
    md.check("vault_y (4000 − 360)", Some(3_640), world.ctx.svm.token_balance(&pool.vault_y));

    let k_pre = 1_000u128 * 4_000u128;
    let k_post = 1_100u128 * 3_640u128;
    md.note(format!("k: {k_pre} → {k_post} (fee accrued to reserves)"));
    md.check("k strictly grew", true, k_post > k_pre);
}

#[test]
fn exact_output_swap_a_to_b_pays_calculated_input() {
    let mut md = Report::new(
        "Swap (exact-output, X→Y): pays the calculated input",
        "Bob wants exactly 360 Y out of a (1000, 4000) pool. The ceiling-rounded \
         inverse requires 100 X in: amount_in_after_fee = ceil(360·1000/3640) = \
         99; amount_in = ceil(99·10000/9970) = 100. He pays exactly that, capped \
         at max_amount_in = 100.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let lp = world.user("LP", 10_000, 40_000);
    md.step("Setup: LP opens the pool with reserves (1000, 4000)");
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    let bob = world.user("Bob", 1_000, 0);
    md.step("Action: Bob requests exactly 360 Y out, capping input at 100 X");
    world.swap(
        &bob,
        &pool,
        SwapKind::ExactOutput { amount_out: 360, max_amount_in: 100 },
        SwapDir::AtoB,
    );

    md.step("After: Bob paid exactly 100 X, received exactly 360 Y");
    md.snapshot("bob", &world.observe_user(&bob, &pool));
    md.check("bob X (1000 − 100)", Some(900), world.ctx.svm.token_balance(&bob.ata_x));
    md.check("bob Y == requested", Some(360), world.ctx.svm.token_balance(&bob.ata_y));
}

/// Mirror of `exact_input_swap_a_to_b_moves_balances_and_grows_k` in the
/// opposite direction. Verifies that the handler's direction-picking branch
/// (the `if a_to_b` block selecting reserves and account infos) works for
/// both halves, not just the a_to_b side.
#[test]
fn exact_input_swap_b_to_a_picks_reserves_in_reverse() {
    let mut md = Report::new(
        "Swap (exact-input, Y→X): direction branch picks reserves in reverse",
        "The mirror of the X→Y case: Bob swaps 100 Y for X into a (1000, 4000) \
         pool. amount_after_fee = floor(100·9970/10000) = 99; amount_out = \
         floor(99·1000/4099) = 24. This exercises the handler's `if a_to_b` \
         branch on its reverse leg.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let lp = world.user("LP", 10_000, 40_000);
    md.step("Setup: LP opens the pool with reserves (1000, 4000)");
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    let bob = world.user("Bob", 0, 1_000);
    md.step("Action: Bob swaps exactly 100 Y in");
    world.swap(
        &bob,
        &pool,
        SwapKind::ExactInput { amount_in: 100, min_amount_out: 1 },
        SwapDir::BtoA,
    );

    md.step("After: Bob paid 100 Y, received 24 X; reserves shifted in reverse");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("bob", &world.observe_user(&bob, &pool));
    md.check("bob Y (1000 − 100)", Some(900), world.ctx.svm.token_balance(&bob.ata_y));
    md.check("bob X received", Some(24), world.ctx.svm.token_balance(&bob.ata_x));
    md.check("vault_y (4000 + 100)", Some(4_100), world.ctx.svm.token_balance(&pool.vault_y));
    md.check("vault_x (1000 − 24)", Some(976), world.ctx.svm.token_balance(&pool.vault_x));
}

/// Slippage protection on exact-input: if the user's `min_amount_out` is
/// higher than what the swap actually delivers, the handler must reject with
/// `SlippageExceeded` before any token movement. This is the user-side
/// guardrail against reserves moving between quote time and execution time.
#[test]
fn exact_input_swap_rejects_when_amount_out_below_min() {
    let mut md = Report::new(
        "Swap (exact-input): rejects when output is below the slippage floor",
        "100 X actually delivers 360 Y; demanding min_amount_out = 500 must \
         reject with SlippageExceeded before any token moves. This is the \
         user-side guardrail against reserves shifting between quote and \
         execution.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let lp = world.user("LP", 10_000, 40_000);
    md.step("Setup: LP opens the pool with reserves (1000, 4000)");
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    let bob = world.user("Bob", 1_000, 0);
    let bob_x_before = world.ctx.svm.token_balance(&bob.ata_x);
    md.step("Action: swap 100 X in but demand ≥ 500 Y out");
    let rejection = world
        .ctx
        .tx(&[&bob.signer])
        .build(
            amm::SwapBundle::from((&pool, &bob)),
            amm::instruction::Swap {
                kind: SwapKind::ExactInput { amount_in: 100, min_amount_out: 500 },
                a_to_b: SwapDir::AtoB.a_to_b(),
            },
        )
        .send_err_named("SlippageExceeded");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: Bob's tokens never moved");
    md.snapshot("bob", &world.observe_user(&bob, &pool));
    md.check("bob X unmoved", bob_x_before, world.ctx.svm.token_balance(&bob.ata_x));
    md.check("bob Y still zero", Some(0), world.ctx.svm.token_balance(&bob.ata_y));
}

/// Slippage protection on exact-output: if the user's `max_amount_in` is
/// lower than the math's required input, the handler must reject.
#[test]
fn exact_output_swap_rejects_when_amount_in_above_max() {
    let mut md = Report::new(
        "Swap (exact-output): rejects when required input exceeds the cap",
        "360 Y output requires 100 X input; capping max_amount_in at 50 must \
         reject with SlippageExceeded.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let lp = world.user("LP", 10_000, 40_000);
    md.step("Setup: LP opens the pool with reserves (1000, 4000)");
    world.deposit(&lp, &pool, 1_000, 4_000, 1_000);

    let bob = world.user("Bob", 1_000, 0);
    md.step("Action: request 360 Y out but cap input at 50 X (math needs 100)");
    let rejection = world
        .ctx
        .tx(&[&bob.signer])
        .build(
            amm::SwapBundle::from((&pool, &bob)),
            amm::instruction::Swap {
                kind: SwapKind::ExactOutput { amount_out: 360, max_amount_in: 50 },
                a_to_b: SwapDir::AtoB.a_to_b(),
            },
        )
        .send_err_named("SlippageExceeded");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: Bob's tokens never moved");
    md.check("bob X unmoved", Some(1_000), world.ctx.svm.token_balance(&bob.ata_x));
    md.check("bob Y still zero", Some(0), world.ctx.svm.token_balance(&bob.ata_y));
}
