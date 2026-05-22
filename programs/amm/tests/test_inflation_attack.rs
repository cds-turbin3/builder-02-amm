//! Math-soundness scenarios: the MINIMUM_LIQUIDITY inflation-attack mitigation.
//!
//! The classic V2 first-LP inflation attack: an attacker mints a tiny initial
//! LP, donates large reserves directly to the vault (which any party can do
//! by transferring SPL tokens to the vault ATA, bypassing add_liquidity), and
//! then watches honest depositors round to zero LP. The spec
//! ([§Minimum liquidity lock](../../../docs/toy-amm.spec.md#minimum-liquidity-lock-mandatory))
//! defends with two coupled guarantees:
//!
//! 1. The first deposit must produce `sqrt(a * b) > MINIMUM_LIQUIDITY`, so an
//!    attacker mounting this attack must commit at least MINIMUM_LIQUIDITY
//!    worth of value into permanently-locked LP.
//! 2. `add_liquidity` rejects deposits that would mint zero LP, so an honest
//!    depositor against an inflated pool fails atomically rather than losing
//!    their tokens to a 0-LP mint.
//!
//! These tests demonstrate both guarantees end-to-end via the on-chain program.

#![cfg(feature = "test-helpers")]

mod common;

use amm::AddLiquidityBundle;
use anchor_litesvm::{Aliases, TestHelpers, TransactionHelpers};
use common::setup;

/// MINIMUM_LIQUIDITY = 1000. With (1, 1), sqrt(1) = 1 <= 1000. With
/// (1000, 1000), sqrt(1_000_000) = 1000 == MINIMUM_LIQUIDITY (boundary).
/// Both should be rejected; the spec requires `minted > MINIMUM_LIQUIDITY`
/// strictly. The honest depositor's tokens stay in their ATA in both cases
/// because the math errors before any CPI runs.
#[test]
fn first_deposit_at_or_below_minimum_liquidity_rejects() {
    // Case 1: (1, 1). Way below threshold.
    {
        let mut world = setup();
        let (_admin, pool) = world.fresh_pool(30);
        let alice = world.make_user(10_000_000_000, 1_000_000, 1_000_000);

        let ix = world.ctx.program().build_ix(
            pool.add_liquidity_bundle(&alice),
            amm::instruction::AddLiquidity {
                amount_a: 1,
                amount_b: 1,
                min_lp_tokens: 0,
            },
        );
        let r = world
            .ctx
            .svm
            .send_instruction(ix, &[&alice.signer])
            .unwrap();
        r.print_logs_structured(&Aliases::default());
        assert!(!r.is_success(), "(1, 1) deposit must fail below threshold");

        // Alice's tokens never moved.
        assert_eq!(world.ctx.svm.token_balance(&alice.ata_x), Some(1_000_000));
        assert_eq!(world.ctx.svm.token_balance(&alice.ata_y), Some(1_000_000));
        // Vaults are still empty.
        assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(0));
        assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(0));
    }

    // Case 2: (1_000, 1_000). On the boundary: sqrt(1_000_000) == 1_000 == MIN.
    // Spec requires strict greater-than, so this also fails.
    {
        let mut world = setup();
        let (_admin, pool) = world.fresh_pool(30);
        let alice = world.make_user(10_000_000_000, 1_000_000, 1_000_000);

        let ix = world.ctx.program().build_ix(
            pool.add_liquidity_bundle(&alice),
            amm::instruction::AddLiquidity {
                amount_a: 1_000,
                amount_b: 1_000,
                min_lp_tokens: 0,
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
            "(1_000, 1_000) deposit must fail at the boundary"
        );
        assert_eq!(world.ctx.svm.token_balance(&alice.ata_x), Some(1_000_000));
        assert_eq!(world.ctx.svm.token_balance(&alice.ata_y), Some(1_000_000));
    }
}

/// One unit above the boundary: sqrt(1_002_001) = 1001 > MINIMUM_LIQUIDITY.
/// User receives 1 LP and lp_vault receives MINIMUM_LIQUIDITY = 1000.
/// This is the minimum-cost deposit that opens a pool: the attacker who
/// wants to mount an inflation attack must commit at least this much
/// (1001 + 1001 = 2002 tokens' worth of permanently-locked value).
#[test]
fn minimal_viable_first_deposit_succeeds_just_above_threshold() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.make_user(10_000_000_000, 10_000, 10_000);

    let ix = world.ctx.program().build_ix(
        pool.add_liquidity_bundle(&alice),
        amm::instruction::AddLiquidity {
            amount_a: 1_001,
            amount_b: 1_001,
            min_lp_tokens: 1,
        },
    );
    world
        .ctx
        .svm
        .send_ok(ix, &[&alice.signer])
        .print_logs_structured(&Aliases::default());

    assert_eq!(
        world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)),
        Some(1),
        "alice receives 1 LP (= 1001 - 1000)"
    );
    assert_eq!(
        world.ctx.svm.token_balance(&pool.lp_vault),
        Some(1_000),
        "lp_vault holds the permanently-locked MINIMUM_LIQUIDITY"
    );
}

