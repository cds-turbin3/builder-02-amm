# Constant Product AMM Math Library Specification

## Table of Contents

- [Purpose](#purpose)
- [Core Types](#core-types)
- [Math Reference](#math-reference)
- [Rounding Policy](#rounding-policy)
- [Implementation Constraints](#implementation-constraints)
- [Invariants](#invariants)
- [Admin Operations](#admin-operations)
- [Anchor Integration](#anchor-integration)
- [Impermanent Loss (Reference)](#impermanent-loss-reference)
- [Security Notes](#security-notes)

---

## Purpose

This library provides deterministic integer arithmetic for a constant product AMM using the invariant `x * y = k`, intended for Solana Anchor programs where all token amounts are atomic u64 units.

## Core Types

```rust
pub struct SwapQuote {
    pub amount_in: u64,
    pub amount_in_after_fee: u64,
    pub fee_amount: u64,
    pub amount_out: u64,
    pub new_reserve_in: u64,
    pub new_reserve_out: u64,
}

pub struct ExactOutputQuote {
    pub amount_out: u64,
    pub amount_in_after_fee: u64,
    pub amount_in: u64,
    pub fee_amount: u64,
    pub new_reserve_in: u64,
    pub new_reserve_out: u64,
}

pub struct LiquidityQuote {
    pub lp_tokens: u64,
    pub amount_a: u64,
    pub amount_b: u64,
}

pub enum AmmMathError {
    ZeroAmount,
    ZeroReserve,
    ZeroDivision,
    InvalidFee,
    Overflow,
    Underflow,
    InsufficientOutput,
    InsufficientLiquidity,
}
```

The `fee_amount` field is informational and always equals `amount_in - amount_in_after_fee`.

---

## Math Reference

### Fee Calculation

Fees are represented in basis points.

```
fee_denominator = 10_000

amount_in_after_fee = floor(amount_in * (fee_denominator - fee_bps) / fee_denominator)
fee_amount = amount_in - amount_in_after_fee
```

### Swap: Exact Input

Given reserves and fixed input:

```
amount_in_after_fee = floor(amount_in * (fee_denominator - fee_bps) / fee_denominator)
fee_amount = amount_in - amount_in_after_fee
amount_out = floor(amount_in_after_fee * reserve_out / (reserve_in + amount_in_after_fee))
new_reserve_in = reserve_in + amount_in
new_reserve_out = reserve_out - amount_out
```

### Swap: Exact Output

Given reserves and fixed output:

```
amount_in_after_fee = ceil(amount_out * reserve_in / (reserve_out - amount_out))
amount_in = ceil(amount_in_after_fee * fee_denominator / (fee_denominator - fee_bps))
fee_amount = amount_in - amount_in_after_fee
new_reserve_in = reserve_in + amount_in
new_reserve_out = reserve_out - amount_out
```

Preconditions:
- `amount_out > 0`
- `amount_out < reserve_out` (otherwise `InsufficientLiquidity`)
- `reserve_in > 0` and `reserve_out > 0`

### Initial Liquidity

For the first liquidity provider:

```
MINIMUM_LIQUIDITY = 1_000

minted_lp_tokens = floor(sqrt(amount_a * amount_b))
lp_tokens_to_user = minted_lp_tokens - MINIMUM_LIQUIDITY
```

Precondition: `minted_lp_tokens > MINIMUM_LIQUIDITY` (otherwise `InsufficientLiquidity`).

MINIMUM_LIQUIDITY tokens are permanently locked (minted to a burn address).

### Adding Liquidity

To preserve the reserve ratio, given `amount_a`:

```
required_b = ceil(amount_a * reserve_b / reserve_a)

lp_tokens = min(
    floor(amount_a * total_lp_supply / reserve_a),
    floor(amount_b * total_lp_supply / reserve_b)
)
```

### Removing Liquidity

Given LP tokens to burn:

```
amount_a = floor(lp_tokens * reserve_a / total_lp_supply)
amount_b = floor(lp_tokens * reserve_b / total_lp_supply)
```

---

## Rounding Policy

**Core rule: if rounding decides who receives the remainder, the pool receives it.**

Applied to swaps:
- **Exact-input swaps**: round `amount_out` down (floor). Trader receives at most what the invariant allows.
- **Exact-output swaps**: round `amount_in` up (ceil, twice: once for invariant, once for fee gross-up). Trader cannot underpay due to truncation.

Applied to liquidity:
- LP tokens minted are rounded down.
- LP withdrawal amounts are rounded down.
- Required deposit amounts are rounded up.

---

## Implementation Constraints

### Arithmetic

- **MUST** use checked arithmetic (`checked_add`, `checked_mul`, `checked_sub`) for all operations that could overflow or underflow.
- **MUST** use u128 internally for all intermediate calculations involving multiplication and division.
- **MUST** return u64 token amounts when safe (after verifying no overflow).
- **MUST NOT** use floating point (f32, f64).

### Fee Policy

- **MUST** validate `fee_bps < fee_denominator`. Return `InvalidFee` if false.
- **MUST** keep fee tokens in the pool reserve; they accrue to LPs by growing the invariant `k`.
- **MUST** use `new_reserve_in = reserve_in + amount_in` (the full input, not `amount_in_after_fee`). Using the reduced amount will cause on-chain reserves to diverge from computed reserves.
- **MUST** use `reserve_in + amount_in_after_fee` in the `amount_out` denominator, not `new_reserve_in`. These differ by the fee amount; using the wrong one silently changes the curve.

### Division Helper

**MUST** implement ceiling division safely, returning `ZeroDivision` on zero denominator:

```rust
fn div_ceil(numerator: u128, denominator: u128) -> Result<u128, AmmMathError> {
    if denominator == 0 {
        return Err(AmmMathError::ZeroDivision);
    }
    numerator
        .checked_add(denominator - 1)
        .ok_or(AmmMathError::Overflow)?
        .checked_div(denominator)
        .ok_or(AmmMathError::Overflow)
}
```

**SHOULD** expose or internally define:
- `fn checked_mul_div_floor(a: u128, b: u128, denominator: u128) -> Result<u128, AmmMathError>`
- `fn checked_mul_div_ceil(a: u128, b: u128, denominator: u128) -> Result<u128, AmmMathError>`
- `fn integer_sqrt_floor(value: u128) -> u128`

### Edge Cases

- **MUST** return `ZeroAmount` if any input amount is zero.
- **MUST** return `ZeroReserve` if any reserve is zero.
- **MUST** return `ZeroDivision` when division by zero is attempted.
- **MUST** return `InsufficientLiquidity` if liquidity cannot satisfy the request (swap output > reserve, or initial deposit too small).
- **MUST** return `Overflow` or `Underflow` when checked arithmetic fails.

---

## Invariants

All successful operations **MUST** satisfy their corresponding invariants.

### Fee Invariant

```
fee_bps < fee_denominator
fee_amount = amount_in - amount_in_after_fee
```

### Swap Invariants (Exact-Input and Exact-Output)

| Invariant | Statement | Rationale |
|---|---|---|
| Positivity | `amount_in > 0 && amount_out > 0` | Zero-amount swaps are invalid. |
| Reserve Identity (In) | `new_reserve_in == reserve_in + amount_in` | Full input enters the pool; fee tokens remain. Critical: using `amount_in_after_fee` silently leaks value. |
| Reserve Identity (Out) | `new_reserve_out == reserve_out - amount_out` | Exact output removed. |
| Constant-Product | `new_k >= old_k` (where `k = reserve_in * reserve_out`) | Invariant holds; fees inflate `k` further. Compute with u128. |
| Pre-Fee Invariant | `(reserve_in + amount_in_after_fee) * new_reserve_out >= reserve_in * old_reserve_out` | Strictly stronger: fee is pure yield, not load-bearing. Protects against bugs that leak value to traders. |

### Liquidity Add Invariants

| Invariant | Statement | Rationale |
|---|---|---|
| Positivity | `amount_a > 0 && amount_b > 0 && lp_tokens > 0` | No zero-sized operations. |
| No Dilution | `new_amount_a / total_lp_supply >= old_amount_a / old_total_lp_supply` (and same for B) | Deposit ratio must not worsen existing LP positions. |
| Initial Deposit | `minted_lp_tokens > MINIMUM_LIQUIDITY` | First deposit must mint enough to exceed locked minimum. |

### Liquidity Remove Invariants

| Invariant | Statement | Rationale |
|---|---|---|
| Positivity | `amount_a > 0 && amount_b > 0 && lp_tokens > 0` | No zero-sized operations. |
| Supply Decrease | `total_lp_supply_after < total_lp_supply_before` | LP tokens are burned. |

### Property Test Requirements

The implementation **MUST** pass property tests verifying:

- Swap never gives more output than exact rational math allows.
- Swap does not reduce `k` (computed with u128 arithmetic).
- Swap satisfies the **pre-fee invariant** (listed above; strictly stronger than `new_k >= old_k`).
- LP minting does not dilute existing LPs.
- LP withdrawal does not overpay exiting LPs.
- Rounding remainders stay in the pool (no loss of precision to traders).

---

## Admin Operations

The on-chain program provides three admin-only instructions: `update_fee`, `set_locked`, and `update_authority`. These touch `Config` directly and do not call the math library.

Authority model: `Config.authority: Option<Pubkey>`

- `Some(pubkey)`: that pubkey may call admin instructions.
- `None`: the pool is immutable; no further admin operations are permitted.

### Instructions

| Instruction | Argument | Effect |
|---|---|---|
| `update_fee` | `new_fee_bps: u16` | sets `Config.fee_bps`. MUST validate `new_fee_bps < fee_denominator`. |
| `set_locked` | `locked: bool` | sets `Config.locked`. While `true`, swap/add/remove return `PoolLocked`. |
| `update_authority` | `new_authority: Option<Pubkey>` | replaces `Config.authority`. Passing `None` renounces forever. |

Account constraints (all three instructions):
- `authority: Signer` (MUST match current `Config.authority`; checked manually for `Option` type)
- `config: Account<Config>` (mutable PDA `[CONFIG_SEED, seed.to_le_bytes()]`)

Admin instructions **MUST NOT** be gated by `Config.locked`; an authority can adjust fees, rotate authority, or unlock even when locked (else a locked-and-renounced pool becomes stuck).

Error handling: return `AuthorityRenounced` if `Config.authority == None`; return `Unauthorized` if signer does not match.

---

## Anchor Integration

Anchor instructions **SHOULD** follow this pattern:

1. Read validated account state (reserves, fees, authority).
2. Call the math library function.
3. Enforce user-provided slippage limits.
4. Perform SPL Token transfers.
5. Update pool state from the returned quote.

Slippage constraints:
- Exact-input swap: `amount_out >= minimum_amount_out`
- Exact-output swap: `amount_in <= maximum_amount_in`
- Liquidity add: `lp_tokens >= minimum_lp_tokens`

The math library is a pure function and cannot prevent slippage (reserves change between quote time and execution). The Anchor instruction enforces the user's tolerance against live reserves.

---

## Impermanent Loss (Reference)

An LP's position value at time T1 (given price ratio change from deposit to withdrawal):

```
IL(r) = 2 * sqrt(r) / (1 + r) - 1
```

where `r` is the ratio of final price to initial price.

The net P&L of an LP is approximately:

```
net P&L ≈ fees_earned - IL
```

The math library does not compute IL; clients can reconstruct the LP's position (amount_a, amount_b) at any time using the **Removing Liquidity** formula, then apply external price data and the `IL(r)` formula to display P&L.

---

## Security Notes

Integer rounding is part of the protocol design. Rounding must never accidentally give value to the trader or exiting LP at the expense of the pool.

All division operations **MUST** document whether floor or ceiling is used and why that choice favors the pool.
