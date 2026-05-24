//! Admin instructions: update_fee, set_locked, update_authority.
//! Happy paths plus two negative paths (unauthorized signer, renounced pool).

#![cfg(feature = "test-helpers")]

mod common;

use amm::{Config, SetLockedBundle, SwapKind, UpdateAuthorityBundle, UpdateFeeBundle};
use anchor_litesvm::{Signer, TestHelpers, TransactionHelpers};
use common::setup;

#[test]
fn update_fee_changes_fee_bps() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    let ix = world.ctx.program().build_ix(
        UpdateFeeBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateFee { new_fee_bps: 100 },
    );
    world
        .ctx
        .svm
        .send_ok(ix, &[&admin], &world.aliases)
        .print_logs_structured(&world.aliases);

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.fee_bps, 100);
}

#[test]
fn set_locked_flips_locked_field() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    let ix = world.ctx.program().build_ix(
        SetLockedBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::SetLocked { locked: true },
    );
    world
        .ctx
        .svm
        .send_ok(ix, &[&admin], &world.aliases)
        .print_logs_structured(&world.aliases);

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert!(config.locked, "locked should be true");
}

#[test]
fn update_authority_renounce_then_admin_calls_fail() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    // Renounce: set authority to None.
    let renounce = world.ctx.program().build_ix(
        UpdateAuthorityBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateAuthority {
            new_authority: None,
        },
    );
    world
        .ctx
        .svm
        .send_ok(renounce, &[&admin], &world.aliases)
        .print_logs_structured(&world.aliases);

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.authority, None);

    // After renounce, a subsequent update_fee from the original admin must
    // fail: Config.authority is None, so the handler returns AuthorityRenounced.
    let after = world.ctx.program().build_ix(
        UpdateFeeBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateFee { new_fee_bps: 50 },
    );
    let r = world.ctx.svm.send_instruction(after, &[&admin]).unwrap();
    r.print_logs_structured(&world.aliases);
    assert!(
        !r.is_success(),
        "admin instruction should fail after renounce"
    );
    let config2: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config2.fee_bps, 30, "fee unchanged after failed call");
}

#[test]
fn unauthorized_signer_cannot_update_fee() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let attacker = world.ctx.svm.create_funded_account(10_000_000_000).unwrap();
    world.alias(attacker.pubkey(), "Attacker");

    // Bundle declares attacker as the authority signer; handler will compare
    // attacker.pubkey() to Config.authority (which is admin) and reject with
    // Unauthorized.
    let ix = world.ctx.program().build_ix(
        UpdateFeeBundle {
            authority: attacker.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateFee { new_fee_bps: 1 },
    );
    let r = world.ctx.svm.send_instruction(ix, &[&attacker]).unwrap();
    r.print_logs_structured(&world.aliases);
    assert!(
        !r.is_success(),
        "non-authority caller should not be able to update fee"
    );

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.fee_bps, 30, "fee remained at initial value");
}

/// `set_locked` shares the manual auth check with `update_fee`. Verify a
/// non-authority signer is rejected.
#[test]
fn unauthorized_signer_cannot_set_locked() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let attacker = world.ctx.svm.create_funded_account(10_000_000_000).unwrap();
    world.alias(attacker.pubkey(), "Attacker");

    let ix = world.ctx.program().build_ix(
        SetLockedBundle {
            authority: attacker.pubkey(),
            config: pool.config,
        },
        amm::instruction::SetLocked { locked: true },
    );
    let r = world.ctx.svm.send_instruction(ix, &[&attacker]).unwrap();
    r.print_logs_structured(&world.aliases);
    assert!(
        !r.is_success(),
        "attacker should not be able to lock the pool"
    );

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert!(!config.locked, "pool remained unlocked");
}

/// After renouncement, `set_locked` must also fail with `AuthorityRenounced`.
#[test]
fn set_locked_after_renounce_fails() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    let renounce = world.ctx.program().build_ix(
        UpdateAuthorityBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateAuthority {
            new_authority: None,
        },
    );
    world
        .ctx
        .svm
        .send_ok(renounce, &[&admin], &world.aliases)
        .print_logs_structured(&world.aliases);

    let lock_ix = world.ctx.program().build_ix(
        SetLockedBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::SetLocked { locked: true },
    );
    let r = world.ctx.svm.send_instruction(lock_ix, &[&admin]).unwrap();
    r.print_logs_structured(&world.aliases);
    assert!(!r.is_success(), "set_locked must fail after renounce");
    assert!(
        r.logs().iter().any(|l| l.contains("AuthorityRenounced")),
        "expected AuthorityRenounced in logs"
    );
}

/// `update_authority` itself can be called by a non-authority. The handler's
/// manual check must reject before any state mutation.
#[test]
fn unauthorized_signer_cannot_update_authority() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);
    let attacker = world.ctx.svm.create_funded_account(10_000_000_000).unwrap();
    world.alias(attacker.pubkey(), "Attacker");

    let ix = world.ctx.program().build_ix(
        UpdateAuthorityBundle {
            authority: attacker.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateAuthority {
            new_authority: Some(attacker.pubkey()),
        },
    );
    let r = world.ctx.svm.send_instruction(ix, &[&attacker]).unwrap();
    r.print_logs_structured(&world.aliases);
    assert!(
        !r.is_success(),
        "attacker should not be able to take authority"
    );

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(
        config.authority,
        Some(admin.pubkey()),
        "authority unchanged after failed attempt"
    );
}

