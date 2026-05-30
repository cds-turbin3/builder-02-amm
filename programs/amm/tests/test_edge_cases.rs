//! Math edge cases that propagate up through the on-chain handlers.
//!
//! These exercise paths the math library has unit tests for, but where we
//! want to confirm the handler surfaces the right error variant (not just
//! a generic anchor failure) and that on-chain state is unchanged after
//! the reject. The handler's `.map_err(AmmError::from)?` line is the
//! bridge being tested.
//!
//! Each test threads a [`Report`]; Markdown lands in
//! `target/md-reports/<slug>.md`.

#![cfg(feature = "test-helpers")]

mod common;

use amm::SwapKind;
use anchor_litesvm::TestHelpers;
use common::{setup, MarkdownBlock, Report, SwapDir};

/// A tiny `amount_in` against a normally-sized pool truncates the fee'd
/// amount to zero, which (because the swap formula multiplies by 0) yields
/// zero output. The math returns `InsufficientOutput`; the handler must
/// surface it and abort before any token moves.
#[test]
fn swap_with_truncated_amount_in_returns_insufficient_output() {
    let mut md = Report::new(
        "Edge case: tiny swap input truncates to zero output",
        "With amount_in = 1 and fee_bps = 30: amount_in_after_fee = \
         floor(1·9970/10000) = 0, so amount_out = 0. The math returns \
         InsufficientOutput and the handler must surface it (not a generic \
         anchor error) before any token moves.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);
    md.step("Setup: Alice opens a (1000, 4000) pool");
    world.deposit(&alice, &pool, 1_000, 4_000, 1);

    let bob = world.user("Bob", 10, 0);
    md.step("Action: Bob swaps amount_in = 1 (fee truncates it to 0)");
    let rejection = world
        .ctx
        .tx(&[&bob.signer])
        .build(
            amm::SwapBundle::from((&pool, &bob)),
            amm::instruction::Swap {
                kind: SwapKind::ExactInput { amount_in: 1, min_amount_out: 0 },
                a_to_b: SwapDir::AtoB.a_to_b(),
            },
        )
        .send_err_named("InsufficientOutput");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: Bob's X is untouched");
    md.snapshot("bob", &world.observe_user(&bob, &pool));
    md.check("bob X unmoved", Some(10), world.ctx.svm.token_balance(&bob.ata_x));
}

/// All non-lock-vault LP is burned. After this, only `lp_vault` still
/// holds LP (== MINIMUM_LIQUIDITY), and the pool's reserves shrink to
/// the lock-vault's claim instead of going to zero. This is the
/// V2-style safety net the spec's [§Minimum liquidity lock] promises:
/// the pool never reaches a "0 LP / 0 reserves" state where the next
/// add_liquidity would face div-by-zero or first-LP-attack conditions.
#[test]
fn drain_to_minimum_liquidity_preserves_lock_vault_and_reserves() {
    let mut md = Report::new(
        "Edge case: draining to MINIMUM_LIQUIDITY keeps the pool re-bootstrappable",
        "When the only LP burns everything, the lock vault still holds \
         MINIMUM_LIQUIDITY and the reserves shrink to its proportional claim \
         (not zero). The pool is drained but not broken: a new LP can \
         re-bootstrap at the new ratio. This is the spec's minimum-liquidity \
         safety net, demonstrated end-to-end.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let alice = world.user("Alice", 10_000, 40_000);
    md.step("Setup: Alice opens and is the only LP (vaults 1000/4000, supply 2000)");
    world.deposit(&alice, &pool, 1_000, 4_000, 1);
    md.snapshot("pool after open", &world.observe_pool(&pool));

    md.step("Action: Alice burns all 1000 of her LP");
    md.note(
        "amount_a = floor(1000·1000/2000) = 500; amount_b = floor(1000·4000/2000) \
         = 2000. Alice's LP → 0; remaining supply = 1000 (lp_vault).",
    );
    world.remove_liquidity(&alice, &pool, 1_000, 500, 2_000);

    md.step("After: Alice drained to 0 LP; lock vault and proportional reserves remain");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("alice", &world.observe_user(&alice, &pool));
    md.check("alice burned everything", Some(0), world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)));
    md.check("alice X (10000 − 1000 + 500)", Some(9_500), world.ctx.svm.token_balance(&alice.ata_x));
    md.check("alice Y (40000 − 4000 + 2000)", Some(38_000), world.ctx.svm.token_balance(&alice.ata_y));
    md.check("lock vault holds MINIMUM_LIQUIDITY", Some(1_000), world.ctx.svm.token_balance(&pool.lp_vault));
    md.check("vault_x not drained to zero", Some(500), world.ctx.svm.token_balance(&pool.vault_x));
    md.check("vault_y not drained to zero", Some(2_000), world.ctx.svm.token_balance(&pool.vault_y));

    let bob = world.user("Bob", 1_000, 4_000);
    md.step("Verify: Bob re-bootstraps the drained pool with (500, 2000)");
    md.note(
        "supply = 1000 (lp_vault only); reserves = (500, 2000). lp_from_a = \
         floor(500·1000/500) = 1000; lp_from_b = floor(2000·1000/2000) = 1000; \
         min = 1000, so Bob gets 1000 LP.",
    );
    world.deposit(&bob, &pool, 500, 2_000, 1_000);
    md.snapshot("bob", &world.observe_user(&bob, &pool));
    md.check("bob re-bootstrapped the pool", Some(1_000), world.ctx.svm.token_balance(&bob.ata_lp(&pool.mint_lp)));
}
