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
//! Each threads a [`Report`]; Markdown lands in `target/md-reports/<slug>.md`.

#![cfg(feature = "test-helpers")]

mod common;

use anchor_litesvm::TestHelpers;
use common::{setup, MarkdownBlock, Report};

/// MINIMUM_LIQUIDITY = 1000. With (1, 1), sqrt(1) = 1 <= 1000. With
/// (1000, 1000), sqrt(1_000_000) = 1000 == MINIMUM_LIQUIDITY (boundary).
/// Both should be rejected; the spec requires `minted > MINIMUM_LIQUIDITY`
/// strictly. The honest depositor's tokens stay in their ATA in both cases
/// because the math errors before any CPI runs.
#[test]
fn first_deposit_at_or_below_minimum_liquidity_rejects() {
    let mut md = Report::new(
        "Inflation defense: first deposit at or below MINIMUM_LIQUIDITY rejects",
        "MINIMUM_LIQUIDITY = 1000, and the spec requires the first deposit to \
         mint strictly more. Case 1 (1, 1): sqrt(1) = 1 ≪ 1000. Case 2 \
         (1000, 1000): sqrt(1_000_000) = 1000, exactly on the boundary. Both \
         reject with InsufficientLiquidity, and the depositor's tokens never \
         move (the math errors before any CPI).",
    );

    // Case 1: (1, 1). Way below threshold.
    md.step("Case 1: deposit (1, 1) — far below threshold");
    {
        let mut world = setup();
        let (_admin, pool) = world.fresh_pool(30);
        let alice = world.user("Alice", 1_000_000, 1_000_000);

        let rejection = world
            .ctx
            .tx(&[&alice.signer])
            .build(
                amm::AddLiquidityBundle::from((&pool, &alice)),
                amm::instruction::AddLiquidity { amount_a: 1, amount_b: 1, min_lp_tokens: 0 },
            )
            .send_err_named("InsufficientLiquidity");
        md.block(
            "case 1 rejection logs",
            MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
        );

        md.snapshot("case 1 alice", &world.observe_user(&alice, &pool));
        md.snapshot("case 1 pool", &world.observe_pool(&pool));
        md.check("case 1 alice X unmoved", Some(1_000_000), world.ctx.svm.token_balance(&alice.ata_x));
        md.check("case 1 alice Y unmoved", Some(1_000_000), world.ctx.svm.token_balance(&alice.ata_y));
        md.check("case 1 vault_x empty", Some(0), world.ctx.svm.token_balance(&pool.vault_x));
        md.check("case 1 vault_y empty", Some(0), world.ctx.svm.token_balance(&pool.vault_y));
    }

    // Case 2: (1_000, 1_000). On the boundary: sqrt(1_000_000) == 1_000 == MIN.
    md.step("Case 2: deposit (1000, 1000) — exactly on the boundary");
    {
        let mut world = setup();
        let (_admin, pool) = world.fresh_pool(30);
        let alice = world.user("Alice", 1_000_000, 1_000_000);

        let rejection = world
            .ctx
            .tx(&[&alice.signer])
            .build(
                amm::AddLiquidityBundle::from((&pool, &alice)),
                amm::instruction::AddLiquidity { amount_a: 1_000, amount_b: 1_000, min_lp_tokens: 0 },
            )
            .send_err_named("InsufficientLiquidity");
        md.block(
            "case 2 rejection logs",
            MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
        );

        md.check("case 2 alice X unmoved", Some(1_000_000), world.ctx.svm.token_balance(&alice.ata_x));
        md.check("case 2 alice Y unmoved", Some(1_000_000), world.ctx.svm.token_balance(&alice.ata_y));
    }
}

/// One unit above the boundary: sqrt(1_002_001) = 1001 > MINIMUM_LIQUIDITY.
/// User receives 1 LP and lp_vault receives MINIMUM_LIQUIDITY = 1000.
#[test]
fn minimal_viable_first_deposit_succeeds_just_above_threshold() {
    let mut md = Report::new(
        "Inflation defense: minimal viable first deposit just above the threshold",
        "(1001, 1001): sqrt(1_002_001) = 1001 > MINIMUM_LIQUIDITY. The depositor \
         receives 1 LP and the lock vault receives 1000. This is the cheapest \
         deposit that opens a pool, and the floor an inflation attacker must \
         permanently lock.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 10_000);

    md.step("Action: Alice deposits (1001, 1001)");
    world.deposit(&alice, &pool, 1_001, 1_001, 1);

    md.step("After: Alice holds 1 LP, lock vault holds MINIMUM_LIQUIDITY");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("alice", &world.observe_user(&alice, &pool));
    md.check("alice receives 1 LP (1001 − 1000)", Some(1), world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)));
    md.check("lock vault holds MINIMUM_LIQUIDITY", Some(1_000), world.ctx.svm.token_balance(&pool.lp_vault));
}

