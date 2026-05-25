//! Admin instructions: update_fee, set_locked, update_authority.
//! Happy paths plus the unauthorized-signer and renounced-pool negatives.
//!
//! Cast: in most scenarios, just `admin` (the authority returned by
//! `fresh_pool`). The unauthorized-signer scenarios add an `attacker`
//! (a `cast`-minted actor with no tokens, since the handler rejects
//! before any transfer). The rotation scenario splits the authority
//! across two actors (`alice_admin` becomes `bob_admin`).

#![cfg(feature = "test-helpers")]

mod common;

use amm::{Config, SwapKind};
use anchor_litesvm::TestHelpers;
use common::{setup, SwapDir};

#[test]
fn update_fee_changes_fee_bps() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    world.update_fee(&admin, &pool, 100);

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.fee_bps, 100);
}

#[test]
fn set_locked_flips_locked_field() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    world.set_locked(&admin, &pool, true);

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert!(config.locked, "locked should be true");
}

#[test]
fn update_authority_renounce_then_admin_calls_fail() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    // Renounce: set authority to None.
    world.update_authority(&admin, &pool, None);

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.authority, None);

    // After renounce, a subsequent update_fee from the original admin must
    // fail: Config.authority is None, so the handler returns AuthorityRenounced.
    world.update_fee_expecting(&admin, &pool, 50, "AuthorityRenounced");

    let config2: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config2.fee_bps, 30, "fee unchanged after failed call");
}

#[test]
fn unauthorized_signer_cannot_update_fee() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let attacker = world.cast("Attacker");

    // Bundle declares attacker as the authority signer; handler will compare
    // attacker.pubkey() to Config.authority (which is admin) and reject with
    // Unauthorized.
    world.update_fee_expecting(&attacker, &pool, 1, "Unauthorized");

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.fee_bps, 30, "fee remained at initial value");
}

/// `set_locked` shares the manual auth check with `update_fee`. Verify a
/// non-authority signer is rejected.
#[test]
fn unauthorized_signer_cannot_set_locked() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let attacker = world.cast("Attacker");

    world.set_locked_expecting(&attacker, &pool, true, "Unauthorized");

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert!(!config.locked, "pool remained unlocked");
}

/// After renouncement, `set_locked` must also fail with `AuthorityRenounced`.
#[test]
fn set_locked_after_renounce_fails() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    world.update_authority(&admin, &pool, None);
    world.set_locked_expecting(&admin, &pool, true, "AuthorityRenounced");
}

/// `update_authority` itself can be called by a non-authority. The handler's
/// manual check must reject before any state mutation.
#[test]
fn unauthorized_signer_cannot_update_authority() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);
    let attacker = world.cast("Attacker");

    world.update_authority_expecting(&attacker, &pool, Some(&attacker), "Unauthorized");

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

    world.update_authority(&admin, &pool, None);

    // Now try to un-renounce by setting authority back. Must fail.
    world.update_authority_expecting(&admin, &pool, Some(&admin), "AuthorityRenounced");
}

/// `update_fee` must propagate: after changing the fee, a subsequent swap
/// computes its amount_out using the new fee_bps, not the old one. This
/// verifies the handler reads `self.config.fee_bps` each invocation rather
/// than caching a value from initialization.
#[test]
fn update_fee_propagates_to_next_swap() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);
    world.deposit(&alice, &pool, 1_000, 4_000, 1);

    // Bump fee to 1000 bps (10%); subsequent swap must use it.
    world.update_fee(&admin, &pool, 1_000);

    // Bob swaps 100 X. With fee_bps = 1_000:
    //   amount_in_after_fee = floor(100 * 9_000 / 10_000) = 90
    //   amount_out = floor(90 * 4_000 / (1_000 + 90)) = floor(360_000 / 1_090) = 330
    // Note: with the original 30 bps fee, amount_out would have been 360
    // (per test_swap.rs). 330 != 360 proves the fee actually changed.
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
    // gets a role-suffixed name so the rotation is visible in the trace.
    let bob_admin = world.cast("BobAdmin");

    // Alice rotates to bob.
    world.update_authority(&alice_admin, &pool, Some(&bob_admin));

    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.authority, Some(bob_admin.pubkey()));

    // Bob can now call admin instructions.
    world.set_locked(&bob_admin, &pool, true);
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert!(
        config.locked,
        "bob succeeded in locking as the new authority"
    );

    // Alice can no longer call admin instructions (handler's Unauthorized).
    world.set_locked_expecting(&alice_admin, &pool, false, "Unauthorized");
}

/// `update_fee` must reject `new_fee_bps >= FEE_DENOMINATOR`. Same boundary
/// as `initialize`; verified separately because the check lives in the
/// `update_fee` handler too.
#[test]
fn update_fee_rejects_invalid_fee_at_denominator() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    world.update_fee_expecting(&admin, &pool, 10_000, "InvalidFee");

    // Fee unchanged.
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    assert_eq!(config.fee_bps, 30);
}
