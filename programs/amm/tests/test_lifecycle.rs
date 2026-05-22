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

#![cfg(feature = "test-helpers")]

mod common;

use amm::{RemoveLiquidityBundle, SwapBundle, SwapKind};
use anchor_litesvm::{Aliases, TestHelpers, TransactionHelpers};
use common::{setup, Pool, UserAccounts};

fn k_of(world: &common::Bootstrap, pool: &Pool) -> u128 {
    let x = world.ctx.svm.token_balance(&pool.vault_x).unwrap() as u128;
    let y = world.ctx.svm.token_balance(&pool.vault_y).unwrap() as u128;
    x * y
}

fn swap_a_to_b(world: &mut common::Bootstrap, pool: &Pool, user: &UserAccounts, amount_in: u64) {
    let ix = world.ctx.program().build_ix(
        pool.swap_bundle(user),
        amm::instruction::Swap {
            kind: SwapKind::ExactInput {
                amount_in,
                min_amount_out: 1,
            },
            a_to_b: true,
        },
    );
    world
        .ctx
        .svm
        .send_ok(ix, &[&user.signer])
        .print_logs_structured(&Aliases::default());
}

fn swap_b_to_a(world: &mut common::Bootstrap, pool: &Pool, user: &UserAccounts, amount_in: u64) {
    let ix = world.ctx.program().build_ix(
        SwapBundle {
            user: user.pubkey(),
            mint_x: pool.mint_x,
            mint_y: pool.mint_y,
            config: pool.config,
            vault_x: pool.vault_x,
            vault_y: pool.vault_y,
            user_x: user.ata_x,
            user_y: user.ata_y,
        },
        amm::instruction::Swap {
            kind: SwapKind::ExactInput {
                amount_in,
                min_amount_out: 1,
            },
            a_to_b: false,
        },
    );
    world
        .ctx
        .svm
        .send_ok(ix, &[&user.signer])
        .print_logs_structured(&Aliases::default());
}

fn withdraw_all(world: &mut common::Bootstrap, pool: &Pool, user: &UserAccounts) {
    let lp = world
        .ctx
        .svm
        .token_balance(&user.ata_lp(&pool.mint_lp))
        .unwrap();
    if lp == 0 {
        return;
    }
    let ix = world.ctx.program().build_ix(
        RemoveLiquidityBundle {
            user: user.pubkey(),
            mint_x: pool.mint_x,
            mint_y: pool.mint_y,
            config: pool.config,
            mint_lp: pool.mint_lp,
            vault_x: pool.vault_x,
            vault_y: pool.vault_y,
            user_x: user.ata_x,
            user_y: user.ata_y,
            user_lp: user.ata_lp(&pool.mint_lp),
        },
        amm::instruction::RemoveLiquidity {
            lp_burn: lp,
            min_a: 0,
            min_b: 0,
        },
    );
    world
        .ctx
        .svm
        .send_ok(ix, &[&user.signer])
        .print_logs_structured(&Aliases::default());
}

