//! `add_liquidity` happy paths: the first-deposit branch (initial_liquidity,
//! locks MINIMUM_LIQUIDITY into lp_vault) and the subsequent-deposit branch
//! (floor-min formula, no lock-vault mint).
//!
//! Cast: a single user (`alice` or `bob`) doing the deposit; `admin`
//! returned by `fresh_pool` only matters when the test exercises the
//! locked-pool case.
//!
//! Each test threads a [`Report`]: it narrates intent and snapshots the
//! before/after vault + actor balances, and its `check`s double as the
//! assertions. The Markdown lands in `target/md-reports/<slug>.md` (and on
//! stdout under `--nocapture`); the per-transaction logs still come from the
//! `print_markdown_pair()` / `print_logs_structured()` calls inside the verbs.

#![cfg(feature = "test-helpers")]

mod common;
use anchor_litesvm::TestHelpers;
use common::{setup, MarkdownBlock, Report};

/// Guard regression: actor labels seed keypairs, so reusing one within a
/// scenario must fail loudly rather than silently alias two actors to one
/// address. (No `Report` here: the panic is the assertion.)
#[test]
#[should_panic(expected = "already used in this scenario")]
fn duplicate_actor_label_panics() {
    let mut world = setup();
    let _alice = world.user("Alice", 10_000, 40_000);
    let _also_alice = world.user("Alice", 1, 1); // same label -> same keypair
}

/// The escape hatch (`actor`): a second handle to an existing identity is
/// allowed, returns the same address, and sees the same on-chain balances. This
/// is the sanctioned counterpart to `duplicate_actor_label_panics`: re-*minting*
/// a label is the bug; re-*referencing* it is fine.
#[test]
fn actor_refetches_the_same_identity_without_reminting() {
    let mut world = setup();
    let alice = world.user("Alice", 10_000, 40_000);

    // Re-fetch by label: same pubkey, same ATAs, no panic, no second mint.
    let alice_again = world.actor("Alice");
    assert_eq!(alice.pubkey(), alice_again.pubkey(), "same derived identity");
    assert_eq!(alice.ata_x, alice_again.ata_x, "same X ATA");
    assert_eq!(alice.ata_y, alice_again.ata_y, "same Y ATA");

    // The handle is live: balances read through it match the original (a second
    // mint would have doubled these).
    assert_eq!(world.ctx.svm.token_balance(&alice_again.ata_x), Some(10_000));
    assert_eq!(world.ctx.svm.token_balance(&alice_again.ata_y), Some(40_000));
}

#[test]
fn first_deposit_mints_to_user_and_locks_minimum_liquidity() {
    let mut md = Report::new(
        "Add Liquidity: first deposit mints to user and locks minimum liquidity",
        "Opening an empty pool with (1000 X, 4000 Y): the AMM mints \
         sqrt(1000·4000) = 2000 LP. The depositor keeps 2000 − MINIMUM_LIQUIDITY \
         = 1000; the protocol permanently locks the other 1000 in lp_vault so the \
         pool can never be drained back to empty.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);

    md.step("Before: empty pool, funded depositor");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("alice", &world.observe_user(&alice, &pool));

    md.step("Action: Alice deposits 1000 X and 4000 Y, asking for ≥ 1000 LP");
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);

    md.step("After: 2000 LP split between Alice (1000) and the locked vault (1000)");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("alice", &world.observe_user(&alice, &pool));

    md.check("alice LP shares", Some(1_000), world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)));
    md.check("lp_vault holds MINIMUM_LIQUIDITY", Some(1_000), world.ctx.svm.token_balance(&pool.lp_vault));
    md.check("vault_x absorbed deposit", Some(1_000), world.ctx.svm.token_balance(&pool.vault_x));
    md.check("vault_y absorbed deposit", Some(4_000), world.ctx.svm.token_balance(&pool.vault_y));
    md.check("alice X debited", Some(9_000), world.ctx.svm.token_balance(&alice.ata_x));
    md.check("alice Y debited", Some(36_000), world.ctx.svm.token_balance(&alice.ata_y));
}

