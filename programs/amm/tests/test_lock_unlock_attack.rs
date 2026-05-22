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

#![cfg(feature = "test-helpers")]

mod common;

use amm::{SetLockedBundle, SwapBundle, SwapKind};
use anchor_litesvm::{Aliases, Signer, TestHelpers, TransactionHelpers};
use common::setup;

#[test]
fn admin_atomically_unlocks_swaps_and_relocks_while_users_blocked() {
    let mut world = setup();
    println!("\n1. Create a Pool");
    let (admin, pool) = world.fresh_pool(30);

    // Alice provides liquidity. The reserves she puts in are the pool's
    // working state; she's the user whose position is supposed to be
    // protected by the "locked" signal.
    let alice = world.make_user(10_000_000_000, 1_000_000, 1_000_000);
    println!("\n2. Initialize with tokens");
    world.deposit(&pool, &alice, 1_000_000, 1_000_000, 1);

    // The authority is also a trader here. We give them an X balance so
    // they have something to swap; the y ATA exists empty so the SPL
    // transfer in the swap-out leg has somewhere to land.
    let admin_ata_x = world
        .ctx
        .svm
        .create_associated_token_account(&world.mint_x, &admin)
        .unwrap();
    let admin_ata_y = world
        .ctx
        .svm
        .create_associated_token_account(&world.mint_y, &admin)
        .unwrap();
    world
        .ctx
        .svm
        .mint_to(&world.mint_x, &admin_ata_x, &world.mint_authority, 200_000)
        .unwrap();

    // Bob is an honest trader, here to play the role of "user the lock is
    // supposed to protect."
    let bob = world.make_user(10_000_000_000, 100_000, 0);

    let aliases = Aliases::default()
        .with(amm::ID, "amm")
        .with(admin.pubkey(), "admin")
        .with(alice.pubkey(), "alice")
        .with(bob.pubkey(), "bob");

    // ----- Step 1: authority locks the pool -----
    let lock_ix = world.ctx.program().build_ix(
        SetLockedBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::SetLocked { locked: true },
    );
    println!("\n3. Mystery .. Admin likely locks pool");
    world
        .ctx
        .svm
        .send_ok(lock_ix, &[&admin])
        .print_logs_structured(&aliases);

    // ----- Step 2: bob tries to swap, rejected with PoolLocked -----
    let bob_swap = world.ctx.program().build_ix(
        pool.swap_bundle(&bob),
        amm::instruction::Swap {
            kind: SwapKind::ExactInput {
                amount_in: 10_000,
                min_amount_out: 1,
            },
            a_to_b: true,
        },
    );
    let r = world
        .ctx
        .svm
        .send_instruction(bob_swap, &[&bob.signer])
        .unwrap();
    println!("\n4. Bob tries a swap, is denied");
    r.print_logs_structured(&aliases);
    assert!(!r.is_success(), "Bob's swap must fail while pool is locked");

    // Capture state before the attack tx.
    let admin_x_before = world.ctx.svm.token_balance(&admin_ata_x).unwrap();
    let admin_y_before = world.ctx.svm.token_balance(&admin_ata_y).unwrap();
    let vault_x_before = world.ctx.svm.token_balance(&pool.vault_x).unwrap();
    let vault_y_before = world.ctx.svm.token_balance(&pool.vault_y).unwrap();

    // ----- Step 3: authority's atomic unlock + swap + relock -----
    let unlock_ix = world.ctx.program().build_ix(
        SetLockedBundle {
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
            user_x: admin_ata_x,
            user_y: admin_ata_y,
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
        SetLockedBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::SetLocked { locked: true },
    );

    let r = world
        .ctx
        .svm
        .send_instructions(&[unlock_ix, admin_swap_ix, relock_ix], &[&admin])
        .unwrap();
    println!("\n5. Admin sandwiches a swap between an unlock and lock tx");
    r.print_logs_structured(&aliases);
    assert!(
        r.is_success(),
        "the three-ix atomic tx is currently allowed; this is the bug"
    );

    // ----- Step 4: the attack worked -----
    // Admin spent X, received Y at the locked-pool ratio.
    let admin_x_after = world.ctx.svm.token_balance(&admin_ata_x).unwrap();
    let admin_y_after = world.ctx.svm.token_balance(&admin_ata_y).unwrap();
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
    let bob_swap_again = world.ctx.program().build_ix(
        SwapBundle {
            user: bob.pubkey(),
            mint_x: pool.mint_x,
            mint_y: pool.mint_y,
            config: pool.config,
            vault_x: pool.vault_x,
            vault_y: pool.vault_y,
            user_x: bob.ata_x,
            user_y: bob.ata_y,
        },
        amm::instruction::Swap {
            kind: SwapKind::ExactInput {
                amount_in: 5_000,
                min_amount_out: 1,
            },
            a_to_b: true,
        },
    );
    let r = world
        .ctx
        .svm
        .send_instruction(bob_swap_again, &[&bob.signer])
        .unwrap();
    println!("\n6. Poor Bob is still locked out");
    r.print_logs_structured(&aliases);
    assert!(
        !r.is_success(),
        "Bob remains locked out after the authority's atomic trade"
    );
    // Sanity: the failure reason is PoolLocked (6008), not anything else.
    assert!(
        r.logs().iter().any(|l| l.contains("PoolLocked")),
        "expected PoolLocked in error logs; got: {:?}",
        r.logs()
    );
}
