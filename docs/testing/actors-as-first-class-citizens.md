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
`locked`) *and* a trader (swaps). Because `fresh_pool` returns the admin as a
`UserAccounts` with ATAs already in place, the promotion to trader is one call:

```rust
world.mint_to_x(&admin, 200_000);
```

Were the admin a bare `Keypair` instead, that one scenario would pay for the
promotion with ~10 lines of inline `create_associated_token_account` + `mint_to`.
One shared actor type spares every test that cost.

**3. Every instruction has a verb.** Were only some verbed (say `set_locked`,
`deposit`, `fresh_pool`) and the rest built inline as
`world.ctx.program().build_ix(SomeBundle { ... }, instruction::Foo { ... })`,
the 33 scenarios would carry ~80 inline ix-builds, each repeating the
bundle-account list and the args struct, and file-local helpers (`swap_a_to_b`,
`swap_b_to_a`, `withdraw_all` in `test_lifecycle.rs`) would spring up as per-file
patches for the gaps. A full verb set on `Scenario` removes the duplication.

**4. The attacker is a one-line `cast`.** `world.cast(label)` gives a funded,
aliased `UserAccounts` with zero token balance, which matches reality: the
admin-negative tests reject before any transfer, so the ATAs are irrelevant.
Without it, the four admin-negative scenarios would each open with the same
incantation:

```rust
let attacker = world.ctx.svm.create_funded_account(10_000_000_000).unwrap();
world.alias(attacker.pubkey(), "Attacker");
```

## The design

The pieces a test composes, in rough order of how much each one buys you.

### `Scenario`, bound to `world`

A test owns a `Scenario` (`let mut world = setup();`): it holds the SVM context,
the alias table, and the cast. Every test reads `world.<verb>(...)`, so the type
name and the variable name agree, and the term matches the voting sibling repo
and the capstone LOI.

### Actors are `UserAccounts`, label included

Each actor is a `UserAccounts` that carries its own `label`. Helpers read the
label off the `&UserAccounts`, and aliases for derived accounts come out as
`"{actor.label}:<role>"` without the caller re-passing it. A name lives on the
actor, not only in the alias table.

### The admin is an actor like any other

`fresh_pool(fee_bps)` returns `(UserAccounts, Pool)`: the pool authority is a
`UserAccounts` of the same type as every trader, ATAs and all. (Those ATAs cost
two account creations and go unused in all but `lock_unlock_attack`, where the
admin also trades.) Admin-flavored and user-flavored tests read identically.

### The verb set

The verbs take typed actors and register derived state in the alias table:

| Verb | What it does |
| --- | --- |
| `cast(label) -> UserAccounts` | a funded, aliased signer with no token accounts |
| `user(label, x, y) -> UserAccounts` | a funded actor with X and Y balances |
| `fresh_pool(fee_bps) -> (UserAccounts, Pool)` | the admin and an initialized pool |
| `initialize(initializer, pool, fee_bps, authority)` | initialize a pool |
| `deposit(user, pool, a, b, min_lp)` | add liquidity (user-first arg order) |
| `remove_liquidity(user, pool, lp_burn, min_a, min_b)` | withdraw liquidity |
| `swap(user, pool, kind, dir: SwapDir)` | swap in a typed direction |
| `set_locked(admin, pool, locked)` | pause or unpause trading |
| `update_fee(admin, pool, new_fee_bps)` | rotate the fee |
| `update_authority(admin, pool, new_authority: Option<&UserAccounts>)` | rotate or renounce authority |
| `mint_to_x` / `mint_to_y(user, amount)` | fund an actor's token balance |
| `mint_to_vault_x` / `mint_to_vault_y(pool, amount)` | donate directly to a vault (the inflation-attack setup) |

### Swap direction is a typed `SwapDir`

The on-chain `Swap` takes `a_to_b: bool`, a mystery value at a call site:
`world.swap(&bob, &pool, kind, true)` doesn't say which mint goes in and which
comes out. The test API lifts it to an enum:

```rust
pub enum SwapDir { AtoB, BtoA }
```

`SwapDir::AtoB` is "spend X, receive Y"; `SwapDir::BtoA` is the reverse. The verb
converts to the bool at the boundary; the program API is untouched. Call sites
read in the domain:

```rust
world.swap_expecting(&bob, &pool, kind, SwapDir::AtoB, "PoolLocked");
```

A bool at the API surface that encodes a *direction* or a *mode* is a smell; an
enum earns its keep the moment two call sites exist.

### Negative-path verbs

Each happy-path verb has an `_expecting(..., error)` companion. The error string
matches as a substring against both the transaction logs and the error field
(the same matcher `anchor_litesvm`'s `send_err_named` uses), so one signature
accepts Anchor names like `"PoolLocked"` and System messages like
`"already in use"`.

The `_expecting` verbs return `TransactionResult` so a test can inspect fees,
logs, or compute units. The inflation-attack test relies on this, asserting that
Henry's lamport delta equals the tx fee exactly (nothing else charged his
lamports):