#[test]
fn subsequent_deposit_uses_floor_min_formula() {
    let mut md = Report::new(
        "Add Liquidity: subsequent deposit uses the floor-min formula",
        "Once a pool holds liquidity, a deposit mints \
         min(floor(a·supply/reserve_a), floor(b·supply/reserve_b)) LP and never \
         touches lp_vault. Bob's ratio-correct (500, 2000) into a (1000, 4000) \
         pool mints exactly 1000 LP and does not dilute Alice.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let alice = world.user("Alice", 10_000, 40_000);
    md.step("Setup: Alice opens the pool with (1000, 4000); supply becomes 2000");
    world.deposit(&alice, &pool, 1_000, 4_000, 1_000);
    md.snapshot("pool after open", &world.observe_pool(&pool));

    let bob = world.user("Bob", 5_000, 20_000);
    md.step("Action: Bob deposits ratio-correct (500, 2000)");
    md.note(
        "lp_from_a = floor(500·2000/1000) = 1000; lp_from_b = floor(2000·2000/4000) \
         = 1000; min = 1000, so Bob receives 1000 LP.",
    );
    world.deposit(&bob, &pool, 500, 2_000, 1_000);

    md.step("After: Bob holds 1000 LP, Alice undiluted, lp_vault unchanged");
    md.snapshot("pool", &world.observe_pool(&pool));
    md.snapshot("bob", &world.observe_user(&bob, &pool));

    md.check("bob LP shares", Some(1_000), world.ctx.svm.token_balance(&bob.ata_lp(&pool.mint_lp)));
    md.check("alice LP unchanged", Some(1_000), world.ctx.svm.token_balance(&alice.ata_lp(&pool.mint_lp)));
    md.check("lp_vault unchanged", Some(1_000), world.ctx.svm.token_balance(&pool.lp_vault));
    md.check("vault_x absorbed bob", Some(1_500), world.ctx.svm.token_balance(&pool.vault_x));
    md.check("vault_y absorbed bob", Some(6_000), world.ctx.svm.token_balance(&pool.vault_y));
    md.check("bob X debited", Some(4_500), world.ctx.svm.token_balance(&bob.ata_x));
    md.check("bob Y debited", Some(18_000), world.ctx.svm.token_balance(&bob.ata_y));
}

#[test]
fn add_liquidity_rejects_when_lp_below_min() {
    let mut md = Report::new(
        "Add Liquidity: rejects when minted LP would fall below the slippage floor",
        "For (1000 X, 4000 Y) the initial-liquidity math mints exactly 1000 LP to \
         the depositor. Asking for 1001 (min_lp_tokens) must reject as \
         SlippageExceeded before any token moves.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);

    md.step("Action: deposit (1000, 4000) but demand ≥ 1001 LP");
    let rejection = world
        .ctx
        .tx(&[&alice.signer])
        .build(
            amm::AddLiquidityBundle::from((&pool, &alice)),
            amm::instruction::AddLiquidity { amount_a: 1_000, amount_b: 4_000, min_lp_tokens: 1_001 },
        )
        .send_err_named("SlippageExceeded");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: rejection left every balance untouched");
    md.snapshot("alice", &world.observe_user(&alice, &pool));
    md.snapshot("pool", &world.observe_pool(&pool));

    md.check("alice X unmoved", Some(10_000), world.ctx.svm.token_balance(&alice.ata_x));
    md.check("alice Y unmoved", Some(40_000), world.ctx.svm.token_balance(&alice.ata_y));
    md.check("vault_x still empty", Some(0), world.ctx.svm.token_balance(&pool.vault_x));
}

#[test]
fn add_liquidity_rejects_when_pool_locked() {
    let mut md = Report::new(
        "Add Liquidity: rejects when the pool is locked",
        "A locked pool must reject add_liquidity with PoolLocked before any CPI \
         runs, mirroring the established guard inside swap.",
    );

    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);
    let alice = world.user("Alice", 10_000, 40_000);

    md.step("Setup: admin locks the pool");
    world.set_locked(&admin, &pool, true);

    md.step("Action: Alice attempts a deposit into the locked pool");
    let rejection = world
        .ctx
        .tx(&[&alice.signer])
        .build(
            amm::AddLiquidityBundle::from((&pool, &alice)),
            amm::instruction::AddLiquidity { amount_a: 1_000, amount_b: 4_000, min_lp_tokens: 0 },
        )
        .send_err_named("PoolLocked");
    md.block(
        "rejection logs",
        MarkdownBlock::Fenced { lang: "console".into(), body: rejection.logs_structured_string() },
    );

    md.step("After: nothing moved");
    md.snapshot("alice", &world.observe_user(&alice, &pool));
    md.snapshot("pool", &world.observe_pool(&pool));

    md.check("alice X unmoved", Some(10_000), world.ctx.svm.token_balance(&alice.ata_x));
    md.check("vault_x still empty", Some(0), world.ctx.svm.token_balance(&pool.vault_x));
}
