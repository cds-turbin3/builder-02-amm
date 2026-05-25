# Actors as first-class citizens (and how to find them)

A companion to [`docs/testing.md`](../testing.md): that doc explains the
*bundle-as-actor* pattern (typed account-list values per instruction);
this one explains the cast-analysis methodology that surfaces what the
*signer-actor* type should look like, and applies the result to this
codebase's test suite. The two patterns compose. The bundle gives every
*instruction* a typed account list; the actor gives every *signer* a
typed identity. Tests read in the domain's vocabulary at both ends.

The thesis comes from the
[Q2 2026 capstone LOI](../../../capstone-loi.md). This doc is the local
application of that thesis to the AMM's test suite, with a sibling
application living in the voting program's repo.

## Methodology: name the cast before designing the type

The trap is designing the actor type up front and forcing every scenario
through it. The cast tells you what shape the type should take. The
recipe:

1. List every test scenario by name.
2. For each, name the **signers** and the **role each plays in the
   scenario**.
3. Separately, name the **subjects**: domain entities that appear but
   don't sign. (For the AMM, the mints, the pool, the candidate strings
   from the voting analogue: anything that exists in the scenario as a
   target rather than as a participant.)
4. Tabulate. Look for the cross-scenario shape.

The patterns to hunt for:

- **How often roles split.** If every scenario collapses every role onto
  a single signer, a role-typed hierarchy is ceremony for no payoff. If
  many scenarios split, role-typing might earn its keep.
- **How labels drift.** Across scenarios, does "the admin" go by
  `admin`, `Admin`, `signer_admin`, `"Admin for registering Candidate"`?
  Every drift is a place where the alias map and the variable name
  disagree.
- **What a role uniquely needs.** What state does this role carry that
  others don't? Is it enough to justify a separate type, or is it a
  method on the shared one?
- **Where subjects masquerade as actors.** Strings hashed into PDAs are
  not actors; lumping them with signers creates bugs.

Apply the recipe with no preconceived answer. The answer falls out of
the table.

## Walking this repo's cast

Nine test files, thirty-three scenarios. Signers are listed with their
narrative role; subjects (mints, pool PDAs, vaults) are constant across
the whole suite and live on the `Scenario`, not per-test.

### `test_initialize.rs`

| Scenario | Signers (role) |
| --- | --- |
| `initialize_creates_config_lp_mint_and_vaults` | `admin` (PoolCreator + Authority) |
| `initialize_rejects_invalid_fee_at_denominator` | `admin` (PoolCreator) |

### `test_add_liquidity.rs`

| Scenario | Signers (role) |
| --- | --- |
| `first_deposit_mints_to_user_and_locks_minimum_liquidity` | `alice` (LP) |
| `subsequent_deposit_uses_floor_min_formula` | `alice`, `bob` (LPs) |
| `add_liquidity_rejects_when_lp_below_min` | `alice` (LP) |
| `add_liquidity_rejects_when_pool_locked` | `admin` (Authority), `alice` (LP) |

### `test_remove_liquidity.rs`

| Scenario | Signers (role) |
| --- | --- |
| `remove_returns_proportional_shares_and_leaves_lock_vault_intact` | `alice` (LP) |
| `remove_liquidity_rejects_when_amount_below_min` | `alice` (LP) |
| `remove_liquidity_rejects_when_pool_locked` | `admin` (Authority), `alice` (LP) |

### `test_swap.rs`

| Scenario | Signers (role) |
| --- | --- |
| `exact_input_swap_a_to_b_moves_balances_and_grows_k` | `lp` (LP), `bob` (Trader) |
| `exact_output_swap_a_to_b_pays_calculated_input` | `lp` (LP), `bob` (Trader) |
| `exact_input_swap_b_to_a_picks_reserves_in_reverse` | `lp` (LP), `bob` (Trader) |
| `exact_input_swap_rejects_when_amount_out_below_min` | `lp` (LP), `bob` (Trader) |
| `exact_output_swap_rejects_when_amount_in_above_max` | `lp` (LP), `bob` (Trader) |

