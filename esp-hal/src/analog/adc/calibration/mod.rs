#[cfg(not(any(esp32, esp32c2, esp32p4, esp32s2)))]
pub use self::curve::{AdcCalCurve, AdcHasCurveCal};
#[cfg(not(any(esp32, esp32p4, esp32s2)))]
pub use self::{
    basic::AdcCalBasic,
    line::{AdcCalLine, AdcHasLineCal},
};

#[cfg(not(any(esp32, esp32p4, esp32s2)))]
mod basic;
#[cfg(not(any(esp32, esp32c2, esp32p4, esp32s2)))]
mod curve;
#[cfg(not(any(esp32, esp32p4, esp32s2)))]
mod line;