/// End-to-end demonstration of the attack and its mitigation:
///
/// 1. Mallory opens the pool with the minimum viable deposit (1001, 1001).
///    She receives 1 LP; the lock vault gets MINIMUM_LIQUIDITY.
/// 2. Mallory donates 1_000_000 X directly to vault_x, bypassing
///    add_liquidity. (In production this would be an SPL Token::transfer
///    from her ATA; for test simplicity we mint to vault_x with the test
///    authority, which is functionally equivalent for the math: vault_x
///    grows without affecting LP supply.)
/// 3. Honest Henry attempts to deposit (1_000, 1_000). The math
///    (`lp_from_a = floor(1_000 * 1_001 / 1_001_001) = 0`,
///    `lp_from_b = floor(1_000 * 1_001 / 1_001) = 1_000`, `min = 0`)
///    returns `InsufficientLiquidity`. The handler rejects atomically, so
///    Henry's tokens never move.
///
/// Without the lp_tokens > 0 check, Henry would silently lose his tokens
/// to a zero-LP mint. Without MINIMUM_LIQUIDITY, Mallory's first deposit
/// could have been (1, 1) and the attack would be ~free.
#[test]
fn inflation_attack_via_donation_leaves_honest_depositor_unharmed() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    // Step 1: Mallory opens the pool minimally.
    let mallory = world.make_user(10_000_000_000, 2_000_000, 10_000);
    world.deposit(&pool, &mallory, 1_001, 1_001, 1);

    // Step 2: Mallory inflates vault_x by 1_000_000 via direct SPL deposit.
    // Vaults go from (1_001, 1_001) to (1_001_001, 1_001); LP supply stays
    // at 1_001 (= mallory's 1 + lp_vault's 1_000).
    world
        .ctx
        .svm
        .mint_to(
            &world.mint_x,
            &pool.vault_x,
            &world.mint_authority,
            1_000_000,
        )
        .unwrap();
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(1_001_001));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(1_001));

    // Step 3: Honest Henry attempts a "normal" deposit.
    let henry = world.make_user(10_000_000_000, 1_000_000, 1_000_000);
    let henry_x_before = world.ctx.svm.token_balance(&henry.ata_x);
    let henry_y_before = world.ctx.svm.token_balance(&henry.ata_y);
    // Capture lamports too: state rolls back on failure, but tx fees do not.
    // Henry is the fee payer (first signer), so this is what he loses to the
    // failed attempt.
    let henry_lamports_before = world.ctx.svm.get_balance(&henry.pubkey()).unwrap();

    let ix = world.ctx.program().build_ix(
        pool.add_liquidity_bundle(&henry),
        amm::instruction::AddLiquidity {
            amount_a: 1_000,
            amount_b: 1_000,
            min_lp_tokens: 0,
        },
    );
    let r = world
        .ctx
        .svm
        .send_instruction(ix, &[&henry.signer])
        .unwrap();
    r.print_logs_structured(&Aliases::default());
    assert!(
        !r.is_success(),
        "honest deposit against inflated pool must fail rather than mint 0 LP"
    );

    // Token state rolls back: Henry's X/Y balances are exactly what they were.
    assert_eq!(world.ctx.svm.token_balance(&henry.ata_x), henry_x_before);
    assert_eq!(world.ctx.svm.token_balance(&henry.ata_y), henry_y_before);

    // Fees do NOT roll back. Henry pays the tx fee even on failure; the SVM
    // reports it on the result. The lamport delta on Henry's account must
    // equal that fee exactly: no other on-chain effect should have moved his
    // SOL.
    let fee = r.inner().fee;
    assert!(fee > 0, "every signed tx pays at least the base fee");
    let henry_lamports_after = world.ctx.svm.get_balance(&henry.pubkey()).unwrap();
    assert_eq!(
        henry_lamports_before - henry_lamports_after,
        fee,
        "Henry's only loss is the tx fee; nothing else should have charged his lamports"
    );

    // Vaults unchanged from the post-donation state.
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_x), Some(1_001_001));
    assert_eq!(world.ctx.svm.token_balance(&pool.vault_y), Some(1_001));

    // The lock vault still holds MINIMUM_LIQUIDITY: the attacker can never
    // recover that 1_000 LP no matter how much they donate.
    assert_eq!(world.ctx.svm.token_balance(&pool.lp_vault), Some(1_000));
}