### `test_admin.rs`

| Scenario | Signers (role) |
| --- | --- |
| `update_fee_changes_fee_bps` | `admin` (Authority) |
| `set_locked_flips_locked_field` | `admin` (Authority) |
| `update_authority_renounce_then_admin_calls_fail` | `admin` (Authority, then ex-Authority) |
| `unauthorized_signer_cannot_update_fee` | `admin`, `attacker` |
| `unauthorized_signer_cannot_set_locked` | `admin`, `attacker` |
| `set_locked_after_renounce_fails` | `admin` |
| `unauthorized_signer_cannot_update_authority` | `admin`, `attacker` |
| `update_authority_after_renounce_fails` | `admin` |
| `update_fee_propagates_to_next_swap` | `admin`, `alice` (LP), `bob` (Trader) |
| `update_authority_rotation_transfers_admin_privilege` | `alice_admin`, `bob_admin` (two Authorities, mid-rotation) |
| `update_fee_rejects_invalid_fee_at_denominator` | `admin` |

### `test_inflation_attack.rs`

| Scenario | Signers (role) |
| --- | --- |
| `first_deposit_at_or_below_minimum_liquidity_rejects` | `alice` (LP) |
| `minimal_viable_first_deposit_succeeds_just_above_threshold` | `alice` (LP) |
| `inflation_attack_via_donation_leaves_honest_depositor_unharmed` | `mallory` (Attacker-LP), `henry` (HonestLP) |

### `test_edge_cases.rs`

| Scenario | Signers (role) |
| --- | --- |
| `swap_with_truncated_amount_in_returns_insufficient_output` | `alice` (LP), `bob` (Trader) |
| `drain_to_minimum_liquidity_preserves_lock_vault_and_reserves` | `alice` (LP), `bob` (LP) |

### `test_lifecycle.rs`

| Scenario | Signers (role) |
| --- | --- |
| `lifecycle_conserves_tokens_across_users_and_vaults` | `alice`, `bob_lp` (LPs), `carol`, `dan` (Traders) |
| `fees_accrue_to_lp_via_k_growth` | `alice` (LP), `trader` (Trader) |

### `test_lock_unlock_attack.rs`

| Scenario | Signers (role) |
| --- | --- |
| `admin_atomically_unlocks_swaps_and_relocks_while_users_blocked` | `admin` (Authority *and* Trader), `alice` (LP), `bob` (HonestTrader) |

### Reading the table

Four observations, in order of how much they shaped the design:

**1. Roles split frequently, but the underlying *type* doesn't need
to.** Of 33 scenarios, ~half have two or more distinct signer-actors.
But the splitting is by *narrative role*, not by behavioral type. LPs
deposit and withdraw; traders swap; admins set lock and update fees;
attackers call admin instructions and get rejected. Each role does
something different, but they all *are* the same kind of thing: a
funded signer with a label and two token ATAs. A role-typed hierarchy
(`Admin`, `Trader`, `LP`, `Attacker`) would force every verb to decide
which input to accept, and every test to declare the right type up
front. The savings are negative.

**2. The admin-as-trader case is the loudest signal.** In
`test_lock_unlock_attack.rs`, the admin is both the authority (flips
locked) *and* a trader (swaps). Under the original design, the admin
came back from `fresh_pool` as a raw `Keypair`, with no ATAs. The test
paid for the promotion to trader with ~10 lines of inline
`create_associated_token_account` + `mint_to`. After the migration,
`fresh_pool` returns the admin as a `UserAccounts` with ATAs already in
place; the promotion is one call:

```rust
world.mint_to_x(&admin, 200_000);
```

One scenario carries the cost-of-not-unifying for the whole suite. The
unification pays it back.