/// End-to-end demonstration of the attack and its mitigation.
#[test]
fn inflation_attack_via_donation_leaves_honest_depositor_unharmed() {
    let mut md = Report::new(
        "Inflation defense: donation attack leaves the honest depositor unharmed",
        "Mallory opens the pool minimally (1001, 1001), donates 1_000_000 X \
         directly to vault_x (bypassing add_liquidity), then honest Henry's \
         normal (1000, 1000) deposit would round to 0 LP. The handler rejects it \
         atomically (InsufficientLiquidity): Henry's tokens roll back, his only \
         loss is the tx fee, and the lock vault keeps its 1000 LP forever.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    md.step("Step 1: Mallory opens the pool minimally with (1001, 1001)");
    let mallory = world.user("Mallory", 2_000_000, 10_000);
    world.deposit(&mallory, &pool, 1_001, 1_001, 1);

    md.step("Step 2: Mallory donates 1_000_000 X straight to vault_x");
    md.note(
        "Direct SPL deposit bypasses add_liquidity: vault_x grows without \
         minting LP, so supply stays at 1001 (Mallory's 1 + lock vault's 1000).",
    );
    world.mint_to_vault_x(&pool, 1_000_000);
    md.snapshot("pool after donation", &world.observe_pool(&pool));
    md.check("vault_x inflated", Some(1_001_001), world.ctx.svm.token_balance(&pool.vault_x));
    md.check("vault_y unchanged", Some(1_001), world.ctx.svm.token_balance(&pool.vault_y));

    md.step("Step 3: honest Henry attempts a normal (1000, 1000) deposit");
    md.note(
        "lp_from_a = floor(1000·1001/1_001_001) = 0; lp_from_b = \
         floor(1000·1001/1001) = 1000; min = 0 → InsufficientLiquidity. Without \
         the lp > 0 check Henry would silently lose his tokens to a 0-LP mint.",
    );
    let henry = world.user("Henry", 1_000_000, 1_000_000);
    let henry_x_before = world.ctx.svm.token_balance(&henry.ata_x);
    let henry_y_before = world.ctx.svm.token_balance(&henry.ata_y);
    // Capture lamports too: state rolls back on failure, but tx fees do not.
    let henry_lamports_before = world.ctx.svm.get_balance(&henry.pubkey()).unwrap();

    let r = world
        .ctx
        .tx(&[&henry.signer])
        .build(
            amm::AddLiquidityBundle::from((&pool, &henry)),
            amm::instruction::AddLiquidity { amount_a: 1_000, amount_b: 1_000, min_lp_tokens: 0 },
        )
        .send_err_named("InsufficientLiquidity");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: r.logs_structured_string() },
    );

    md.step("After: Henry's token state rolled back");
    md.snapshot("henry", &world.observe_user(&henry, &pool));
    md.check("henry X rolled back", henry_x_before, world.ctx.svm.token_balance(&henry.ata_x));
    md.check("henry Y rolled back", henry_y_before, world.ctx.svm.token_balance(&henry.ata_y));

    // Fees do NOT roll back. Henry is the fee payer; the lamport delta must
    // equal the tx fee exactly, with no other on-chain effect on his SOL.
    let fee = r.inner().fee;
    md.check("a signed tx pays at least the base fee", true, fee > 0);
    let henry_lamports_after = world.ctx.svm.get_balance(&henry.pubkey()).unwrap();
    md.note(format!("Henry's only loss is the tx fee: {fee} lamports."));
    md.check(
        "henry's only lamport loss is the fee",
        fee,
        henry_lamports_before - henry_lamports_after,
    );

    md.step("After: vaults and lock vault unchanged from the post-donation state");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.check("vault_x unchanged", Some(1_001_001), world.ctx.svm.token_balance(&pool.vault_x));
    md.check("vault_y unchanged", Some(1_001), world.ctx.svm.token_balance(&pool.vault_y));
    md.check("lock vault still holds 1000 LP", Some(1_000), world.ctx.svm.token_balance(&pool.lp_vault));
}
