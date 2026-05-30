//! Integration scenarios that span multiple instructions and assert
//! cross-instruction invariants:
//!
//! - **Token conservation.** Sum of user balances + vault balances must
//!   equal the cumulative amount minted to users via the test helpers.
//!   Every deposit, swap, and withdraw redistributes tokens; none of
//!   them should create or destroy any.
//! - **Fee accrual to LPs.** The V2 fee model puts swap fees into the
//!   reserves rather than skimming them out. After a series of swaps,
//!   the constant-product `k` (= vault_x * vault_y) must strictly grow
//!   beyond its post-deposit value, and the LP's pro-rata share of the
//!   new reserves must be worth more (geometrically) than their share
//!   was immediately after deposit.
//!
//! Each test threads a [`Report`]; the conservation check is recorded at
//! every step. Markdown lands in `target/md-reports/<slug>.md`.

#![cfg(feature = "test-helpers")]

mod common;

use amm::SwapKind;
use anchor_litesvm::TestHelpers;
use common::{setup, Pool, Report, Scenario, SwapDir, UserAccounts};

fn k_of(world: &Scenario, pool: &Pool) -> u128 {
    let x = world.ctx.svm.token_balance(&pool.vault_x).unwrap() as u128;
    let y = world.ctx.svm.token_balance(&pool.vault_y).unwrap() as u128;
    x * y
}

fn swap_a_to_b(world: &mut Scenario, pool: &Pool, user: &UserAccounts, amount_in: u64) {
    world.swap(
        user,
        pool,
        SwapKind::ExactInput { amount_in, min_amount_out: 1 },
        SwapDir::AtoB,
    );
}

fn swap_b_to_a(world: &mut Scenario, pool: &Pool, user: &UserAccounts, amount_in: u64) {
    world.swap(
        user,
        pool,
        SwapKind::ExactInput { amount_in, min_amount_out: 1 },
        SwapDir::BtoA,
    );
}

fn withdraw_all(world: &mut Scenario, pool: &Pool, user: &UserAccounts) {
    let lp = world
        .ctx
        .svm
        .token_balance(&user.ata_lp(&pool.mint_lp))
        .unwrap();
    if lp == 0 {
        return;
    }
    world.remove_liquidity(user, pool, lp, 0, 0);
}

/// Record a `step` heading and the two conservation checks for this point in
/// the lifecycle: sum(user X) + vault_x == total X minted, likewise for Y. This
/// replaces the old in-test closure so the invariant lands in the report as a
/// pass/fail line at every step, not just as a silent assertion.
fn record_conservation(
    md: &mut Report,
    world: &mut Scenario,
    pool: &Pool,
    actors: &[&UserAccounts],
    totals: (u64, u64),
    label: &str,
) {
    md.step(label);
    let mut users_x = 0u64;
    let mut users_y = 0u64;
    for a in actors {
        users_x += world.ctx.svm.token_balance(&a.ata_x).unwrap_or(0);
        users_y += world.ctx.svm.token_balance(&a.ata_y).unwrap_or(0);
    }
    let vault_x = world.ctx.svm.token_balance(&pool.vault_x).unwrap_or(0);
    let vault_y = world.ctx.svm.token_balance(&pool.vault_y).unwrap_or(0);
    md.check("X conserved (users + vault == minted)", totals.0, users_x + vault_x);
    md.check("Y conserved (users + vault == minted)", totals.1, users_y + vault_y);
}