**3. Instructions were hand-built more often than verbed.** Pre-migration
the suite had verbs for `set_locked`, `deposit`, and `fresh_pool`. Every
other instruction (`swap`, `remove_liquidity`, `update_fee`,
`update_authority`, `initialize`) was constructed inline as
`world.ctx.program().build_ix(SomeBundle { ... }, instruction::Foo { ... })`.
Across 33 scenarios that's ~80 inline ix-builds, each repeating the
bundle-account list and the args struct. The file-local helpers
(`swap_a_to_b`, `swap_b_to_a`, `withdraw_all`) in `test_lifecycle.rs`
were the signal: someone was already patching the missing verbs
per-file. Filling out the verb set on `Scenario` collapses the
duplication.

**4. The `Attacker` was a four-line incantation, repeated.** Four
admin-negative scenarios opened with

```rust
let attacker = world.ctx.svm.create_funded_account(10_000_000_000).unwrap();
world.alias(attacker.pubkey(), "Attacker");
```

The `world.cast(label)` verb (lifted from voting) collapses this to one
line. The attacker is a `UserAccounts` with zero token balance, which
matches reality: the test rejects before any transfer, so the ATAs are
irrelevant.

## What changed in the design

In rough order of leverage:

### a. `Bootstrap` renamed to `Scenario`

The type was already used as `world` in every test (`let mut world =
setup();`). The type/variable disagreement was a small papercut every
time you read a test. The rename aligns the type name with both the
voting sibling repo and the LOI's terminology.

### b. `UserAccounts` carries `label: String`

