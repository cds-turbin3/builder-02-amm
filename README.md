# Toy AMM

A constant-product (`x * y = k`) automated market maker for Solana, written in
Anchor. It is a *teaching* AMM: small enough to read in one sitting, but built
the way a real one should be, with the math split out as a pure, exhaustively
tested library and the on-chain program kept to a thin control plane over it.

It is also the largest worked example for
[anchor-litesvm](https://github.com/cds-rs/anchor-litesvm): every instruction is
exercised through a deterministic, snapshot-able test suite, and a couple of the
tests are deliberately written as security proofs-of-concept with classroom
exercises attached.

> Scope: this is a learning artifact, not audited mainnet code. The on-chain
> program id in `Anchor.toml` is a placeholder carried from an earlier project;
> regenerate it (`solana-keygen new -o target/deploy/amm-keypair.json && anchor
> keys sync`) before you deploy anywhere real.

## What it does

Five capabilities, the usual constant-product set plus admin controls:

| Instruction | What it does |
|-------------|--------------|
| `initialize` | Create a pool `Config` PDA for a `(mint_x, mint_y, seed)` triple and its LP mint. |
| `add_liquidity` | Deposit both assets; mint LP tokens proportional to reserves (first deposit uses `sqrt(x*y)`). |
| `remove_liquidity` | Burn LP tokens; withdraw a pro-rata share of both reserves. |
| `swap` | Exact-input or exact-output trade in either direction (`SwapKind`, `a_to_b`). |
| `update_fee` / `set_locked` / `update_authority` | Admin: rotate the fee, pause trading, or renounce authority. |

All math is integer-only with `u128` intermediates, every operation is checked
(no silent wraparound, even in release: `overflow-checks = true`), and rounding
always favors the pool.

## Architecture

Three deliberately decoupled layers. The point of the split is that the
interesting arithmetic is testable without a VM, and the program is small enough
that what's left is just account loading and CPI:

```
┌────────────────────────────────────────────────┐
│  crates/amm-math   (pure functions)            │
│  x*y=k formulas, LP math, checked u128,        │
│  narrow_u64, integer_sqrt_floor. No state.     │
└───────────────────────┬────────────────────────┘
                        │ called by
                        ▼
┌────────────────────────────────────────────────┐
│  programs/amm      (thin Anchor control plane) │
│  loads accounts, calls amm-math, applies the   │
│  resulting deltas, signs the token CPIs.       │
└───────────────────────┬────────────────────────┘
                        │ signs over
                        ▼
┌────────────────────────────────────────────────┐
│  SPL token vaults + LP mint  (asset plane)     │
└────────────────────────────────────────────────┘
```

`amm-math` is the layer worth studying: it keeps token amounts in `u64`, widens
to `u128` only for the arithmetic, and narrows back through a `narrow_u64` that
errors instead of truncating. The constant-product invariant never needs more
than `u128` (every product is `u64 * u64`), so the whole library stays in native
integers. The formulas and rounding policy are specified in
[`docs/toy-amm.spec.md`](docs/toy-amm.spec.md); how the layers fit together is in
[`docs/design.md`](docs/design.md).

## Quick start

Prerequisites: a recent Rust toolchain, the Solana (Agave) CLI, Anchor 1.0, and
[`just`](https://github.com/casey/just).

```sh
# Build the on-chain program (SBF) and run the full suite.
just t

# Same, but with structured logs printed and tests serialized (readable output).
just tt
```

`just t` builds the `.so` first because the tests `include_bytes!` it, then runs
`cargo test --workspace --features amm/test-helpers`. The `test-helpers` feature
is what pulls in anchor-litesvm and the per-instruction bundle types; without it
the on-chain binary stays clean (anchor-litesvm is a host-only, `cfg(not(target_os
= "solana"))` dependency).

## Testing

The suite is built on [anchor-litesvm](https://github.com/cds-rs/anchor-litesvm)
(the `turbin3` branch, Anchor 1.0), and it leans on two patterns that are
documented as much as they're used:

- **Bundle-as-actor**: each instruction gets a typed account-list value
  (`#[derive(Bundle)]` + `BundledPubkeys`), so a test reads `build_ix(bundle,
  instruction::Swap { .. })` instead of hand-ordering a `Vec<AccountMeta>`. See
  [`docs/testing.md`](docs/testing.md).
- **Actors as first-class citizens**: every signer gets a typed identity from a
  seeded registry, so scenarios read in the domain's vocabulary and the keypairs
  are deterministic. The cast-analysis methodology behind it is in
  [`docs/testing/actors-as-first-class-citizens.md`](docs/testing/actors-as-first-class-citizens.md).

Because the actors are seeded, the output is byte-stable across runs. Each
scenario emits a Markdown `Report`, and `just test-md` concatenates them into a
single committed artifact:

```sh
just test-md          # regenerate docs/testing/test-report.md
just publish-report   # the same report, pushed to a public gist
```

A diff in [`docs/testing/test-report.md`](docs/testing/test-report.md) therefore
means a *behavior* change (a CU shift, a different account graph), not just that
the keypairs rolled. The test files live in `programs/amm/tests/`: the happy
paths (`test_initialize`, `test_add_liquidity`, `test_remove_liquidity`,
`test_swap`, `test_lifecycle`, `test_admin`), the edge cases
(`test_edge_cases`), and the security proofs-of-concept below.

## Security exercises

Two of the tests are not happy paths: they reproduce real bugs. They are written
to *pass against the unpatched program*, so a passing test is the evidence the
attack works. [`docs/security/`](docs/security/README.md) tracks each one as a
numbered issue / response / exercise triple:

- `test_inflation_attack` (the classic first-depositor LP-share inflation).
- `test_lock_unlock_attack` ([issue 001](docs/security/issues/001-lock-unlock-timing-attack.md):
  a lock/unlock timing attack).

The exercises present a captured `print_logs_structured()` trace, pose a
question, and hide the walkthrough behind a `<details>` block, so the same PoC
serves as both a regression test and a classroom artifact:

```sh
just poc                     # the lock/unlock attack, with full structured logs
just tt --test test_inflation_attack
```

## Repository layout

```
crates/amm-math/        # pure constant-product math (no Solana deps)
programs/amm/
  src/instructions/     # one module per instruction
  src/{state,constants,error}.rs
  src/test_helpers.rs   # host-only bundle + actor fixtures (feature-gated)
  tests/                # anchor-litesvm integration tests
docs/
  toy-amm.spec.md       # the math spec (formulas, rounding, invariants)
  design.md             # how the layers fit together
  testing.md            # the bundle-as-actor pattern
  testing/              # actor methodology + the generated test report
  security/             # issues, responses, and classroom exercises
```

## Credits

Part of the Turbin3 capstone cohort's worked examples, built on
[anchor-litesvm](https://github.com/cds-rs/anchor-litesvm). If you test your own
program against the framework, the [dogfooding
call](https://github.com/cds-rs/anchor-litesvm/issues/3) is where feedback goes.
