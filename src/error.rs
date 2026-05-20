#[derive(Debug)]
pub enum AmmMathError {
    /// an amount is zero
    ZeroAmount,

    /// either reserve is zero
    ZeroReserve,

    /// div by zero
    ZeroDiv,

    InvalidFee,
    Overflow,
    Underflow,
    InsufficientOutput,
    InsufficientLiquidity,
}