```rust
let r = world.deposit_expecting(&henry, &pool, 1_000, 1_000, 0, "InsufficientLiquidity");
let fee = r.inner().fee;
assert_eq!(henry_lamports_before - henry_lamports_after, fee, ...);
```

Tests that don't need the result ignore it.

### The escape hatch: `s.alias` and `s.ctx`

Two negative tests exercise failures that are *by construction* off the verbs'
natural derivation path:

- the voting program's cross-wired-PDA test (sibling repo);
- the AMM's `lock_unlock_attack`, which packs unlock + swap + relock into one
  atomic transaction. The verbs send one instruction per tx; the attack depends
  on atomicity, so the test drops to
  `world.ctx.svm.send_instructions(&[unlock_ix, admin_swap_ix, relock_ix], ...)`
  with the three instructions built directly.

`s.alias(pubkey, label)` is public precisely so these tests keep their
off-pattern accounts named in the structured logs. The escape hatch is the right
shape for any test whose *point* is violating the invariants the verbs encode.

## Worked example: `admin_atomically_unlocks_swaps_and_relocks_while_users_blocked`

The scenario where the admin both flips the lock and trades shows the verbs and
the one escape hatch in the same test:

```rust
fn admin_atomically_unlocks_swaps_and_relocks_while_users_blocked() {
    let mut world = setup();
    let (admin, pool) = world.fresh_pool(30);

    let alice = world.user("Alice", 1_000_000, 1_000_000);
    world.deposit(&alice, &pool, 1_000_000, 1_000_000, 1);

    // Promote the admin to trader: one line. ATAs came from fresh_pool.
    world.mint_to_x(&admin, 200_000);

    let bob = world.user("Bob", 100_000, 0);

    // Happy-path and negative-path verbs.
    world.set_locked(&admin, &pool, true);
    world.swap_expecting(&bob, &pool,
        SwapKind::ExactInput { amount_in: 10_000, min_amount_out: 1 },
        SwapDir::AtoB, "PoolLocked");

    // The escape hatch: the attack needs all three instructions in one atomic
    // tx, so it drops below the verbs to send_instructions, the ix's built directly.
    let unlock_ix = world.ctx.program().build_ix(/* ... */);
    let admin_swap_ix = world.ctx.program().build_ix(/* ... */);
    let relock_ix = world.ctx.program().build_ix(/* ... */);
    world.ctx.svm
        .send_instructions(&[unlock_ix, admin_swap_ix, relock_ix], &[&admin.signer])
        .unwrap()
        .print_logs_structured(&world.aliases)
        .assert_success();

    // Bob is still blocked on the far side of the window.
    world.swap_expecting(&bob, &pool,
        SwapKind::ExactInput { amount_in: 5_000, min_amount_out: 1 },
        SwapDir::AtoB, "PoolLocked");
}
```

Every beat is a single-verb scenario sentence: `world.set_locked(...)`,
`world.swap_expecting(...)`, `world.mint_to_x(...)`. The only inline construction
is the part that *intrinsically* needs the lower-level API: the atomic
three-instruction bundle, which has to land as one transaction. The narrative is
the test.

## What a test holds in its head

The plumbing a test would otherwise repeat (instruction construction, the send
with its signer slice and aliases, alias registration, the log-print call) lives
once in `tests/common/mod.rs`, which is the larger half of the suite by line
count. What remains in a test file reads as *scenario sentences*: one verb per
beat, actors and pools as named typed values, errors named at the call site.

So a reader of any one test holds four things:

1. the cast (one or two `let alice = world.user(...)` lines);
2. the pool (one `let (admin, pool) = world.fresh_pool(30)` line);
3. the beats (one verb per action, in order);
4. the assertions.

and not: the bundle's field layout per instruction, the order of accounts in the
signer slice, whether the right aliases were registered before the send, whether
the log-print call was added. The lower-level API surfaces in exactly two places,
both by design: the `lock_unlock_attack` atomic bundle (which must be one
transaction) and `test_initialize.rs` (which pins per-field initialization and so
works below `fresh_pool`'s auto-aliasing).

`test_admin.rs::update_fee_changes_fee_bps` is the floor of the form: cast, pool,
verb, assert, held in one glance.

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

## How this pattern scales

The same methodology (cast-list table, identify what's already first-class, design
what isn't) produces a design sized to the program's complexity:

- Voting (3 instructions, 12 scenarios): one `Actor` type (signer + label) and a
  small `Scenario` API. See
  `voting/docs/testing/actors-as-first-class-citizens.md`.
- AMM (8 instructions, 33 scenarios): `UserAccounts` (signer + label + 2 ATAs) and
  a fuller verb set with `_expecting` variants.

The open direction (capstone-sized) is generalizing the testing-side abstractions
so other Anchor programs adopt the pattern without rebuilding the `Scenario` shell
per repo: the user-guide deliverable in Part D of the LOI, of which this is one of
three reference patterns.
