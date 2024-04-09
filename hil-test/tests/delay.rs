//! Delay Test

#![no_std]
#![no_main]

use defmt_rtt as _;
use embedded_hal::delay::DelayNs;
use esp_backtrace as _;
use esp_hal::{clock::ClockControl, delay::Delay, peripherals::Peripherals, prelude::*};

struct Context {
    delay: Delay,
}

impl Context {
    pub fn init() -> Self {
        let peripherals = Peripherals::take();
        let system = peripherals.SYSTEM.split();
        let clocks = ClockControl::boot_defaults(system.clock_control).freeze();

        let delay = Delay::new(&clocks);

        Context { delay }
    }
}

#[cfg(test)]
#[embedded_test::tests]
mod tests {
    use super::*;

    #[init]
    fn init() -> Context {
        Context::init()
    }

    #[test]
    #[timeout(1)]
    fn delay_700millis(ctx: Context) {
        ctx.delay.delay_millis(700);
    }

    #[test]
    #[timeout(2)]
    fn delay_1_600_000_000ns(mut ctx: Context) {
        ctx.delay.delay_ns(1_600_000_000);
    }

    #[test]
    #[timeout(3)]
    fn delay_2_700_000us(mut ctx: Context) {
        ctx.delay.delay_us(2_700_000);
    }

    #[test]
    #[timeout(5)]
    fn delay_5_000ms(mut ctx: Context) {
        ctx.delay.delay_ms(4700);
    }
}
