pub use num_bigint::BigInt;
pub use num_traits::{ToPrimitive, Zero};

pub type SurtrInt = BigInt;
pub type RuntimeTag = u32;
pub type BuiltinId = u16;
pub type FunctionId = u32;

pub fn int<T>(value: T) -> SurtrInt
where
    T: Into<BigInt>,
{
    value.into()
}
