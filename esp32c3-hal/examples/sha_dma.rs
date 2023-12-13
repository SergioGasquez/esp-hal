//! Demonstrates the use of the SHA peripheral and compares the speed of
//! hardware-accelerated and pure software hashing.

#![no_std]
#![no_main]

use esp32c3_hal::{
    clock::ClockControl,
    dma::DmaPriority,
    gdma::Gdma,
    peripherals::Peripherals,
    prelude::*,
    sha::{dma::WithDmaSha, Sha, ShaMode},
};
use esp_backtrace as _;
use esp_println::println;
use nb::block;
use sha2::{Digest, Sha256};

// define input data
const INPUT: &[u8] = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const INPUT_LEN: usize = INPUT.len();

#[entry]
fn main() -> ! {
    let peripherals = Peripherals::take();
    let system = peripherals.SYSTEM.split();
    let _clocks = ClockControl::boot_defaults(system.clock_control).freeze();

    let dma = Gdma::new(peripherals.DMA);
    let dma_channel = dma.channel0;

    let mut descriptors = [0u32; 8 * 3];
    let mut rx_descriptors = [0u32; 8 * 3];

    let source_data: &[u8] = buffer1();
    let mut remaining = source_data;
    let mut hasher = Sha::new(peripherals.SHA, ShaMode::SHA256).with_dma(dma_channel.configure(
        false,
        &mut descriptors,
        &mut rx_descriptors,
        DmaPriority::Priority0,
    ));

    // Short hashes can be created by decreasing the output buffer to the desired
    // length
    let output = buffer2();

    // let pre_calc = xtensa_lx::timer::get_cycle_count();
    // The hardware implementation takes a subslice of the input, and returns the
    // unprocessed parts The unprocessed parts can be input in the next
    // iteration, you can always add more data until finish() is called. After
    // finish() is called update()'s will contribute to a new hash which
    // can be extracted again with finish().

    while remaining.len() > 0 {
        // Can add println to view progress, however println takes a few orders of
        // magnitude longer than the Sha function itself so not useful for
        // comparing processing time println!("Remaining len: {}",
        // remaining.len());

        // All the HW Sha functions are infallible so unwrap is fine to use if you use
        // block!
        remaining = block!(hasher.sha.update(remaining)).unwrap();
    }

    // Finish can be called as many times as desired to get mutliple copies of the
    // output.
    block!(hasher.sha.finish(output.as_mut_slice())).unwrap();
    // let post_calc = xtensa_lx::timer::get_cycle_count();
    // let hw_time = post_calc - pre_calc;
    // println!("Took {} cycles", hw_time);
    println!("SHA256 Hash HW output {:02x?}", output);

    // let pre_calc = xtensa_lx::timer::get_cycle_count();
    let mut hasher = Sha256::new();
    hasher.update(source_data);
    let soft_result = hasher.finalize();
    // let post_calc = xtensa_lx::timer::get_cycle_count();
    // let soft_time = post_calc - pre_calc;
    // println!("Took {} cycles", soft_time);
    println!("SHA256 Hash SW output {:02x?}", soft_result);

    // println!("HW SHA is {}x faster", soft_time/hw_time);

    loop {}
}

fn buffer1() -> &'static mut [u8; INPUT_LEN] {
    static mut BUFFER: [u8; INPUT_LEN] = [0u8; INPUT_LEN];
    unsafe {
        BUFFER.copy_from_slice(INPUT);
        &mut BUFFER
    }
}

fn buffer2() -> &'static mut [u8; 32] {
    static mut BUFFER: [u8; 32] = [0u8; 32];
    unsafe { &mut BUFFER }
}
