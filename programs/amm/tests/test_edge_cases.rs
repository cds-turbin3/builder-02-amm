//! Math edge cases that propagate up through the on-chain handlers.
//!
//! These exercise paths the math library has unit tests for, but where we
//! want to confirm the handler surfaces the right error variant (not just
//! a generic anchor failure) and that on-chain state is unchanged after
//! the reject. The handler's `.map_err(AmmError::from)?` line is the
//! bridge being tested.

#![cfg(feature = "test-helpers")]

mod common;

use amm::SwapKind;
use anchor_litesvm::{TestHelpers, TransactionHelpers};
use common::setup;

/// A tiny `amount_in` against a normally-sized pool truncates the fee'd
/// amount to zero, which (because the swap formula multiplies by 0) yields
/// zero output. The math returns `InsufficientOutput`; the handler must
/// surface it and abort before any token moves.
///
/// Concretely: `amount_in = 1`, `fee_bps = 30`:
///   amount_in_after_fee = floor(1 * 9_970 / 10_000) = 0
///   amount_out = floor(0 * reserve_out / (reserve_in + 0)) = 0
///   -> InsufficientOutput.
#[test]
fn swap_with_truncated_amount_in_returns_insufficient_output() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.make_user("Alice", 10_000_000_000, 10_000, 40_000);
    world.deposit(&pool, &alice, 1_000, 4_000, 1);

    let bob = world.make_user("Bob", 10_000_000_000, 10, 0);
    let ix = world.ctx.program().build_ix(
        pool.swap_bundle(&bob),
        amm::instruction::Swap {
            kind: SwapKind::ExactInput {
                amount_in: 1,
                min_amount_out: 0,
            },
            a_to_b: true,
        },
    );
    let r = world.ctx.svm.send_instruction(ix, &[&bob.signer]).unwrap();
    r.print_logs_structured(&world.aliases);
    assert!(!r.is_success(), "swap with amount_in = 1 must reject");
    assert!(
        r.logs().iter().any(|l| l.contains("InsufficientOutput")),
        "expected InsufficientOutput in logs"
    );

    // Bob's X is untouched.
    assert_eq!(world.ctx.svm.token_balance(&bob.ata_x), Some(10));
}

/// All non-lock-vault LP is burned. After this, only `lp_vault` still
/// holds LP (== MINIMUM_LIQUIDITY), and the pool's reserves shrink to
/// the lock-vault's claim instead of going to zero. This is the
/// V2-style safety net the spec's [§Minimum liquidity lock] promises:
/// the pool never reaches a "0 LP / 0 reserves" state where the next
/// add_liquidity would face div-by-zero or first-LP-attack conditions.
#[test]
fn drain_to_minimum_liquidity_preserves_lock_vault_and_reserves() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    // Alice opens and is the only LP.
    //   vaults = (1_000, 4_000); alice = 1_000 LP; lp_vault = 1_000 LP; supply = 2_000
    let alice = world.make_user("Alice", 10_000_000_000, 10_000, 40_000);
    world.deposit(&pool, &alice, 1_000, 4_000, 1);

    // Alice burns all her LP. The math:
    //   amount_a = floor(1_000 * 1_000 / 2_000) = 500
    //   amount_b = floor(1_000 * 4_000 / 2_000) = 2_000
    //   alice's LP after burn = 0; pool's LP supply after burn = 1_000 (lp_vault).
    let burn = world.ctx.program().build_ix(
        pool.remove_liquidity_bundle(&alice),
        amm::instruction::RemoveLiquidity {
            lp_burn: 1_000,
            min_a: 500,
            min_b: 2_000,
        },
    );
    world
        .ctx
        .svm
        .send_ok(burn, &[&alice.signer], &world.aliases)
        .print_logs_structured(&world.aliases);

    // Alice has zero LP and the proportional tokens back.
    assert_eq!(
        world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)),
        Some(0),
        "alice burned everything"
    );
    assert_eq!(world.ctx.svm.token_balance(&alice.ata_x), Some(9_500));
    assert_eq!(world.ctx.svm.token_balance(&alice.ata_y), Some(38_000));

    // lp_vault still holds MINIMUM_LIQUIDITY; vaults still hold the
    // lock-vault's proportional share (not zero).
    assert_eq!(world.ctx.svm.token_balance(&pool.lp_vault), Some(1_000));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(500));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(2_000));

    // The pool is now "drained" but not broken: a new LP can re-bootstrap
    // via add_liquidity at the new ratio. Bob deposits to verify.
    //   supply = 1_000 (only lp_vault); reserves = (500, 2_000)
    //   bob deposits (500, 2_000) at the current ratio:
    //     lp_from_a = floor(500 * 1_000 / 500)   = 1_000
    //     lp_from_b = floor(2_000 * 1_000 / 2_000) = 1_000
    //     min = 1_000 -> bob gets 1_000 LP
    let bob = world.make_user("Bob", 10_000_000_000, 1_000, 4_000);
    world.deposit(&pool, &bob, 500, 2_000, 1_000);
    assert_eq!(
        world.ctx.svm.token_balance(&bob.ata_lp(&pool.mint_lp)),
        Some(1_000),
        "bob successfully re-bootstrapped the drained pool"
    );
}