/// Full lifecycle: two LPs deposit, two traders swap in both directions,
/// then both LPs withdraw. At each step, the sum of (user balances +
/// vault balances) for each mint must equal the cumulative amount minted
/// to users via `user`. No instruction should create or destroy tokens.
#[test]
fn lifecycle_conserves_tokens_across_users_and_vaults() {
    let mut md = Report::new(
        "Lifecycle: token conservation across deposits, swaps, and withdrawals",
        "Two LPs deposit, two traders swap both directions, then both LPs \
         withdraw. At every step the sum of user balances plus vault balances \
         must equal the total minted to users (X = 18_000, Y = 63_000). No \
         instruction may create or destroy tokens.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    // Two LPs and two traders. Total minted to users via `user`:
    //   X = 10_000 + 5_000 + 2_000 + 1_000 = 18_000
    //   Y = 40_000 + 20_000 + 1_000 + 2_000 = 63_000
    let alice = world.user("Alice", 10_000, 40_000);
    let bob_lp = world.user("BobLP", 5_000, 20_000);
    let carol = world.user("Carol", 2_000, 1_000);
    let dan = world.user("Dan", 1_000, 2_000);
    let actors = [&alice, &bob_lp, &carol, &dan];
    let totals = (18_000u64, 63_000u64);

    record_conservation(&mut md, &mut world, &pool, &actors, totals, "Before any instruction");

    world.deposit(&alice, &pool, 1_000, 4_000, 1);
    record_conservation(&mut md, &mut world, &pool, &actors, totals, "After Alice's deposit");

    swap_a_to_b(&mut world, &pool, &carol, 100);
    record_conservation(&mut md, &mut world, &pool, &actors, totals, "After Carol swaps X→Y");

    world.deposit(&bob_lp, &pool, 500, 2_000, 1);
    record_conservation(&mut md, &mut world, &pool, &actors, totals, "After BobLP's deposit");

    swap_b_to_a(&mut world, &pool, &dan, 200);
    record_conservation(&mut md, &mut world, &pool, &actors, totals, "After Dan swaps Y→X");

    swap_a_to_b(&mut world, &pool, &carol, 50);
    record_conservation(&mut md, &mut world, &pool, &actors, totals, "After Carol's second swap");

    withdraw_all(&mut world, &pool, &alice);
    record_conservation(&mut md, &mut world, &pool, &actors, totals, "After Alice withdraws");

    withdraw_all(&mut world, &pool, &bob_lp);
    record_conservation(&mut md, &mut world, &pool, &actors, totals, "After BobLP withdraws");

    md.snapshot("final pool", &world.observe_pool(&pool));
}

/// After a series of swaps, the constant-product `k` strictly grows
/// (V2 fee accrual). The LP who withdraws after the swaps must receive,
/// in geometric-mean terms, more value than they could have withdrawn
/// immediately after their deposit.
#[test]
fn fees_accrue_to_lp_via_k_growth() {
    let mut md = Report::new(
        "Lifecycle: fees accrue to LPs via k growth",
        "The V2 fee model leaves swap fees in the reserves, so the constant \
         product k = vault_x · vault_y strictly grows across trades. An LP who \
         withdraws after the swaps gets a geometrically larger claim than their \
         share was worth at deposit time. Fees accrue through k, not a separate \
         fee account.",
    );

    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    let alice = world.user("Alice", 10_000, 40_000);
    md.step("Setup: Alice provides liquidity at 1:4 (vaults 1000/4000, k = 4_000_000)");
    world.deposit(&alice, &pool, 1_000, 4_000, 1);

    let k_after_deposit = k_of(&world, &pool);
    md.snapshot("pool after deposit", &world.observe_pool(&pool));
    md.check("k at deposit", 4_000_000u128, k_after_deposit);

    md.step("Action: a trader swaps both directions 5 times, generating fees");
    md.note(
        "Amounts vary each iteration so the txs have distinct signatures \
         (identical-tx replays would be rejected as AlreadyProcessed).",
    );
    let trader = world.user("Trader", 5_000, 20_000);
    for i in 0..5 {
        swap_a_to_b(&mut world, &pool, &trader, 50 + i);
        swap_b_to_a(&mut world, &pool, &trader, 200 + i);
    }

    let k_after_swaps = k_of(&world, &pool);
    md.step("After: k strictly grew (fees stayed in the reserves)");
    md.snapshot("pool after swaps", &world.observe_pool(&pool));
    md.note(format!("k: {k_after_deposit} → {k_after_swaps}"));
    md.check("k strictly grew", true, k_after_swaps > k_after_deposit);

    // Alice's geometric claim at deposit time was sqrt(500 * 2000) = 1_000.
    let alice_x_before_withdraw = world.ctx.svm.token_balance(&alice.ata_x).unwrap();
    let alice_y_before_withdraw = world.ctx.svm.token_balance(&alice.ata_y).unwrap();
    md.step("Action: Alice withdraws her full position");
    withdraw_all(&mut world, &pool, &alice);
    let alice_x_gained =
        world.ctx.svm.token_balance(&alice.ata_x).unwrap() - alice_x_before_withdraw;
    let alice_y_gained =
        world.ctx.svm.token_balance(&alice.ata_y).unwrap() - alice_y_before_withdraw;

    let geometric_claim = ((alice_x_gained as u128) * (alice_y_gained as u128)) as f64;
    let geometric_at_deposit = (500.0_f64) * (2_000.0_f64);
    md.step("After: Alice's geometric claim grew beyond her deposit-time share");
    md.note(format!(
        "withdrawn (X, Y) = ({alice_x_gained}, {alice_y_gained}); geometric \
         claim = {geometric_claim}, vs {geometric_at_deposit} at deposit."
    ));
    md.check("LP's geometric claim grew with fees", true, geometric_claim > geometric_at_deposit);
}