/// Once renounced, `update_authority` itself becomes uncallable: the
/// authority check requires a stored Some(_), which we don't have.
#[test]
fn update_authority_after_renounce_fails() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    let renounce = world.ctx.program().build_ix(
        UpdateAuthorityBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateAuthority {
            new_authority: None,
        },
    );
    world
        .ctx
        .svm
        .send_ok(renounce, &[&admin], &world.aliases)
        .print_logs_structured(&world.aliases);

    // Now try to un-renounce by setting authority back. Must fail.
    let restore = world.ctx.program().build_ix(
        UpdateAuthorityBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateAuthority {
            new_authority: Some(admin.pubkey()),
        },
    );
    let r = world.ctx.svm.send_instruction(restore, &[&admin]).unwrap();
    r.print_logs_structured(&world.aliases);
    assert!(!r.is_success(), "renounce is irreversible");
    assert!(
        r.logs().iter().any(|l| l.contains("AuthorityRenounced")),
        "expected AuthorityRenounced in logs"
    );
}

/// `update_fee` must propagate: after changing the fee, a subsequent swap
/// computes its amount_out using the new fee_bps, not the old one. This
/// verifies the handler reads `self.config.fee_bps` each invocation rather
/// than caching a value from initialization.
#[test]
fn update_fee_propagates_to_next_swap() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);
    let alice = world.make_user("Alice", 10_000_000_000, 10_000, 40_000);
    world.deposit(&pool, &alice, 1_000, 4_000, 1);

    // Bump fee to 1000 bps (10%); subsequent swap must use it.
    let update = world.ctx.program().build_ix(
        UpdateFeeBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateFee { new_fee_bps: 1_000 },
    );
    world
        .ctx
        .svm
        .send_ok(update, &[&admin], &world.aliases)
        .print_logs_structured(&world.aliases);

    // Bob swaps 100 X. With fee_bps = 1_000:
    //   amount_in_after_fee = floor(100 * 9_000 / 10_000) = 90
    //   amount_out = floor(90 * 4_000 / (1_000 + 90)) = floor(360_000 / 1_090) = 330
    // Note: with the original 30 bps fee, amount_out would have been 360
    // (per test_swap.rs). 330 != 360 proves the fee actually changed.
    let bob = world.make_user("Bob", 10_000_000_000, 1_000, 0);
    let swap = world.ctx.program().build_ix(
        pool.swap_bundle(&bob),
        amm::instruction::Swap {
            kind: SwapKind::ExactInput {
                amount_in: 100,
                min_amount_out: 1,
            },
            a_to_b: true,
        },
    );
    world
        .ctx
        .svm
        .send_ok(swap, &[&bob.signer], &world.aliases)
        .print_logs_structured(&world.aliases);

    assert_eq!(
        world.ctx.svm.token_balance(&bob.ata_y),
        Some(330),
        "swap used new fee_bps; with old 30 bps would have been 360"
    );
}

/// `update_authority` transfers admin privilege: the new authority can
/// call admin instructions, the old authority cannot.
#[test]
fn update_authority_rotation_transfers_admin_privilege() {
    let mut world = setup();
    let (alice_admin, pool) = world.fresh_pool(30);
    // alice_admin keeps the default "Admin" alias from fresh_pool. After the
    // rotation, "Admin" in log frames will refer to her former role; bob
    // gets a role-suffixed name so the rotation is visible in the trace and
    // he doesn't collide with a plain "Bob" trader in other tests.
    let bob_admin = world.ctx.svm.create_funded_account(10_000_000_000).unwrap();
    world.alias(bob_admin.pubkey(), "BobAdmin");

    // Alice rotates to bob.
    let rotate = world.ctx.program().build_ix(
        UpdateAuthorityBundle {
            authority: alice_admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateAuthority {
            new_authority: Some(bob_admin.pubkey()),
        },
    );
    world
        .ctx
        .svm
        .send_ok(rotate, &[&alice_admin], &world.aliases)
        .print_logs_structured(&world.aliases);

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.authority, Some(bob_admin.pubkey()));

    // Bob can now call admin instructions.
    let bob_locks = world.ctx.program().build_ix(
        SetLockedBundle {
            authority: bob_admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::SetLocked { locked: true },
    );
    world
        .ctx
        .svm
        .send_ok(bob_locks, &[&bob_admin], &world.aliases)
        .print_logs_structured(&world.aliases);
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert!(
        config.locked,
        "bob succeeded in locking as the new authority"
    );

    // Alice can no longer call admin instructions (handler's Unauthorized).
    let alice_tries = world.ctx.program().build_ix(
        SetLockedBundle {
            authority: alice_admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::SetLocked { locked: false },
    );
    let r = world
        .ctx
        .svm
        .send_instruction(alice_tries, &[&alice_admin])
        .unwrap();
    r.print_logs_structured(&world.aliases);
    assert!(
        !r.is_success(),
        "former authority must not retain privilege"
    );
    assert!(
        r.logs().iter().any(|l| l.contains("Unauthorized")),
        "expected Unauthorized in logs"
    );
}

/// `update_fee` must reject `new_fee_bps >= FEE_DENOMINATOR`. Same boundary
/// as `initialize`; verified separately because the check lives in the
/// `update_fee` handler too.
#[test]
fn update_fee_rejects_invalid_fee_at_denominator() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    let ix = world.ctx.program().build_ix(
        UpdateFeeBundle {
            authority: admin.pubkey(),
            config: pool.config,
        },
        amm::instruction::UpdateFee {
            new_fee_bps: 10_000,
        },
    );
    let r = world.ctx.svm.send_instruction(ix, &[&admin]).unwrap();
    r.print_logs_structured(&world.aliases);
    assert!(!r.is_success(), "fee_bps == FEE_DENOMINATOR must reject");
    assert!(
        r.logs().iter().any(|l| l.contains("InvalidFee")),
        "expected InvalidFee in logs"
    );

    // Fee unchanged.
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.fee_bps, 30);
}
