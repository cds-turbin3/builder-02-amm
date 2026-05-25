# Trace exercise 001: What is going on here?

## Setup

You're looking at the captured `print_logs_structured()` output from running a single test against the toy AMM. The relevant facts you need:

- The amm program has four trade-path instructions (`initialize`, `add_liquidity`, `remove_liquidity`, `swap`) and three admin-only instructions (`update_fee`, `set_locked`, `update_authority`).
- Trade-path instructions check `Config.locked` and return `PoolLocked` (Anchor error 6008) when the pool is locked.
- Admin instructions are *not* gated by `locked`; the authority can always call them. (This carve-out exists so a locked-and-renounced pool can't get permanently stuck.)
- SPL Token's `TransferChecked` moves a balance from one token account to another, signed by the source's authority.
- `print_logs_structured()` shows one tree per transaction, with `[N]` indicating CPI stack depth and `✓` / `✗` for success / failure. The number next to each frame is the cumulative CU consumed by that frame and its CPI subtree, so a root frame's CU equals the transaction's total; the footer repeats that total as `Compute Units` and gives the transaction `Fee`. Failed frames render two children inside the frame subtree: a raw `Error: custom program error: 0x<code>` line and an `AnchorError thrown in <path>:<line>` block carrying the decoded `Error Code` / `Number` / `Message`; the trailing `Error: InstructionError(<ix_index>, Custom(<code>))` sits below the tree at the transaction level.
- The `Compute Units` numbers in the trace below are illustrative, not normative<sup>*</sup>. Reason from the trees' *shapes* and approximate magnitudes, not the exact numbers.

Three actors appear in this test:

- **alice** is a liquidity provider. She has deposited into the pool.
- **bob** is an honest trader trying to swap.
- **admin** is the pool's authority, the signer registered on `Config.authority`.

Below is the entire output from the test, in chronological order. Six transactions land. The first two are setup (you can skim them). The interesting ones start at `[A]`. Read carefully.

## The output

```
[setup-1: initialize the pool]
── CYbYnHW7…2yf5::Initialize ──
Transaction  signers=[6PqHNGix…xUVG]
└── CYbYnHW7…2yf5::Initialize [1] ✓ 86420cu  signer=6PqHNGix…xUVG
    ├── System::CreateAccount [2] ✓
    ├── Token::InitializeMint2 [2] ✓ 201cu
    ├── AssociatedToken::Create [2] ✓ 13517cu
    │   ├── Token::GetAccountDataSize [3] ✓ 183cu
    │   ├── System::CreateAccount [3] ✓
    │   ├── Token::InitializeImmutableOwner [3] ✓ 38cu
    │   └── Token::InitializeAccount3 [3] ✓ 235cu
    ├── AssociatedToken::Create [2] ✓ 18017cu
    │   ├── Token::GetAccountDataSize [3] ✓ 183cu
    │   ├── System::CreateAccount [3] ✓
    │   ├── Token::InitializeImmutableOwner [3] ✓ 38cu
    │   └── Token::InitializeAccount3 [3] ✓ 235cu
    ├── AssociatedToken::Create [2] ✓ 13517cu
    │   ├── Token::GetAccountDataSize [3] ✓ 183cu
    │   ├── System::CreateAccount [3] ✓
    │   ├── Token::InitializeImmutableOwner [3] ✓ 38cu
    │   └── Token::InitializeAccount3 [3] ✓ 235cu
    └── System::CreateAccount [2] ✓
Compute Units (this run): 86420
Fee: 5000 lamports

[setup-2: alice deposits liquidity]
── CYbYnHW7…2yf5::AddLiquidity ──
Transaction  signers=[8r9fJP9H…bjUA]
└── CYbYnHW7…2yf5::AddLiquidity [1] ✓ 60121cu  signer=8r9fJP9H…bjUA
    ├── AssociatedToken::Create [2] ✓ 13416cu
    │   ├── Token::GetAccountDataSize [3] ✓ 183cu
    │   ├── System::CreateAccount [3] ✓
    │   ├── Token::InitializeImmutableOwner [3] ✓ 38cu
    │   └── Token::InitializeAccount3 [3] ✓ 235cu
    ├── Token::TransferChecked [2] ✓ 105cu
    ├── Token::TransferChecked [2] ✓ 105cu
    ├── Token::MintTo [2] ✓ 119cu
    └── Token::MintTo [2] ✓ 119cu
Compute Units (this run): 60121
Fee: 5000 lamports

[A]
── amm::SetLocked ──
Transaction  signers=[admin]
└── amm::SetLocked [1] ✓ 4079cu  signer=admin
Compute Units (this run): 4079
Fee: 5000 lamports

Legend (2):
  amm   = CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5
  admin = 6PqHNGixYAxRvMkzuMgEP1DjZzHZhGKVaAV9fSnxxUVG

[B]
── amm::Swap ──
Transaction  signers=[bob]
└── amm::Swap [1] ✗ 32414cu  signer=bob
    ├── Error: custom program error: 0x1778
    └── AnchorError thrown in programs/amm/src/instructions/swap.rs:72
         Error Code: PoolLocked
         Error Number: 6008
         Error Message: Pool is locked.
Error: InstructionError(0, Custom(6008))
Compute Units (this run): 32414
Fee: 5000 lamports

Legend (2):
  amm = CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5
  bob = Dj4q3rbrubSeCSo4sg8EaQk9BLMX2AHR5etWeZZZKNNh

[C]
Transaction  signers=[admin]
├── amm::SetLocked [1] ✓ 4081cu  signer=admin
├── amm::Swap [1] ✓ 28115cu  signer=admin
│   ├── Token::TransferChecked [2] ✓ 105cu
│   └── Token::TransferChecked [2] ✓ 105cu
└── amm::SetLocked [1] ✓ 4079cu  signer=admin
Compute Units (this run): 36275
Fee: 5000 lamports

Legend (2):
  admin = 6PqHNGixYAxRvMkzuMgEP1DjZzHZhGKVaAV9fSnxxUVG
  amm   = CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5

[D]
── amm::Swap ──
Transaction  signers=[bob]
└── amm::Swap [1] ✗ 32414cu  signer=bob
    ├── Error: custom program error: 0x1778
    └── AnchorError thrown in programs/amm/src/instructions/swap.rs:72
         Error Code: PoolLocked
         Error Number: 6008
         Error Message: Pool is locked.
Error: InstructionError(0, Custom(6008))
Compute Units (this run): 32414
Fee: 5000 lamports

Legend (2):
  amm = CYbYnHW7SsnjGya616UuSintpEdezzJZCZuLZT6f2yf5
  bob = Dj4q3rbrubSeCSo4sg8EaQk9BLMX2AHR5etWeZZZKNNh
```

## The question

**What is going on in transactions [A], [B], [C], and [D]? And why is one of them concerning?**

Reason from the trees alone first. The setup gave you the program's invariants; the rest you have to read off the trees and the chronological order. Don't peek at the discussion below until you've worked out at least an answer to:

- What is transaction [C] doing? Specifically, why three sibling roots in one transaction?
- Why does [D] fail with `PoolLocked` if [C] just succeeded?
- Who could possibly have signed [C], given what we know about which actors can call which instructions?

## Sub-questions (use these to break it apart)

- How many distinct top-level instructions are in transaction [C]? Count the `[1]` frames.
- All three roots in [C] resolve to the same program (`amm`). Two of them are `amm::SetLocked`, one is `amm::Swap`. What ordering does that produce, reading left to right?
- The CU and CPI shape corroborates the names: `amm::SetLocked` frames are ~4080 CU with zero CPI children (pure state mutation), while `amm::Swap` in [C] is ~28000 CU with two `Token::TransferChecked` children (the swap's in-leg and out-leg). Reading the names confirms a reading you could also have inferred from the shape alone.
- [B] and [D] are byte-identical in shape (same name `amm::Swap`, same error, similar CU). Bob is the obvious candidate signer. What was the pool's state when each was attempted?
- The PoolLocked error in [B] and [D] points at `programs/amm/src/instructions/swap.rs:72`. That's the line `require!(!self.config.locked, AmmError::PoolLocked)` inside the `swap` handler. What does its existence tell you about the pool state when [B] and [D] were attempted?
- And the corollary: what was the pool's state during [C]'s middle root, given that the middle root is `amm::Swap` and *did not* fail with `PoolLocked`?
- Two `Token::TransferChecked` calls inside the `amm::Swap` of [C]: user → vault, vault → user. Whose user? Whose vault?
- Look at the order in [C]: `SetLocked` → `Swap` → `SetLocked`. What single pattern does that match?

## Discussion

<details>
<summary>Click to reveal walkthrough</summary>

### What each transaction is

The names spell out most of it. The CU and CPI shape corroborate the names; they would also have been enough on their own if the names were not available (as they were not until anchor-litesvm started parsing Anchor's `Program log: Instruction:` convention).

- **[A]** is `amm::SetLocked`, signed by admin. The frame is a single root with zero CPI children at ~4080 CU; that is consistent with a pure state mutation (the handler reads/writes `Config`, no token movement, no init). It explains why subsequent transactions see `locked == true`.

- **[B]** is `amm::Swap`, signed by bob. The `[1] ✗` with no CPIs means the handler errored *before* invoking any other program. PoolLocked is the specific error; line 72 of `swap.rs` is the `require!(!self.config.locked, ...)` check. The pool is locked, [B] is a trade-path call, and the locked-check fired immediately. Confirms [A] set `locked = true`.

- **[C]** is the smoking gun. **Three sibling top-level frames in one transaction**, all signed by admin: `amm::SetLocked` → `amm::Swap` → `amm::SetLocked`. The first and third frames look exactly like [A] (~4080 CU, zero CPIs, pure state mutation). The middle frame has two `Token::TransferChecked` calls, the canonical fingerprint of a swap (user → vault, vault → user).

  Who could have signed [C]? `set_locked` is admin-only (`require_keys_eq!(self.authority.key(), config.authority, Unauthorized)`). The only signer who can call `set_locked` is `Config.authority`: by setup, that's **admin**. So [C] is the authority bundling an unlock + their own swap + a relock in one atomic transaction.

- **[D]** is another `amm::Swap` attempt by bob. Byte-identical to [B] in shape and error. Even though the authority just executed a swap in [C], the pool is locked again by the time [D] arrives, because [C]'s third frame is `set_locked(true)`.

### Why [C] is concerning

The lock is supposed to mean "the pool is paused; nobody is trading." Bob, looking at the on-chain `Config.locked` flag between transactions, sees `true` before [B], `true` after [B], `true` before [D], `true` after [D]. From his vantage point, the pool never unlocked.

But it did. [C]'s middle frame is a successful swap, which means at the instant it ran, `Config.locked == false`. The authority opened the gate, walked through, and closed it again, all within a transaction's atomic boundary. No other user had a window to interact. Honest users see only failed attempts; the authority captured a trade.

Failure modes this enables:

- **Stale-price arbitrage.** Admin observes off-chain that the market price moved against the pool, freezes the pool to prevent LPs from rebalancing, atomically takes the favorable side of the trade against the now-stale on-chain reserves, freezes again. LPs absorb the loss without ever having a chance to act.
- **Selective service.** Admin runs friendly trades for off-chain counterparties through the "locked" pool while strangers see `PoolLocked`.
- **MEV-protection theater.** A pool marketed as "pause-on-suspicious-activity" can't actually deliver that protection, because the pauser is also a privileged trader.

### How the structured logs surfaced this

The vulnerability is invisible in plain Solana log strings: there, it's "Program X invoke [1]" three times, with no indication that one of them is `set_locked` vs `swap` until you cross-reference the `Program log: Instruction:` lines by hand and reconstruct the depth tree by pairing `invoke` and `success` markers. The structured tree makes it readable in three ways:

1. **Multiple sibling roots = multi-ix tx.** Solana's log format makes the depth implicit; the tree makes it explicit. You can count `[1]` frames at a glance.
2. **Instruction-name decoding for every frame.** Well-known programs (SPL Token, ATA, System) are decoded from their discriminator (`Token::TransferChecked`); Anchor user programs (like `amm`) are decoded from the `Program log: Instruction: <Name>` line that Anchor's generated dispatcher emits on every handler entry. The bug pattern reads out by name directly: `SetLocked` → `Swap` → `SetLocked`.
3. **CPI counts as confirmation.** Pure state mutations (`amm::SetLocked`) have zero children. Token-moving instructions (`amm::Swap`) have CPIs. The shape `[cheap, expensive-with-token-children, cheap]` is consistent with the names, and would have been enough to identify the pattern even without them.

### How the test made this concrete

The test that produced this output is `programs/amm/tests/test_lock_unlock_attack.rs`. Its critical assertion is:

```rust
let r = world
    .ctx
    .svm
    .send_instructions(&[unlock_ix, admin_swap_ix, relock_ix], &[&admin])
    .unwrap();
r.print_logs_structured(&world.aliases);
assert!(
    r.is_success(),
    "the three-ix atomic tx is currently allowed; this is the bug"
);
```

The test *passes*, which is the bug. A passing test for an attack the spec implicitly forbids is the on-chain demonstration that the implementation diverged from the spec's intent. The fix is in `docs/security/responses/001-lock-unlock-timing-attack.md`: introduce a timelock on unlock so [C]'s middle frame fails with `PoolLocked` (because the unlock hasn't taken effect yet), which causes the entire 3-ix transaction to roll back atomically and the attack vector to close.

</details>

## <sup>*</sup> Why CU values drift between runs

Solana's compute-unit consumption is deterministic per execution: given the same binary, same accounts, and same instruction data, you get the same CU. The trace above looks reproducible, but it isn't, because the inputs aren't held fixed across test runs.

The drift comes from Anchor's account validation. Constraints like `associated_token::mint = ..., associated_token::authority = ...` call `find_program_address` to derive the ATA at validation time. `find_program_address` iterates bumps from 255 downward until it finds an off-curve key; the iteration count varies from 1 to ~50+ depending on the participating pubkeys' bit patterns, and each iteration costs CU. So an instruction that validates ATAs consumes a different amount of CU each time those ATAs are derived from different mints or different user keypairs.

The test fixtures here use `Keypair::new()` (OS-random) for mints and users, so each run generates fresh pubkeys, those pubkeys produce different ATA bumps, and the per-frame CU drifts by a few thousand. Frames that don't validate ATAs (`set_locked` at ~4,080 CU; the bookend frames of the attack tx) are stable. Frames that do (`add_liquidity`, `swap`, all the failure paths through the swap handler) float.

For background on the deterministic-fixture approach we considered and rejected (because it would have narrowed test coverage to one specific bump path), see the discussion captured in the project's `NOTES`.

## See also

- The full vulnerability writeup: [`../issues/001-lock-unlock-timing-attack.md`](../issues/001-lock-unlock-timing-attack.md)
- The proposed mitigation: [`../responses/001-lock-unlock-timing-attack.md`](../responses/001-lock-unlock-timing-attack.md)
- The test that produced this output: `programs/amm/tests/test_lock_unlock_attack.rs`
- To reproduce: `just tt --test test_lock_unlock_attack`
