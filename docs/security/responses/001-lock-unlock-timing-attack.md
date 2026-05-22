# 001 - Response: lock/unlock timing attack

| Field        | Value                                                          |
|--------------|----------------------------------------------------------------|
| Issue        | [001 - Lock/unlock timing attack](../issues/001-lock-unlock-timing-attack.md) |
| Status       | Mitigation planned                                             |
| Severity     | High                                                           |
| Mitigation   | Timelock on unlock                                             |
| Owner        | Amal                                                           |

## Acknowledgement

The reported issue is valid. The on-chain capability is real, the PoC reproduces it deterministically, and it contradicts what `Config.locked == true` communicates to users via the spec. We are mitigating, not accepting.

## Mitigation choice

We adopt a **timelock on unlock**: `set_locked(false)` no longer flips `Config.locked` immediately; it records a `pending_unlock_at: i64` Unix-timestamp commitment and leaves `locked` unchanged. The actual `locked = false` transition is lazy, applied at the start of any subsequent trade-path handler that observes `Clock::unix_timestamp >= pending_unlock_at` (and `locked == true && pending_unlock_at != 0`).

This kills the attack mechanism directly: the three-ix attack tx has its `set_locked(false)` succeed (it writes `pending_unlock_at`) but the trailing `swap` runs against `Config.locked == true` and `Clock::unix_timestamp < pending_unlock_at`. The swap returns `PoolLocked`. The whole tx rolls back atomically.

### Why timelock over alternatives

We considered three options in the issue. Recapping the trade-offs and the resolution:

- **Timelock (chosen).** Preserves operational flexibility; can fix accidental locks after the delay window. Gives users a public, on-chain commitment they can read off the pool's Config. Cost: one extra field in `Config`, slightly more complex state machine in `set_locked` and the trade-path handlers, and a new error variant.
- **One-way lock.** Simpler code: drop the unlock path entirely. Strong guarantee: a locked pool is locked forever, period. Cost: an accidentally-locked pool is permanently dead; users have to migrate to a new pool. Rejected because the operational fragility outweighs the simplicity gain for a toy AMM.
- **Remove the lock entirely.** Even simpler: no `set_locked` instruction at all. Cost: loses a feature the spec already documents and that some pools want for legitimate operational reasons (e.g., responding to a discovered vulnerability in the program itself). Rejected.

A user who specifically wants the strongest guarantees can still get them via the existing `update_authority(None)` to renounce, which removes any future admin action including the timed unlock. The timelock is the recommended path for pools that retain an authority.

## Parameter: `unlock_delay`

Concrete choice: **24 hours** (`86_400` seconds), stored on `Config` at pool creation, immutable thereafter.

Rationale:

- Long enough that users running standard wallets / front-ends can reasonably observe a pending unlock and exit their positions before it triggers. Many user flows refresh state at most every few hours; a 24-hour window covers daily rhythms.
- Short enough that operational unlocking (e.g., the authority deploys a routine config change and re-opens the pool the next day) doesn't grind the pool's lifecycle to a halt.
- Long enough that the asymmetric-trade window the issue describes can't be opened in any practical sense: 24 hours of public unlocked-state is normal market exposure, not a captured-window attack.

A configurable per-pool value (chosen at initialization) is reasonable but adds parameter-management surface. For a toy AMM, hard-coding `unlock_delay = 86_400` keeps the change minimal. If future pools need a different value, the constant graduates to a field with a small migration; we accept that future cost rather than pay the design tax now.

## Implementation plan

### Changes to `Config` (state.rs)

Add a single field:

```rust
pub pending_unlock_at: i64,
```

`0` means "no pending unlock" (sentinel). A non-zero value is the Unix timestamp at which the unlock becomes effective. We use `i64` to match `Clock::unix_timestamp`.

Initialization: `pending_unlock_at = 0` on `set_inner` in `Initialize::init`.

### Changes to `set_locked` (instructions/set_locked.rs)

New behavior:

- `set_locked(true)`: clears `pending_unlock_at` to `0` (any scheduled unlock is canceled), sets `locked = true`. Immediate.
- `set_locked(false)`: requires `Config.locked == true` (you can't schedule an unlock for an already-unlocked pool). Reads `Clock::unix_timestamp`, writes `pending_unlock_at = now + UNLOCK_DELAY`. Does *not* mutate `locked` yet.

The auth check (signer matches `Config.authority` and `authority != None`) is unchanged.

### Changes to trade-path handlers (`swap`, `add_liquidity`, `remove_liquidity`)

At the top of each handler, before the existing `require!(!config.locked, ...)`:

```rust
if self.config.locked && self.config.pending_unlock_at != 0 {
    let now = Clock::get()?.unix_timestamp;
    if now >= self.config.pending_unlock_at {
        self.config.locked = false;
        self.config.pending_unlock_at = 0;
    }
}
require!(!self.config.locked, AmmError::PoolLocked);
```

The "lazy apply" pattern: any user transacting after the timelock expires implicitly transitions the pool to unlocked. No separate "execute unlock" instruction needed.

### Changes to `AmmError`

Optional: add a `PendingUnlock` variant for the case where someone calls `set_locked(false)` while a pending unlock is already scheduled. This isn't strictly necessary (we could just no-op or overwrite the scheduled time), but it's a clearer error than silent overwrite.

Decision: **add the variant**, and make `set_locked(false)` on a pool that already has `pending_unlock_at != 0` return `PendingUnlock`. The authority must call `set_locked(true)` (which clears the schedule) to reset.

### Spec update

The [§Admin Instructions](../../toy-amm.spec.md#admin-instructions) section needs a paragraph documenting the timelock semantics. The current text says `set_locked(locked: bool)` "sets `Config.locked`"; the updated text should say:

> `set_locked(true)` flips `Config.locked` immediately and clears any pending unlock. `set_locked(false)` does *not* flip `Config.locked`; instead it schedules a pending unlock at `Clock::unix_timestamp + UNLOCK_DELAY` (24 hours). The lock clears lazily on the first trade-path instruction observed after that timestamp. The delay prevents the authority from atomically opening and closing a trading window in one transaction; see `docs/security/issues/001-lock-unlock-timing-attack.md`.

### Test changes

`programs/amm/tests/test_lock_unlock_attack.rs`:

The current PoC test asserts the atomic 3-ix tx *succeeds*. After the mitigation, that assertion must flip: the atomic tx must *fail* with `PoolLocked`. The test should also check that:

- After `set_locked(false)`, `Config.locked` is still `true` and `pending_unlock_at` is set.
- After advancing the clock past `pending_unlock_at`, the next trade-path instruction succeeds (and observes `locked = false` post-transition).
- `set_locked(false)` followed immediately (in a separate tx) by `set_locked(true)` cancels the pending unlock; trade-path instructions continue to fail with `PoolLocked`.

New tests to add (separate file, e.g. `test_timelock.rs`):

- `unlock_schedules_pending_at_future_timestamp`: assert the math of `pending_unlock_at = now + 86_400`.
- `trade_path_clears_lock_lazily_after_delay`: warp clock past delay, send a swap, observe `Config.locked = false` post-tx.
- `relock_clears_pending_schedule`: `set_locked(true)` after a pending unlock should clear `pending_unlock_at`.
- `set_locked_false_twice_returns_pending_unlock`: two calls to `set_locked(false)` without an intervening `set_locked(true)` should error.

## Disclosure and rollout

Toy project; no external users to coordinate with. Sequence:

1. Land the issue + response markdowns (this file and `../issues/001-lock-unlock-timing-attack.md`). Documents the vulnerability publicly.
2. Implement the mitigation per this plan. Single commit, ideally.
3. Update the PoC test to assert the mitigation works.
4. Add new tests covering the timelock semantics.
5. Update the spec.

No version bump or migration concern; we are pre-release.

## Open questions

- Should `unlock_delay` be configurable per-pool? Currently hard-coded; documented in spec; can be promoted to a field later.
- Should `update_fee` and `update_authority` also be timelocked? Defer; separate issue if needed.
- Should the lazy-apply transition emit an event (`PoolUnlocked` log)? Useful for indexers; adds one event emission per affected ix. Defer; can be added without breaking changes.

## References

- Issue: [`../issues/001-lock-unlock-timing-attack.md`](../issues/001-lock-unlock-timing-attack.md)
- Spec: [§Admin Instructions](../../toy-amm.spec.md#admin-instructions)
- PoC: `programs/amm/tests/test_lock_unlock_attack.rs` (currently asserts the vulnerability; will be updated post-mitigation)
