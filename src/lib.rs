mod error;
mod liquidity;
mod math;
mod swap;
mod types;

// export in flat manner, no namespace needed for a a toy
pub use error::AmmMathError;
pub use liquidity::*;
pub use math::*;
pub use swap::*;
pub use types::*;