/// Full lifecycle: two LPs deposit, two traders swap in both directions,
/// then both LPs withdraw. At each step, the sum of (user balances +
/// vault balances) for each mint must equal the cumulative amount minted
/// to users via `make_user`. No instruction should create or destroy tokens.
#[test]
fn lifecycle_conserves_tokens_across_users_and_vaults() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    // Two LPs and two traders. Total minted to users via make_user:
    //   X = 10_000 (alice) + 5_000 (bob_lp) + 2_000 (carol) + 1_000 (dan) = 18_000
    //   Y = 40_000 (alice) + 20_000 (bob_lp) + 1_000 (carol) + 2_000 (dan) = 63_000
    let alice = world.make_user(10_000_000_000, 10_000, 40_000);
    let bob_lp = world.make_user(10_000_000_000, 5_000, 20_000);
    let carol = world.make_user(10_000_000_000, 2_000, 1_000);
    let dan = world.make_user(10_000_000_000, 1_000, 2_000);
    let total_x_minted: u64 = 18_000;
    let total_y_minted: u64 = 63_000;

    let assert_conservation = |world: &common::Bootstrap, label: &str| {
        let bal = |ata| world.ctx.svm.token_balance(ata).unwrap_or(0);
        let users_x = bal(&alice.ata_x) + bal(&bob_lp.ata_x) + bal(&carol.ata_x) + bal(&dan.ata_x);
        let users_y = bal(&alice.ata_y) + bal(&bob_lp.ata_y) + bal(&carol.ata_y) + bal(&dan.ata_y);
        let vault_x = bal(&pool.vault_x);
        let vault_y = bal(&pool.vault_y);
        assert_eq!(
            users_x + vault_x,
            total_x_minted,
            "{}: X conservation broken",
            label
        );
        assert_eq!(
            users_y + vault_y,
            total_y_minted,
            "{}: Y conservation broken",
            label
        );
    };

    assert_conservation(&world, "before any instruction");

    world.deposit(&pool, &alice, 1_000, 4_000, 1);
    assert_conservation(&world, "after alice's deposit");

    swap_a_to_b(&mut world, &pool, &carol, 100);
    assert_conservation(&world, "after carol swaps X->Y");

    world.deposit(&pool, &bob_lp, 500, 2_000, 1);
    assert_conservation(&world, "after bob_lp's deposit");

    swap_b_to_a(&mut world, &pool, &dan, 200);
    assert_conservation(&world, "after dan swaps Y->X");

    swap_a_to_b(&mut world, &pool, &carol, 50);
    assert_conservation(&world, "after carol's second swap");

    withdraw_all(&mut world, &pool, &alice);
    assert_conservation(&world, "after alice withdraws");

    withdraw_all(&mut world, &pool, &bob_lp);
    assert_conservation(&world, "after bob_lp withdraws");
}

/// After a series of swaps, the constant-product `k` strictly grows
/// (V2 fee accrual). The LP who withdraws after the swaps must receive,
/// in geometric-mean terms, more value than they could have withdrawn
/// immediately after their deposit. This is the on-chain demonstration
/// that fees accrue to LPs through `k` growth, not via a separate fee
/// account.
#[test]
fn fees_accrue_to_lp_via_k_growth() {
    let mut world = setup();
    let (_admin, pool) = world.fresh_pool(30);

    // Alice provides liquidity at a 1:4 ratio. After this:
    //   vaults = (1_000, 4_000); k_initial = 4_000_000
    //   alice has 1_000 LP; lp_vault has 1_000 LP; supply = 2_000
    let alice = world.make_user(10_000_000_000, 10_000, 40_000);
    world.deposit(&pool, &alice, 1_000, 4_000, 1);

    let k_after_deposit = k_of(&world, &pool);
    assert_eq!(k_after_deposit, 4_000_000);

    // Several traders swap in both directions, generating fees. Vary the
    // amounts each iteration so the resulting txs have distinct signatures
    // (identical-tx replays would be rejected as AlreadyProcessed).
    let trader = world.make_user(10_000_000_000, 5_000, 20_000);
    for i in 0..5 {
        swap_a_to_b(&mut world, &pool, &trader, 50 + i);
        swap_b_to_a(&mut world, &pool, &trader, 200 + i);
    }

    let k_after_swaps = k_of(&world, &pool);
    assert!(
        k_after_swaps > k_after_deposit,
        "fees should grow k; got k_initial={} k_post={}",
        k_after_deposit,
        k_after_swaps
    );

    // Alice's geometric claim at deposit time was sqrt(500 * 2000) = 1_000
    // (she owns half the pool: her 1_000 LP out of supply 2_000).
    // After fees accrue, her pro-rata share of the post-swap reserves
    // should be geometrically larger than 1_000.
    let alice_x_before_withdraw = world.ctx.svm.token_balance(&alice.ata_x).unwrap();
    let alice_y_before_withdraw = world.ctx.svm.token_balance(&alice.ata_y).unwrap();
    withdraw_all(&mut world, &pool, &alice);
    let alice_x_gained =
        world.ctx.svm.token_balance(&alice.ata_x).unwrap() - alice_x_before_withdraw;
    let alice_y_gained =
        world.ctx.svm.token_balance(&alice.ata_y).unwrap() - alice_y_before_withdraw;

    let geometric_claim = ((alice_x_gained as u128) * (alice_y_gained as u128)) as f64;
    let geometric_at_deposit = (500.0_f64) * (2_000.0_f64);
    assert!(
        geometric_claim > geometric_at_deposit,
        "LP's geometric claim should grow with fees: deposit-share = {}, post-fee share = {}",
        geometric_at_deposit,
        geometric_claim
    );
}
