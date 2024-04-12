//! GPIO interrupt

//% CHIPS: esp32 esp32c2 esp32c3 esp32c6 esp32h2 esp32s2 esp32s3
//% FEATURES: embassy embassy-time-timg0 embassy-executor-thread embassy-generic-timers

#![no_std]
#![no_main]

use core::cell::RefCell;

use critical_section::Mutex;
use esp_backtrace as _;
use esp_hal::{
    clock::ClockControl,
    delay::Delay,
    embassy,
    gpio::{Event, Input, PullDown, IO},
    peripherals::Peripherals,
    prelude::*,
    timer::TimerGroup,
};

static COUNTER: Mutex<RefCell<u32>> = Mutex::new(RefCell::new(0));
static INPUT_PIN: Mutex<RefCell<Option<esp_hal::gpio::Gpio2<Input<PullDown>>>>> =
    Mutex::new(RefCell::new(None));

#[entry]
fn main() -> ! {
    let peripherals = Peripherals::take();
    let system = peripherals.SYSTEM.split();
    let clocks = ClockControl::boot_defaults(system.clock_control).freeze();

    // Set GPIO2 as an output, and set its state high initially.
    let mut io = IO::new(peripherals.GPIO, peripherals.IO_MUX);
    io.set_interrupt_handler(interrupt_handler);

    let delay = Delay::new(&clocks);

    let timg0 = TimerGroup::new_async(peripherals.TIMG0, &clocks);
    embassy::init(&clocks, timg0);

    let mut io2 = io.pins.gpio2.into_pull_down_input();
    let mut io4 = io.pins.gpio4.into_push_pull_output();
    io4.set_low();

    critical_section::with(|cs| {
        *COUNTER.borrow_ref_mut(cs) = 0;
        io2.listen(Event::AnyEdge);
        INPUT_PIN.borrow_ref_mut(cs).replace(io2);
    });
    io4.set_high();
    delay.delay_millis(1);
    io4.set_low();
    delay.delay_millis(1);
    io4.set_high();
    delay.delay_millis(1);
    io4.set_low();
    delay.delay_millis(1);
    io4.set_high();
    delay.delay_millis(1);
    io4.set_low();
    delay.delay_millis(1);
    io4.set_high();
    delay.delay_millis(1);
    io4.set_low();
    delay.delay_millis(1);
    io4.set_high();
    delay.delay_millis(1);

    let count = critical_section::with(|cs| *COUNTER.borrow_ref(cs));
    assert_eq!(count, 9);
    esp_println::println!("Interrupts: {}", count);

    io2 = critical_section::with(|cs| INPUT_PIN.borrow_ref_mut(cs).take().unwrap());
    io2.unlisten();

    loop {
        delay.delay_millis(500);
    }
}

#[handler]
pub fn interrupt_handler() {
    critical_section::with(|cs| {
        use esp_hal::gpio::Pin;

        *COUNTER.borrow_ref_mut(cs) += 1;
        INPUT_PIN
            .borrow_ref_mut(cs)
            .as_mut() // we can't unwrap as the handler may get called for async operations
            .map(|pin| pin.clear_interrupt());
    });
}
