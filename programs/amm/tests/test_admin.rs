//! Admin instructions: update_fee, set_locked, update_authority.
//! Happy paths plus the unauthorized-signer and renounced-pool negatives.
//!
//! Cast: in most scenarios, just `admin` (the authority returned by
//! `fresh_pool`). The unauthorized-signer scenarios add an `attacker`
//! (a `cast`-minted actor with no tokens, since the handler rejects
//! before any transfer). The rotation scenario splits the authority
//! across two actors (`alice_admin` becomes `bob_admin`).
//!
//! Each test threads a [`Report`]; Markdown lands in
//! `target/md-reports/<slug>.md`.

#![cfg(feature = "test-helpers")]

mod common;

use amm::{Config, SwapKind};
use anchor_litesvm::TestHelpers;
use common::{setup, MarkdownBlock, Report, SwapDir};

#[test]
fn update_fee_changes_fee_bps() {
    let mut md = Report::new(
        "Admin: update_fee changes fee_bps",
        "The authority can update the pool's fee. After update_fee(100), Config \
         reflects the new value.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);

    md.step("Action: admin updates fee_bps 30 → 100");
    world.update_fee(&admin, &pool, 100);

    md.step("After: Config carries the new fee");
    md.block("config", world.observe_config(&pool));
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("fee_bps updated", 100, config.fee_bps);
}

#[test]
fn set_locked_flips_locked_field() {
    let mut md = Report::new(
        "Admin: set_locked flips the locked field",
        "The authority can lock the pool. After set_locked(true), Config.locked \
         is true.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);

    md.step("Action: admin locks the pool");
    world.set_locked(&admin, &pool, true);

    md.step("After: Config.locked is true");
    md.block("config", world.observe_config(&pool));
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("locked flipped to true", true, config.locked);
}

#[test]
fn update_authority_renounce_then_admin_calls_fail() {
    let mut md = Report::new(
        "Admin: after renouncing authority, admin calls fail",
        "Setting authority to None renounces control permanently. A subsequent \
         update_fee from the former admin must fail with AuthorityRenounced, and \
         the fee must stay put.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);

    md.step("Action: admin renounces (authority → None)");
    world.update_authority(&admin, &pool, None);
    md.block("config after renounce", world.observe_config(&pool));
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("authority renounced", None, config.authority);

    md.step("Action: former admin attempts update_fee → must fail");
    let rejection = world
        .ctx
        .tx(&[&admin.signer])
        .build(
            amm::UpdateFeeBundle { authority: admin.pubkey(), config: pool.config },
            amm::instruction::UpdateFee { new_fee_bps: 50 },
        )
        .send_err_named("AuthorityRenounced");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: fee unchanged");
    let config2: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("fee unchanged after failed call", 30, config2.fee_bps);
}

#[test]
fn unauthorized_signer_cannot_update_fee() {
    let mut md = Report::new(
        "Admin: unauthorized signer cannot update_fee",
        "The handler compares the signer to Config.authority. An attacker signing \
         their own UpdateFeeBundle is rejected with Unauthorized; the fee stays \
         at its initial value.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);
    let attacker = world.cast("Attacker");

    md.step("Action: Attacker (not the authority) attempts update_fee");
    let rejection = world
        .ctx
        .tx(&[&attacker.signer])
        .build(
            amm::UpdateFeeBundle { authority: attacker.pubkey(), config: pool.config },
            amm::instruction::UpdateFee { new_fee_bps: 1 },
        )
        .send_err_named("Unauthorized");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: fee remained at the initial value");
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("fee unchanged", 30, config.fee_bps);
}

/// `set_locked` shares the manual auth check with `update_fee`. Verify a
/// non-authority signer is rejected.
#[test]
fn unauthorized_signer_cannot_set_locked() {
    let mut md = Report::new(
        "Admin: unauthorized signer cannot set_locked",
        "set_locked shares the manual auth check with update_fee. A non-authority \
         signer is rejected with Unauthorized and the pool stays unlocked.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);
    let attacker = world.cast("Attacker");

    md.step("Action: Attacker attempts set_locked(true)");
    let rejection = world
        .ctx
        .tx(&[&attacker.signer])
        .build(
            amm::SetLockedBundle { authority: attacker.pubkey(), config: pool.config },
            amm::instruction::SetLocked { locked: true },
        )
        .send_err_named("Unauthorized");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: pool remained unlocked");
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("still unlocked", false, config.locked);
}

/// After renouncement, `set_locked` must also fail with `AuthorityRenounced`.
#[test]
fn set_locked_after_renounce_fails() {
    let mut md = Report::new(
        "Admin: set_locked fails after renounce",
        "Once authority is renounced, set_locked from the former admin must fail \
         with AuthorityRenounced (the same guard update_fee hits).",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);

    md.step("Setup: admin renounces");
    world.update_authority(&admin, &pool, None);

    md.step("Action: former admin attempts set_locked(true) → must fail");
    let rejection = world
        .ctx
        .tx(&[&admin.signer])
        .build(
            amm::SetLockedBundle { authority: admin.pubkey(), config: pool.config },
            amm::instruction::SetLocked { locked: true },
        )
        .send_err_named("AuthorityRenounced");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: pool stayed unlocked");
    md.block("config", world.observe_config(&pool));
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("still unlocked", false, config.locked);
}

/// `update_authority` itself can be called by a non-authority. The handler's
/// manual check must reject before any state mutation.
#[test]
fn unauthorized_signer_cannot_update_authority() {
    let mut md = Report::new(
        "Admin: unauthorized signer cannot update_authority",
        "update_authority itself is guarded: an attacker trying to seize \
         authority is rejected with Unauthorized before any state mutation, and \
         the real authority is unchanged.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);
    let attacker = world.cast("Attacker");

    md.step("Action: Attacker attempts to seize authority");
    let rejection = world
        .ctx
        .tx(&[&attacker.signer])
        .build(
            amm::UpdateAuthorityBundle { authority: attacker.pubkey(), config: pool.config },
            amm::instruction::UpdateAuthority { new_authority: Some(attacker.pubkey()) },
        )
        .send_err_named("Unauthorized");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: authority unchanged");
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("authority still admin", Some(admin.pubkey()), config.authority);
}

/// Once renounced, `update_authority` itself becomes uncallable: the
/// authority check requires a stored Some(_), which we don't have.
#[test]
fn update_authority_after_renounce_fails() {
    let mut md = Report::new(
        "Admin: update_authority is uncallable after renounce",
        "Renouncement is one-way: with authority = None there is no Some(_) for \
         the guard to match, so trying to set authority back fails with \
         AuthorityRenounced.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);

    md.step("Setup: admin renounces");
    world.update_authority(&admin, &pool, None);

    md.step("Action: try to un-renounce by setting authority back → must fail");
    let rejection = world
        .ctx
        .tx(&[&admin.signer])
        .build(
            amm::UpdateAuthorityBundle { authority: admin.pubkey(), config: pool.config },
            amm::instruction::UpdateAuthority { new_authority: Some(admin.pubkey()) },
        )
        .send_err_named("AuthorityRenounced");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: authority is still None");
    md.block("config", world.observe_config(&pool));
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("authority still renounced", None, config.authority);
}

/// `update_fee` must propagate: after changing the fee, a subsequent swap
/// computes its amount_out using the new fee_bps, not the old one.
#[test]
fn update_fee_propagates_to_next_swap() {
    let mut md = Report::new(
        "Admin: update_fee propagates to the next swap",
        "The swap handler reads Config.fee_bps each invocation. After bumping the \
         fee to 1000 bps (10%), Bob's 100 X swap yields 330 Y, not the 360 it \
         would at the original 30 bps. 330 ≠ 360 proves the fee actually changed.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);
    let lp = world.user("LP", 10_000, 40_000);
    md.step("Setup: LP opens a (1000, 4000) pool");
    world.deposit(&lp, &pool, 1_000, 4_000, 1);

    md.step("Action: admin bumps fee 30 → 1000 bps");
    world.update_fee(&admin, &pool, 1_000);
    md.block("config", world.observe_config(&pool));

    let bob = world.user("Bob", 1_000, 0);
    md.step("Action: Bob swaps 100 X under the new fee");
    md.note(
        "amount_in_after_fee = floor(100·9000/10000) = 90; amount_out = \
         floor(90·4000/1090) = 330. (At 30 bps it would have been 360.)",
    );
    world.swap(
        &bob,
        &pool,
        SwapKind::ExactInput { amount_in: 100, min_amount_out: 1 },
        SwapDir::AtoB,
    );

    md.step("After: swap used the new fee");
    md.snapshot("bob", &world.observe_user(&bob, &pool));
    md.check("bob Y reflects 1000 bps fee (not 360)", Some(330), world.ctx.svm.token_balance(&bob.ata_y));
}

/// `update_authority` transfers admin privilege: the new authority can
/// call admin instructions, the old authority cannot.
#[test]
fn update_authority_rotation_transfers_admin_privilege() {
    let mut md = Report::new(
        "Admin: authority rotation transfers privilege",
        "After Alice (the initial admin) rotates authority to Bob, Bob can call \
         admin instructions and Alice can no longer: her later set_locked is \
         rejected with Unauthorized.",
    );

    let mut world = setup();
    let (alice_admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);
    let bob_admin = world.cast("Admin(Bob)");
    let bob_pk = bob_admin.pubkey();

    md.step("Action: Alice rotates authority to Bob");
    world.update_authority(&alice_admin, &pool, Some(&bob_admin));
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("authority is now Bob", Some(bob_pk), config.authority);

    md.step("Verify: Bob (new authority) can lock the pool");
    world.set_locked(&bob_admin, &pool, true);
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("bob locked the pool", true, config.locked);

    md.step("Verify: Alice (former authority) can no longer act → Unauthorized");
    let rejection = world
        .ctx
        .tx(&[&alice_admin.signer])
        .build(
            amm::SetLockedBundle { authority: alice_admin.pubkey(), config: pool.config },
            amm::instruction::SetLocked { locked: false },
        )
        .send_err_named("Unauthorized");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );
}

/// `update_fee` must reject `new_fee_bps >= FEE_DENOMINATOR`. Same boundary
/// as `initialize`; verified separately because the check lives in the
/// `update_fee` handler too.
#[test]
fn update_fee_rejects_invalid_fee_at_denominator() {
    let mut md = Report::new(
        "Admin: update_fee rejects fee at the denominator boundary",
        "Same boundary as initialize, enforced again in the update_fee handler: \
         new_fee_bps = 10_000 must reject with InvalidFee and leave the fee \
         unchanged.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool_as("Admin(Alice)", 30);

    md.step("Action: admin attempts update_fee(10_000) (the boundary)");
    let rejection = world
        .ctx
        .tx(&[&admin.signer])
        .build(
            amm::UpdateFeeBundle { authority: admin.pubkey(), config: pool.config },
            amm::instruction::UpdateFee { new_fee_bps: 10_000 },
        )
        .send_err_named("InvalidFee");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: fee unchanged");
    let config: Config = world.ctx.get_account(&pool.config).unwrap();
    md.check("fee unchanged", 30, config.fee_bps);
}
