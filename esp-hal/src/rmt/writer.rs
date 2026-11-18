#[cfg(place_rmt_driver_in_ram)]
use procmacros::ram;

use super::{DynChannelAccess, Error, PulseCode, Tx};

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(super) enum WriterState {
    // There's still data left to write
    Active,

    // An error occurred, and either
    // - no data was written (and transmission was never started), or
    // - some data was written and the last code written is an end marker, such that transmission
    //   will eventually stop
    Error(Error),

    // Completed without errors
    Done,
}

impl WriterState {
    pub(super) fn is_ok(self) -> bool {
        !matches!(self, Self::Error(_))
    }

    pub(super) fn to_result(self) -> Result<(), Error> {
        match self {
            // tx ended but writer wasn't done yet
            Self::Active => Err(Error::TransmissionError),
            Self::Error(e) => Err(e),
            Self::Done => Ok(()),
        }
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(super) struct RmtWriter {
    // Position of the end marker in channel RAM. The next code goes here.
    offset: u16,

    written: usize,

    last_code: PulseCode,

    state: WriterState,
}

// Copies `src` into the channel RAM ring, starting at `start` and wrapping at `memsize`.
#[inline(always)]
unsafe fn copy_to_ring(ram: *mut PulseCode, memsize: usize, start: usize, src: &[PulseCode]) {
    let first = (memsize - start).min(src.len());
    for (i, code) in src[..first].iter().enumerate() {
        unsafe { ram.add(start + i).write_volatile(*code) };
    }
    for (i, code) in src[first..].iter().enumerate() {
        unsafe { ram.add(i).write_volatile(*code) };
    }
}

impl RmtWriter {
    pub(super) fn new() -> Self {
        Self {
            offset: 0,
            written: 0,
            last_code: PulseCode::default(),
            state: WriterState::Active,
        }
    }

    #[inline]
    pub(super) fn state(&self) -> WriterState {
        self.state
    }

    // Copies codes from `data` to the channel RAM and advances `data`.
    //
    // An end marker always follows the last written code. If `initial` is set, this fills the
    // RAM except for that end marker. Otherwise, it appends half the RAM. The first new code
    // replaces the old end marker, and it is written last: the hardware reads either the old end
    // marker and stops, or the new code and continues. So a late call ends the transmission
    // cleanly instead of sending old RAM contents.
    #[cfg_attr(place_rmt_driver_in_ram, ram)]
    pub(super) fn write(
        &mut self,
        data: &mut &[PulseCode],
        raw: DynChannelAccess<Tx>,
        initial: bool,
    ) {
        if self.state != WriterState::Active {
            return;
        }

        debug_assert!(!initial || self.offset == 0);

        let ram = raw.channel_ram_start();
        let memsize = raw.memsize().codes();

        let max_count = if initial { memsize - 1 } else { memsize / 2 };
        let count = data.len().min(max_count);
        let offset = self.offset as usize;

        if count > 0 {
            unsafe {
                copy_to_ring(ram, memsize, (offset + 1) % memsize, &data[1..count]);
                ram.add((offset + count) % memsize)
                    .write_volatile(PulseCode::end_marker());
                ram.add(offset).write_volatile(data[0]);
            }
            self.last_code = data[count - 1];
        }

        self.offset = ((offset + count) % memsize) as u16;
        self.written += count;

        // The panic can never trigger since count <= data.len()!
        data.split_off(..count).unwrap();
        if data.is_empty() {
            self.state = if self.written == 0 {
                // data was empty
                WriterState::Error(Error::InvalidArgument)
            } else if self.last_code.is_end_marker() {
                WriterState::Done
            } else {
                // The driver's end marker stops the transmission.
                WriterState::Error(Error::EndMarkerMissing)
            };
        }
    }
}
