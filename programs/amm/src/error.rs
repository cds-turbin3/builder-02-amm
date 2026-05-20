//! On-chain error codes. Most variants are 1:1 reflections of
//! [`amm_math::AmmMathError`] so math failures surface as Anchor errors with
//! distinct codes; the trailing variants (`PoolLocked`, `SlippageExceeded`)
//! are program-level and have no math-library counterpart.
//!
//! See [§Numeric Policy](../../../docs/toy-amm.spec.md#numeric-policy) and
//! [§Required Invariants](../../../docs/toy-amm.spec.md#required-invariants).

use amm_math::AmmMathError;
use anchor_lang::prelude::*;

#[error_code]
pub enum AmmError {
    #[msg("Input amount is zero")]
    ZeroAmount,
    #[msg("A reserve is zero")]
    ZeroReserve,
    #[msg("Division by zero")]
    ZeroDiv,
    #[msg("Fee bps is out of range (must be < FEE_DENOMINATOR)")]
    InvalidFee,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Arithmetic underflow")]
    Underflow,
    #[msg("Computed output rounded to zero")]
    InsufficientOutput,
    #[msg("Pool cannot satisfy the request")]
    InsufficientLiquidity,
    #[msg("Pool is locked")]
    PoolLocked,
    #[msg("Slippage tolerance exceeded")]
    SlippageExceeded,
    #[msg("Signer does not match Config.authority")]
    Unauthorized,
    #[msg("Authority has been renounced; pool is immutable")]
    AuthorityRenounced,
}

// Orphan rule blocks `From<AmmMathError> for anchor_lang::error::Error` directly
// (both types are foreign to this crate), so callers map through AmmError:
//   amm_math_call().map_err(AmmError::from)?
impl From<AmmMathError> for AmmError {
    fn from(e: AmmMathError) -> Self {
        match e {
            AmmMathError::ZeroAmount => AmmError::ZeroAmount,
            AmmMathError::ZeroReserve => AmmError::ZeroReserve,
            AmmMathError::ZeroDiv => AmmError::ZeroDiv,
            AmmMathError::InvalidFee => AmmError::InvalidFee,
            AmmMathError::Overflow => AmmError::Overflow,
            AmmMathError::Underflow => AmmError::Underflow,
            AmmMathError::InsufficientOutput => AmmError::InsufficientOutput,
            AmmMathError::InsufficientLiquidity => AmmError::InsufficientLiquidity,
        }
    }
}
