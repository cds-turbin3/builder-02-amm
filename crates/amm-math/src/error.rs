/// Error variants for AMM math operations.
#[derive(Debug, PartialEq)]
pub enum AmmMathError {
    /// An input amount was zero.
    ZeroAmount,

    /// A reserve was zero.
    ZeroReserve,

    /// Division by zero in a math helper.
    ZeroDiv,

    /// Fee parameter out of range (fee_bps >= fee_denominator).
    InvalidFee,

    /// Arithmetic overflow in a checked operation.
    Overflow,

    /// Arithmetic underflow in a checked operation.
    Underflow,

    /// Computed swap output rounded to zero.
    InsufficientOutput,

    /// Pool cannot satisfy the request (drain, tiny LP burn, uninitialized supply, etc.).
    InsufficientLiquidity,
}
