# 001 - Lock/unlock timing attack

| Field        | Value                                                          |
|--------------|----------------------------------------------------------------|
| Status       | Open                                                           |
| Severity     | High                                                           |
| Component    | `programs/amm/`: `set_locked` instruction + trade-path handlers |
| Reporter     | Internal review                                                |
| PoC          | `programs/amm/tests/test_lock_unlock_attack.rs`                |
| Spec section | [§Admin Instructions](../../toy-amm.spec.md#admin-instructions) |

## Summary

The pool's authority can perform trades while the pool is, from every other user's perspective, locked. By bundling `set_locked(false)`, their own `swap`, and `set_locked(true)` into a single Solana transaction, the authority opens a trading window only they can access, captures value, and closes the window, all atomically. No other user has any opportunity to interact during that window. The locked state, as visible to users between transactions, never reflects the trades that occurred.

This violates the implicit contract that `Config.locked == true` communicates to users: that the pool is paused and no trades are executing.

## Background: what `set_locked` is supposed to mean

The spec ([§Admin Instructions](../../toy-amm.spec.md#admin-instructions)) introduces `Config.locked` as an authority-controlled boolean that gates the three trade-path instructions (`swap`, `add_liquidity`, `remove_liquidity`). When `true`, those instructions return `AmmError::PoolLocked`. The intent, by both spec wording ("freeze or unfreeze the pool") and reasonable user reading, is that a locked pool is a paused pool: nobody trades, the reserves are frozen, LPs can rebalance off-chain knowing the pool is stationary.

Admin instructions (`update_fee`, `set_locked`, `update_authority`) are explicitly *not* gated by `locked`. That carve-out exists for a sensible reason: otherwise a locked-and-renounced pool would be permanently stuck. The current design pays for that escape hatch with a hidden privilege the spec doesn't acknowledge: the authority can flip the lock and execute a trade in the same transaction.

## Vulnerability

The attack is a single atomic transaction with three instructions, all signed by the authority:

```
1. set_locked(false)   - flip Config.locked from true to false
2. swap(...)           - any trade the authority wants
3. set_locked(true)    - flip Config.locked back to true
```

Solana transactions are atomic and ordered. From the moment the transaction lands until it completes, no other transaction can interleave. The pre-state has `locked == true`. The post-state has `locked == true`. Mid-transaction, after instruction 1 commits and before instruction 3 commits, the authority's swap executes against a pool that, by the on-chain `locked` flag at that instant, is not locked. By the time the next block is observable, `locked` is back to `true`.

Any user attempting `swap`, `add_liquidity`, or `remove_liquidity` in any transaction adjacent to the authority's tx sees `locked == true` and is rejected with `PoolLocked`. The authority is the only party who can act.

### PoC

`programs/amm/tests/test_lock_unlock_attack.rs` constructs this scenario end-to-end. Alice provides liquidity, the authority locks the pool, an honest trader (Bob) is rejected with `PoolLocked`, then the authority executes the three-ix atomic tx. The test asserts:

- Bob's `swap` fails with `PoolLocked` before the attack.
- The authority's atomic tx succeeds (this is the bug).
- The authority's token balances reflect the executed trade (X out, Y in).
- The pool's vaults move correspondingly.
- Bob's `swap` continues to fail with `PoolLocked` after the attack.

The captured CPI tree shows three sibling top-level program frames in one transaction, with two `Token::TransferChecked` calls nested under the middle frame (the swap). This is the on-chain fingerprint of the attack:

```
Transaction
├── amm-program [1] ✓        (unlock)
├── amm-program [1] ✓        (swap)
│   ├── Token::TransferChecked  (user -> vault)
│   └── Token::TransferChecked  (vault -> user)
└── amm-program [1] ✓        (relock)
```

For the verbatim output (including CU per frame, fee, and the bracketing transactions) along with an annotated walkthrough framed as a classroom exercise, see [the trace exercise](../exercises/001-what-is-going-on.md). Run `just poc` to reproduce the trace locally.

## Impact

The economic value of the attack depends on the authority having an information advantage. The vulnerability itself is the asymmetric *capability*. Examples of how it can be exploited:

1. **Stale-price arbitrage.** The authority observes off-chain that the market price of token X has dropped sharply. The pool, frozen by the lock, still has the pre-drop reserves. The authority unlocks, swaps Y for X at the stale ratio, relocks. When the lock is eventually lifted for honest users, the pool's reserves reflect the new (worse for LPs) ratio. The authority captured the spread; LPs (Alice in the PoC) absorb the loss.

2. **Selective service.** The authority unlocks just long enough to execute a swap for an off-chain customer (paying out-of-band), then relocks. Other users see a permanently-locked pool but specific friends get filled.

3. **MEV protection theater.** A pool marketed as "MEV-protected by pause-on-suspect-activity" cannot actually deliver that protection: the entity running the pause is the same entity that can execute trades through the pause.

In each scenario, the LP's loss is bounded by reserve depth and the size of the information edge. For a deep pool with a small fee, a single attack might extract ~the fee on the swap. Over many attacks, value accrues to the authority and bleeds from LPs.

### Severity rationale

This is **High**, not Critical:

- LP funds are not directly drained. The authority can't unilaterally transfer reserves out; they have to *trade* through the pool, which the math constrains (no zero-output swaps, slippage bounded by reserve depth).
- The attack requires the authority to be willing to execute trades signed by their own key, leaving an on-chain audit trail. It's not a stealthy exploit.
- However, the asymmetric capability is real, undisclosed, and contradicts user expectations established by the spec wording.

If LP funds could be drained directly or if the attack were invisible on chain, this would be Critical. As-is, it's a meaningful trust gap that needs to be closed.

## Recommendation

Make `set_locked(false)` *not* immediately mutable. Specifically, introduce a timelock: `set_locked(false)` schedules a pending unlock at `now + unlock_delay` (where `unlock_delay` is a per-pool constant or a stored value), and the actual `Config.locked = false` transition happens lazily on the first subsequent transaction that observes `Clock::unix_timestamp >= pending_unlock_at`.

With a timelock in place:

- The atomic [unlock, swap, relock] pattern fails: the trailing `swap` runs in the same transaction as the `set_locked(false)`, which only *scheduled* an unlock. `Config.locked` is still `true` when the swap reads it. The swap returns `PoolLocked` and the entire tx aborts.
- Honest users see a public, on-chain commitment that the pool will unlock at a specific future time. They can exit their positions during the delay window if they don't want to be present at the unlock.
- The authority retains the ability to fix accidental locks (the lock can be cleared after the delay), which a one-way lock would lose.

The chosen `unlock_delay` is a policy decision. Long delays (days) maximize user safety but make legitimate operational unlocking slow. Short delays (minutes) give weaker guarantees but more flexibility. Defer the specific value to the response document.

Alternative mitigations considered:

- **One-way lock.** `set_locked(true)` is permanent; no unlock instruction exists. Strongest guarantee, brittle: an accidental lock requires a fresh pool. Rejected for the trade-off.
- **Remove the lock entirely.** Strongest guarantee in a different direction: users have no expectation that the pool ever pauses. Defensible but loses operational flexibility and contradicts a feature the spec already documents. Rejected.

## Out of scope for this report

- Off-chain price information advantage in general. The timelock doesn't prevent the authority from trading; it prevents trading inside a window users cannot enter. If the authority trades during the public unlocked window, that's just normal market activity.
- Authority renouncement (`update_authority(None)`). A renounced pool can't mount this attack because no signer satisfies the authority check. This is already a stronger guarantee than the timelock; the spec recommends renouncement for trustless pools. The timelock is for pools that retain an authority.
- Other admin instructions (`update_fee`, `update_authority`). Whether *these* should also have a timelock is a separate question; this report addresses only `set_locked`.

## References

- Spec: [§Admin Instructions](../../toy-amm.spec.md#admin-instructions)
- PoC: `programs/amm/tests/test_lock_unlock_attack.rs`
- Response (mitigation): `docs/security/responses/001-lock-unlock-timing-attack.md`