The label moves from being known only to the alias map (write-once,
read-never from the test's perspective) onto the actor itself. Helpers
that need the label can read it from the `&UserAccounts`; aliases for
derived accounts can be auto-generated as `"{actor.label}:<role>"`
without re-passing the label.

### c. Admin returned as `UserAccounts`, not `Keypair`

`fresh_pool` now hands back `(UserAccounts, Pool)`. Every admin-flavored
test has the same type as every user-flavored test. ATAs are created
for the admin too (cheap, two account creations); they're unused in 10
of 11 admin scenarios and load-bearing in the one (`lock_unlock_attack`)
that needs them.

### d. The verb set on `Scenario`

The full set, taking typed actors and registering derived state in the
alias table:

| Verb | Replaces |
| --- | --- |
| `cast(label) -> UserAccounts` | `create_funded_account` + `alias` |
| `user(label, x, y) -> UserAccounts` | the old `make_user` with explicit SOL |
| `fresh_pool(fee_bps) -> (UserAccounts, Pool)` | unchanged at the call site; admin's type changed |
| `initialize(initializer, pool, fee_bps, authority)` | inline `InitializeBundle` builds in `test_initialize.rs` |
| `deposit(user, pool, a, b, min_lp)` | unchanged at the call site; arg order is user-first now |
| `remove_liquidity(user, pool, lp_burn, min_a, min_b)` | inline `RemoveLiquidityBundle` builds |
| `swap(user, pool, kind, a_to_b)` | inline `SwapBundle` builds (~15 callsites) |
| `set_locked(admin, pool, locked)` | unchanged at the call site; admin's type changed |
| `update_fee(admin, pool, new_fee_bps)` | inline `UpdateFeeBundle` builds |
| `update_authority(admin, pool, new_authority: Option<&UserAccounts>)` | inline `UpdateAuthorityBundle` builds |
| `mint_to_x` / `mint_to_y(user, amount)` | inline `mint_to` for the admin-as-trader promotion |
| `mint_to_vault_x` / `mint_to_vault_y(pool, amount)` | inline `mint_to` for the inflation-attack donation |

### e. Negative-path verbs

Each happy-path verb has an `_expecting(..., error)` companion. The
error string is matched as a substring against both the transaction
logs and the error field (same matcher `anchor_litesvm`'s
`send_err_named` uses), so one signature accepts Anchor names like
`"PoolLocked"` and System messages like `"already in use"`.

The `_expecting` verbs return `TransactionResult` (rather than unit) so
tests that want to inspect fees, logs, or compute units can do so. The
inflation-attack test relies on this: it asserts that Henry's lamport
delta equals the tx fee exactly (no other on-chain effect should have
charged his lamports), which requires reading `r.inner().fee`.

```rust
let r = world.deposit_expecting(&henry, &pool, 1_000, 1_000, 0, "InsufficientLiquidity");
let fee = r.inner().fee;
assert_eq!(henry_lamports_before - henry_lamports_after, fee, ...);
```

Tests that don't need the result can ignore it.

### f. The escape hatch: `s.alias` and `s.ctx`

Two negative tests exercise failures that are *by construction* off the
verb's natural derivation path:

- The voting program's cross-wired-PDA test (sibling repo).
- The AMM's `lock_unlock_attack` test, which packs three instructions
  (unlock + swap + relock) into one atomic transaction. The verbs on
  `Scenario` send one instruction per tx; the attack depends on
  atomicity, so the test drops to
  `world.ctx.svm.send_instructions(&[unlock_ix, admin_swap_ix, relock_ix], ...)`
  with the three ix's built directly.

`s.alias(pubkey, label)` is exposed publicly precisely so these tests
can keep their off-pattern accounts named in the structured log
output. The escape hatch is the right pattern for any test whose
*point* is violating the invariants the verbs encode.

## Worked example: `admin_atomically_unlocks_swaps_and_relocks_while_users_blocked`

The single scenario where the admin both flips the lock and trades is
the cleanest before/after, because it's where the admin-as-`UserAccounts`
change pays off most visibly.

### Before

```rust
fn admin_atomically_unlocks_swaps_and_relocks_while_users_blocked() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);          // admin: Keypair

    let alice = world.make_user("Alice", 10_000_000_000, 1_000_000, 1_000_000);
    world.deposit(&pool, &alice, 1_000_000, 1_000_000, 1);

    // Promote admin to trader: ten lines of manual ATA setup.
    let admin_ata_x = world.ctx.svm
        .create_associated_token_account(&world.mint_x, &admin).unwrap();
    let admin_ata_y = world.ctx.svm
        .create_associated_token_account(&world.mint_y, &admin).unwrap();
    world.ctx.svm.mint_to(&world.mint_x, &admin_ata_x, &world.mint_authority, 200_000)
        .unwrap();

    let bob = world.make_user("Bob", 10_000_000_000, 100_000, 0);

    // Step 1: inline ix construction for set_locked
    let lock_ix = world.ctx.program().build_ix(
        SetLockedBundle { authority: admin.pubkey(), config: pool.config },
        amm::instruction::SetLocked { locked: true },
    );
    world.ctx.svm.send_ok(lock_ix, &[&admin], &world.aliases)
        .print_logs_structured(&world.aliases);

    // Step 2: inline ix construction for bob's blocked swap
    let bob_swap = world.ctx.program().build_ix(
        pool.swap_bundle(&bob),
        amm::instruction::Swap { /* ... */ },
    );
    world.ctx.svm.send_err(bob_swap, &[&bob.signer], &world.aliases)
        .print_logs_structured(&world.aliases);

    // ... and so on
}
```

### After

```rust
fn admin_atomically_unlocks_swaps_and_relocks_while_users_blocked() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);          // admin: UserAccounts

    let alice = world.user("Alice", 1_000_000, 1_000_000);
    world.deposit(&alice, &pool, 1_000_000, 1_000_000, 1);

    // Promote admin to trader: one line. ATAs were created by fresh_pool.
    world.mint_to_x(&admin, 200_000);

    let bob = world.user("Bob", 100_000, 0);

    // Step 1: verb
    world.set_locked(&admin, &pool, true);

    // Step 2: negative-path verb with named error
    world.swap_expecting(&bob, &pool,
        SwapKind::ExactInput { amount_in: 10_000, min_amount_out: 1 },
        true, "PoolLocked");

    // Step 3: drop to lower-level send_instructions for the atomic-bundle attack
    // (only this part stays inline; the rest of the test is verbs)
    let unlock_ix = world.ctx.program().build_ix(/* ... */);
    let admin_swap_ix = world.ctx.program().build_ix(/* ... */);
    let relock_ix = world.ctx.program().build_ix(/* ... */);
    world.ctx.svm
        .send_instructions(&[unlock_ix, admin_swap_ix, relock_ix], &[&admin.signer])
        .unwrap()
        .print_logs_structured(&world.aliases)
        .assert_success();

    // ... bob still can't swap (verb again)
    world.swap_expecting(&bob, &pool,
        SwapKind::ExactInput { amount_in: 5_000, min_amount_out: 1 },
        true, "PoolLocked");
}
```

The test is shorter, the noise-to-signal ratio is better, and the only
inline construction left is the part that *intrinsically* needs the
lower-level API (the atomic three-instruction bundle). Every other beat
is a single-verb scenario sentence: `world.set_locked(...)`,
`world.swap_expecting(...)`, `world.mint_to_x(...)`. The narrative is
the test.

## Size and cognition: did it actually pay off?

Worth checking against the diff, not just the worked example. Two
numbers matter: how much line-count moved around, and how much
*inline noise* the tests carry (the parts that don't read as the
domain's verbs).

### Line counts

| File | Before | After | Delta |
| --- | ---: | ---: | ---: |
| `test_add_liquidity.rs` | 162 | 118 | -44 |
| `test_admin.rs` | 403 | 208 | **-195** |
| `test_edge_cases.rs` | 117 | 100 | -17 |
| `test_inflation_attack.rs` | 213 | 163 | -50 |
| `test_initialize.rs` | 101 | 58 | -43 |
| `test_lifecycle.rs` | 209 | 185 | -24 |
| `test_lock_unlock_attack.rs` | 208 | 172 | -36 |
| `test_remove_liquidity.rs` | 118 | 82 | -36 |
| `test_swap.rs` | 195 | 166 | -29 |
| **Tests subtotal** | **1726** | **1252** | **-474 (-27%)** |
| `tests/common/mod.rs` | 297 | 648 | +351 |
| **Tests + common** | **2023** | **1900** | -123 (-6%) |

The tests shrank by 474 lines (27%); the shared scaffolding grew by
351 lines (more than doubled). Net: -123 lines (-6%). Modest as a raw
LOC delta, but the more interesting number is *which* lines moved.

### Inline-noise markers

Four phrases that mean "this is plumbing, not scenario": the inline
ix construction (`program().build_ix(...)`), the inline send
(`send_ok` / `send_err` / `send_err_named` / `send_instructions`), the
inline alias updates (`world.alias(...)` and `aliases.with(...)` in
test code), and the inline log-printing (`print_logs_structured(...)`).
Counted across the 9 test files only (not common):

| Marker | Before | After | Delta |
| --- | ---: | ---: | ---: |
| `program().build_ix(...)` | 46 | 4 | -42 |
| `.send_ok` / `.send_err*` / `.send_instructions` | 44 | 1 | -43 |
| `world.alias` / `aliases.with` | 8 | 2 | -6 |
| `.print_logs_structured` | 44 | 1 | -43 |

The remaining inline noise lives in exactly two places:

- The four `build_ix`, the one `send_instructions`, and the one
  `print_logs_structured` are all in `test_lock_unlock_attack.rs`,
  building the three-instruction atomic bundle that *intrinsically*
  requires the lower-level API (the verbs send one ix per tx; the
  attack depends on atomicity).
- The two `alias` calls are in `test_initialize.rs`, which exercises
  the lower-level `initialize` path (without `fresh_pool`'s
  auto-aliasing) so the test can pin per-field initialization
  semantics.

Both are exactly the escape-hatch shape the design is meant to
preserve: tests whose *point* is violating the verbs' assumptions step
below the verbs deliberately.

### What the lines represent

The 474-line tests shrinkage is not "code golf." It's specifically the
removal of repeated patterns the reader had to mentally compile every
time:

- The 42 vanished `build_ix` calls were, on average, ~8 lines each
  (bundle struct + args struct + closing). That's roughly 330 lines of
  cargo-cult typing the reader had to verify each time: did `authority`
  match the right signer? Did `config` match the pool? Did the args
  pass match the args declared?
- The 43 vanished `send_ok` / `send_err` calls each chained a signer
  slice and an aliases reference. A reader who saw `&[&alice.signer]`
  had to confirm that alice was the same signer as the bundle's
  `user`, every time.
- The 43 vanished `print_logs_structured` calls were *line noise*: the
  test author wanted log output, but the call had to be re-added on
  every send, and a missed one silently lost diagnostic value.

What's left in the tests is closer to *scenario sentences*: one verb
per beat, actors and pools as named typed values, errors named at the
call site. The 351 lines added to `tests/common/mod.rs` are paid
once and amortized across every scenario; the 474 lines removed from
test files are paid back every time a reader scans a test for what
it's *about*.

### Net cognitive cost

The migration shifts complexity from the *use-site* (33 places) to the
*definition-site* (1 place). Same total complexity in some sense, but
unevenly distributed: the use-site readings happen ~33× more often
than the definition-site readings, so the trade is favourable. The
reader of any one test now has to hold in their head:

1. The cast (one or two `let alice = world.user(...)` lines per test).
2. The pool (one `let (admin, pool) = world.fresh_pool(30)` line).
3. The beats (one verb per action, in order).
4. The assertions (unchanged).

What they no longer have to hold: the bundle's field layout per
instruction; the order of accounts in the signer slice; whether the
right aliases were registered before the send; whether the log-print
call was added or forgotten.

The 6-line `test_admin.rs::update_fee_changes_fee_bps` (down from 22)
is the upper bound of this kind of test: cast, pool, verb, assert. A
reader holds it in one glance.

## What's deliberately left out

- **A `Voter` / `Admin` / `Trader` trait split.** The cast analysis
  says no. Every signer-actor in this suite is a funded signer with a
  label and two ATAs; the role lives in the variable name and the
  verbs called on it.
- **A `population(n, prefix)` helper.** Only `test_lifecycle.rs` has
  more than two users, and it names them individually (`alice`,
  `bob_lp`, `carol`, `dan`) on purpose. The narrative is what makes
  the conservation assertion legible.
- **A `MintAuthority` actor type.** The mint authority lives on
  `Scenario` itself, used internally by `cast` / `user` /
  `mint_to_x` / etc. It's a system actor, not a participant; promoting
  it to `UserAccounts` would add type machinery for no narrative gain.
- **A `pool` parameter on `UserAccounts` to bake in the LP ATA.** The
  `user.ata_lp(&pool.mint_lp)` method is fine; baking the pool into
  the struct would couple actor construction to pool construction,
  which is exactly what the current shape is designed to avoid.

## Where this pattern goes next

The voting-program sibling repo went through the same migration first
(documented in `voting/docs/testing/actors-as-first-class-citizens.md`).
The methodology (cast-list table → identify what's already first-class
→ design what isn't) is the same; the design that fell out differs in
proportion to the program's complexity:

- Voting has 3 instructions and 12 scenarios; the design is one `Actor`
  type (signer + label) and a small `Scenario` API.
- AMM has 8 instructions and 33 scenarios; the design is `UserAccounts`
  (signer + label + 2 ATAs) and a fuller verb set with `_expecting`
  variants.

Both fall out of the same recipe. The next step (capstone-sized) is to
generalize the testing-side abstractions enough that other Anchor
programs can adopt the pattern without reinventing the `Scenario` shell
per repo. That's the user-guide deliverable in Part D of the LOI; this
doc is one of its three reference patterns.
