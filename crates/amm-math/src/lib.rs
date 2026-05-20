//! Constant-product AMM math library.
//!
//! Pure integer arithmetic for AMM operations.
//!
//! Module map:
//! - [`math`]    — helper functions (`div_ceil`, `checked_mul_div_floor/ceil`, `integer_sqrt_floor`, `narrow_u64`)
//! - [`swap`]    — exact-input and exact-output swap formulas
//! - [`liquidity`] — initial deposit, add liquidity, remove liquidity
//! - [`types`]   — quote types (`SwapQuote`, `ExactOutputQuote`, `LiquidityQuote`)
//! - [`error`]   — `AmmMathError`

mod error;
mod liquidity;
mod math;
mod swap;
mod types;

// Flat re-exports: callers use `amm_math::swap_exact_input` rather than
// `amm_math::swap::swap_exact_input`. See [§Anchor Integration Rule] for the
// expected consumer (an Anchor program calling these as pure functions).
pub use error::AmmMathError;
pub use liquidity::*;
pub use math::*;
pub use swap::*;
pub use types::*;
