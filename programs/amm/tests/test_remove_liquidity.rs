//! `remove_liquidity` happy path: alice burns half her LP and receives
//! proportional shares of both vaults. The lock vault is unaffected; the
//! remaining LP supply still includes MINIMUM_LIQUIDITY.
//!
//! Each test threads a [`Report`]: intent + before/after snapshots, with
//! `check`s doubling as the assertions. Markdown lands in
//! `target/md-reports/<slug>.md`.

#![cfg(feature = "test-helpers")]

mod common;

use anchor_litesvm::TestHelpers;
use common::{setup, MarkdownBlock, Report};

#[test]
fn remove_returns_proportional_shares_and_leaves_lock_vault_intact() {
    let mut md = Report::new(
        "Remove Liquidity: proportional shares out, lock vault intact",
        "After Alice opens a (1000, 4000) pool, burning 500 of her 1000 LP \
         returns floor-on-both-sides proportional shares: 250 X and 1000 Y. The \
         lock vault keeps its MINIMUM_LIQUIDITY; the pool never empties.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let alice = world.user("Alice", 10_000, 40_000);
    md.step("Setup: Alice opens the pool with (1000, 4000)");
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);
    // Post-deposit state: vaults (1_000, 4_000), alice LP = 1_000,
    // lp_vault LP = 1_000, total supply = 2_000.
    md.snapshot("pool after open", &world.observe_pool(&pool));
    md.snapshot("alice after open", &world.observe_user(&alice, &pool));

    md.step("Action: Alice burns 500 LP");
    md.note(
        "By floor-on-both-sides at supply 2000: amount_a = floor(500·1000/2000) \
         = 250; amount_b = floor(500·4000/2000) = 1000.",
    );
    world.remove_liquidity(&alice, &pool, 500, 250, 1_000);

    md.step("After: Alice down to 500 LP, holding her returned shares");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("alice", &world.observe_user(&alice, &pool));

    md.check("alice LP after burn", Some(500), world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)));
    md.check("alice X (10000 − 1000 + 250)", Some(9_250), world.ctx.svm.token_balance(&alice.ata_x));
    md.check("alice Y (40000 − 4000 + 1000)", Some(37_000), world.ctx.svm.token_balance(&alice.ata_y));
    md.check("vault_x decreased by share", Some(750), world.ctx.svm.token_balance(&pool.vault_x));
    md.check("vault_y decreased by share", Some(3_000), world.ctx.svm.token_balance(&pool.vault_y));
    md.check("lock vault untouched", Some(1_000), world.ctx.svm.token_balance(&pool.lp_vault));
}

/// Slippage protection: if `min_a` or `min_b` is higher than the math's
/// computed share, the handler rejects.
#[test]
fn remove_liquidity_rejects_when_amount_below_min() {
    let mut md = Report::new(
        "Remove Liquidity: rejects when returned amount is below the slippage floor",
        "Burning 500 LP returns (250, 1000). Demanding min_a = 300 exceeds what \
         the math delivers, so the handler must reject with SlippageExceeded and \
         leave Alice's LP and the vaults untouched.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);
    md.step("Setup: Alice opens the pool with (1000, 4000)");
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);

    md.step("Action: burn 500 LP but demand min_a = 300 (math only yields 250)");
    let rejection = world
        .ctx
        .tx(&[&alice.signer])
        .build(
            amm::RemoveLiquidityBundle::from((&pool, &alice)),
            amm::instruction::RemoveLiquidity { lp_burn: 500, min_a: 300, min_b: 1_000 },
        )
        .send_err_named("SlippageExceeded");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: nothing moved");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("alice", &world.observe_user(&alice, &pool));
    md.check("alice LP unchanged", Some(1_000), world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)));
    md.check("vault_x unchanged", Some(1_000), world.ctx.svm.token_balance(&pool.vault_x));
}

/// When the pool is locked, `remove_liquidity` must return `PoolLocked`.
#[test]
fn remove_liquidity_rejects_when_pool_locked() {
    let mut md = Report::new(
        "Remove Liquidity: rejects when the pool is locked",
        "A locked pool must reject remove_liquidity with PoolLocked, even for a \
         depositor with a legitimate LP balance.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);
    md.step("Setup: Alice deposits, then admin locks the pool");
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);
    world.set_locked(&admin, &pool, true);

    md.step("Action: Alice attempts to burn 500 LP from the locked pool");
    let rejection = world
        .ctx
        .tx(&[&alice.signer])
        .build(
            amm::RemoveLiquidityBundle::from((&pool, &alice)),
            amm::instruction::RemoveLiquidity { lp_burn: 500, min_a: 0, min_b: 0 },
        )
        .send_err_named("PoolLocked");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: Alice's LP and the vaults are untouched");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.check("alice LP unchanged", Some(1_000), world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)));
    md.check("vault_x unchanged", Some(1_000), world.ctx.svm.token_balance(&pool.vault_x));
}
