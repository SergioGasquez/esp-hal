//! Clock Monitor Test

//% CHIPS: esp32 esp32c2 esp32c3 esp32c6 esp32h2 esp32s2 esp32s3

#![no_std]
#![no_main]

use defmt_rtt as _;
use esp_backtrace as _;
use esp_hal::{clock::ClockControl, peripherals::Peripherals, prelude::*, rtc_cntl::Rtc};

struct Context<'a> {
    rtc: Rtc<'a>,
    #[cfg(any(feature = "esp32", feature = "esp32c2"))]
    xtal_freq: u32,
}

impl Context<'_> {
    pub fn init() -> Self {
        let peripherals = Peripherals::take();
        let system = peripherals.SYSTEM.split();
        #[cfg(any(feature = "esp32", feature = "esp32c2"))]
        let clocks = ClockControl::boot_defaults(system.clock_control).freeze();
        #[cfg(not(any(feature = "esp32", feature = "esp32c2")))]
        ClockControl::boot_defaults(system.clock_control).freeze();
        let rtc = Rtc::new(peripherals.LPWR, None);

        Context {
            rtc,
            #[cfg(any(feature = "esp32", feature = "esp32c2"))]
            xtal_freq: clocks.xtal_clock.to_MHz(),
        }
    }
}

#[cfg(test)]
#[embedded_test::tests]
mod tests {
    use super::*;

    #[init]
    fn init() -> Context<'static> {
        Context::init()
    }

    #[test]
    fn test_estimated_clock(mut ctx: Context<'static>) {
        #[cfg(any(feature = "esp32", feature = "esp32c2"))] // 26 MHz
        {
            if ctx.xtal_freq == 26 {
                // 26 MHz
                defmt::assert!((23..=29).contains(&ctx.rtc.estimate_xtal_frequency()));
            } else {
                // 40 MHz
                defmt::assert!((35..=45).contains(&ctx.rtc.estimate_xtal_frequency()));
            }
        }
        #[cfg(feature = "esp32h2")] // 32 MHz
        defmt::assert!((29..=35).contains(&ctx.rtc.estimate_xtal_frequency()));
        #[cfg(not(any(feature = "esp32", feature = "esp32c2", feature = "esp32h2")))] // 40 MHz
        defmt::assert!((35..=45).contains(&ctx.rtc.estimate_xtal_frequency()));
    }
}
