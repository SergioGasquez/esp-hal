//! # Secure Hash Algorithm peripheral driver
//!
//! ## Overview
//! This SHA (Secure Hash Algorithm) driver for ESP chips is a software module
//! that provides an interface to interact with the SHA peripheral on ESP
//! microcontroller chips. This driver allows you to perform cryptographic hash
//! operations using various hash algorithms supported by the SHA peripheral,
//! such as:
//!    * SHA-1
//!    * SHA-224
//!    * SHA-256
//!    * SHA-384
//!    * SHA-512
//!
//! The driver supports two working modes:
//!    * Typical SHA
//!    * DMA-SHA (Direct Memory Access SHA is NOT implemented yet).
//!
//! It provides functions to update the hash calculation with input data, finish
//! the hash calculation and retrieve the resulting hash value. The SHA
//! peripheral on ESP chips can handle large data streams efficiently, making it
//! suitable for cryptographic applications that require secure hashing.
//!
//! To use the SHA Peripheral Driver, you need to initialize it with the desired
//! SHA mode and the corresponding SHA peripheral. Once initialized, you can
//! update the hash calculation by providing input data, finish the calculation
//! to retrieve the hash value and repeat the process for a new hash calculation
//! if needed.
//!
//! ## Example
//! ```no_run
//! let source_data = "HELLO, ESPRESSIF!".as_bytes();
//! let mut remaining = source_data.clone();
//! let mut hasher = Sha::new(
//!     peripherals.SHA,
//!     ShaMode::SHA256,
//!     &mut system.peripheral_clock_control,
//! );
//!
//! // Short hashes can be created by decreasing the output buffer to the desired
//! // length
//! let mut output = [0u8; 32];
//!
//! while remaining.len() > 0 {
//!     // All the HW Sha functions are infallible so unwrap is fine to use if you use
//!     // block!
//!     remaining = block!(hasher.update(remaining)).unwrap();
//! }
//!
//! // Finish can be called as many times as desired to get multiple copies of the
//! // output.
//! block!(hasher.finish(output.as_mut_slice())).unwrap();
//!
//! println!("SHA256 Hash output {:02x?}", output);
//!
//! let mut hasher = Sha256::new();
//! hasher.update(source_data);
//! let soft_result = hasher.finalize();
//!
//! println!("SHA256 Hash output {:02x?}", soft_result);
//! ```

use core::convert::Infallible;

use crate::{
    dma::DmaError,
    peripheral::{Peripheral, PeripheralRef},
    peripherals::SHA,
    reg_access::AlignmentHelper,
    system::PeripheralClockControl,
};

#[allow(unused)]
const MAX_DMA_SIZE: usize = 32736;

#[derive(Debug, Clone, Copy)]
pub enum Error {
    DmaError(DmaError),
    MaxDmaTransferSizeExceeded,
    Unknown,
}

impl From<DmaError> for Error {
    fn from(value: DmaError) -> Self {
        Error::DmaError(value)
    }
}

// All the hash algorithms introduced in FIPS PUB 180-4 Spec.
// – SHA-1
// – SHA-224
// – SHA-256
// – SHA-384
// – SHA-512
// – SHA-512/224
// – SHA-512/256
// – SHA-512/t (not implemented yet)
// Two working modes
// – Typical SHA
// – DMA-SHA (not implemented yet)

pub struct Sha<'d> {
    sha: PeripheralRef<'d, SHA>,
    mode: ShaMode,
    operation_mode: OperationMode,
    alignment_helper: AlignmentHelper,
    cursor: usize,
    first_run: bool,
    finished: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum ShaMode {
    SHA1,
    #[cfg(not(esp32))]
    SHA224,
    SHA256,
    #[cfg(any(esp32s2, esp32s3, esp32))]
    SHA384,
    #[cfg(any(esp32s2, esp32s3, esp32))]
    SHA512,
    #[cfg(any(esp32s2, esp32s3))]
    SHA512_224,
    #[cfg(any(esp32s2, esp32s3))]
    SHA512_256,
    // SHA512_(u16) // Max 511
}

#[derive(Debug, Clone, Copy)]
pub enum OperationMode {
    Typical,
    // ESP32 does not support DMA mode
    #[cfg(not(esp32))]
    DMA,
}

// TODO: Maybe make Sha Generic (Sha<Mode>) in order to allow for better
// compiler optimizations? (Requires complex const generics which isn't stable
// yet)

#[cfg(not(esp32))]
fn mode_as_bits(mode: ShaMode) -> u8 {
    match mode {
        ShaMode::SHA1 => 0,
        ShaMode::SHA224 => 1,
        ShaMode::SHA256 => 2,
        #[cfg(any(esp32s2, esp32s3))]
        ShaMode::SHA384 => 3,
        #[cfg(any(esp32s2, esp32s3))]
        ShaMode::SHA512 => 4,
        #[cfg(any(esp32s2, esp32s3))]
        ShaMode::SHA512_224 => 5,
        #[cfg(any(esp32s2, esp32s3))]
        ShaMode::SHA512_256 => 6,
        // _ => 0 // TODO: SHA512/t
    }
}

// TODO: Allow/Implemenet SHA512_(u16)

// A few notes on this implementation with regards to 'memcpy',
// - It seems that ptr::write_bytes already acts as volatile, while ptr::copy_*
//   does not (in this case)
// - The registers are *not* cleared after processing, so padding needs to be
//   written out
// - This component uses core::intrinsics::volatile_* which is unstable, but is
//   the only way to
// efficiently copy memory with volatile
// - For this particular registers (and probably others), a full u32 needs to be
//   written partial
// register writes (i.e. in u8 mode) does not work
//   - This means that we need to buffer bytes coming in up to 4 u8's in order
//     to create a full u32

// This implementation might fail after u32::MAX/8 bytes, to increase please see
// ::finish() length/self.cursor usage
impl<'d> Sha<'d> {
    pub fn new(
        sha: impl Peripheral<P = SHA> + 'd,
        mode: ShaMode,
        peripheral_clock_control: &mut PeripheralClockControl,
    ) -> Self {
        crate::into_ref!(sha);
        peripheral_clock_control.enable(crate::system::Peripheral::Sha);

        // Setup SHA Mode
        #[cfg(not(esp32))]
        sha.mode
            .write(|w| unsafe { w.mode().bits(mode_as_bits(mode)) });

        Self {
            sha,
            mode,
            operation_mode: OperationMode::Typical,
            cursor: 0,
            first_run: true,
            finished: false,
            alignment_helper: AlignmentHelper::default(),
        }
    }

    pub fn first_run(&self) -> bool {
        self.first_run
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    #[cfg(not(esp32))]
    fn process_buffer(&mut self) {
        match self.operation_mode {
            OperationMode::Typical => {
                if self.first_run {
                    // Set SHA_START_REG
                    self.sha.start.write(|w| unsafe { w.bits(1) });
                    self.first_run = false;
                } else {
                    // SET SHA_CONTINUE_REG
                    self.sha.continue_.write(|w| unsafe { w.bits(1) });
                }
            }
            OperationMode::DMA => {
                if self.first_run {
                    // Set SHA_DMA_START_REG
                    self.sha.dma_start.write(|w| unsafe { w.bits(1) });
                    // TODO clear_dma_interrupts(); ?
                    // Set SHA_DMA_INT_ENA_REG
                    self.sha.irq_ena.write(|w| unsafe { w.bits(1) });
                    // Set the number of blocks in DMA_BLOCK_NUM
                    self.sha
                        .dma_block_num
                        .write(|w| unsafe { w.bits(self.chunk_length() as u32) });

                    self.first_run = false;
                } else {
                    // SET SHA_DMA_CONTINUE_REG
                    self.sha.dma_continue.write(|w| unsafe { w.bits(1) });
                }
            }
        }
    }

    #[cfg(esp32)]
    fn process_buffer(&mut self) {
        if self.first_run {
            match self.mode {
                ShaMode::SHA1 => self.sha.sha1_start.write(|w| unsafe { w.bits(1) }),
                ShaMode::SHA256 => self.sha.sha256_start.write(|w| unsafe { w.bits(1) }),
                ShaMode::SHA384 => self.sha.sha384_start.write(|w| unsafe { w.bits(1) }),
                ShaMode::SHA512 => self.sha.sha512_start.write(|w| unsafe { w.bits(1) }),
            }
            self.first_run = false;
        } else {
            match self.mode {
                ShaMode::SHA1 => self.sha.sha1_continue.write(|w| unsafe { w.bits(1) }),
                ShaMode::SHA256 => self.sha.sha256_continue.write(|w| unsafe { w.bits(1) }),
                ShaMode::SHA384 => self.sha.sha384_continue.write(|w| unsafe { w.bits(1) }),
                ShaMode::SHA512 => self.sha.sha512_continue.write(|w| unsafe { w.bits(1) }),
            }
        }
    }

    // Return block size (in bytes) for a given SHA type
    fn chunk_length(&self) -> usize {
        return match self.mode {
            // 512 bits (64 bytes) blocks
            ShaMode::SHA1 | ShaMode::SHA256 => 64,
            #[cfg(not(esp32))]
            ShaMode::SHA224 => 64,
            #[cfg(not(any(esp32c2, esp32c3, esp32c6, esp32h2)))]
            _ => 128,
        };
    }

    #[cfg(esp32)]
    fn is_busy(&self) -> bool {
        match self.mode {
            ShaMode::SHA1 => self.sha.sha1_busy.read().sha1_busy().bit_is_set(),
            ShaMode::SHA256 => self.sha.sha256_busy.read().sha256_busy().bit_is_set(),
            ShaMode::SHA384 => self.sha.sha384_busy.read().sha384_busy().bit_is_set(),
            ShaMode::SHA512 => self.sha.sha512_busy.read().sha512_busy().bit_is_set(),
        }
    }

    #[cfg(not(esp32))]
    fn is_busy(&self) -> bool {
        self.sha.busy.read().bits() != 0
    }

    // Return the output length (in bytes) for a given SHA type
    pub fn digest_length(&self) -> usize {
        match self.mode {
            ShaMode::SHA1 => 20,
            #[cfg(not(esp32))]
            ShaMode::SHA224 => 28,
            ShaMode::SHA256 => 32,
            #[cfg(any(esp32, esp32s2, esp32s3))]
            ShaMode::SHA384 => 48,
            #[cfg(any(esp32, esp32s2, esp32s3))]
            ShaMode::SHA512 => 64,
            #[cfg(any(esp32s2, esp32s3))]
            ShaMode::SHA512_224 => 28,
            #[cfg(any(esp32s2, esp32s3))]
            ShaMode::SHA512_256 => 32,
        }
    }

    // Flush partial data, ensures aligned cursor
    fn flush_data(&mut self) -> nb::Result<(), Infallible> {
        if self.is_busy() {
            return Err(nb::Error::WouldBlock);
        }

        let chunk_len = self.chunk_length();

        let flushed = self.alignment_helper.flush_to(
            #[cfg(esp32)]
            &mut self.sha.text,
            #[cfg(not(esp32))]
            &mut self.sha.m_mem,
            (self.cursor % chunk_len) / self.alignment_helper.align_size(),
        );

        self.cursor = self.cursor.wrapping_add(flushed);
        if flushed > 0 && self.cursor % chunk_len == 0 {
            self.process_buffer();
            while self.is_busy() {}
        }

        Ok(())
    }

    // This function ensures that incoming data is aligned to u32 (due to issues
    // with cpy_mem<u8>)
    fn write_data<'a>(&mut self, incoming: &'a [u8]) -> nb::Result<&'a [u8], Infallible> {
        let mod_cursor = self.cursor % self.chunk_length();

        let chunk_len = self.chunk_length();

        let (remaining, bound_reached) = self.alignment_helper.aligned_volatile_copy(
            #[cfg(esp32)]
            &mut self.sha.text,
            #[cfg(not(esp32))]
            &mut self.sha.m_mem,
            incoming,
            chunk_len / self.alignment_helper.align_size(),
            mod_cursor / self.alignment_helper.align_size(),
        );

        self.cursor = self.cursor.wrapping_add(incoming.len() - remaining.len());

        if bound_reached {
            self.process_buffer();
        }

        Ok(remaining)
    }

    pub fn update<'a>(&mut self, buffer: &'a [u8]) -> nb::Result<&'a [u8], Infallible> {
        if self.is_busy() {
            return Err(nb::Error::WouldBlock);
        }

        self.finished = false;

        let remaining = self.write_data(buffer)?;

        Ok(remaining)
    }

    // Finish of the calculation (if not already) and copy result to output
    // After `finish()` is called `update()`s will contribute to a new hash which
    // can be calculated again with `finish()`.
    //
    // Typically output is expected to be the size of digest_length(), but smaller
    // inputs can be given to get a "short hash"
    pub fn finish(&mut self, output: &mut [u8]) -> nb::Result<(), Infallible> {
        // The main purpose of this function is to dynamically generate padding for the
        // input. Padding: Append "1" bit, Pad zeros until 512/1024 filled
        // then set the message length in the LSB (overwriting the padding)
        // If not enough free space for length+1, add length at end of a new zero'd
        // block

        if self.is_busy() {
            return Err(nb::Error::WouldBlock);
        }

        let chunk_len = self.chunk_length();

        // Store message length for padding
        let length = (self.cursor as u64 * 8).to_be_bytes();
        nb::block!(self.update(&[0x80]))?; // Append "1" bit
        nb::block!(self.flush_data())?; // Flush partial data, ensures aligned cursor
        debug_assert!(self.cursor % 4 == 0);

        let mod_cursor = self.cursor % chunk_len;
        if (chunk_len - mod_cursor) < core::mem::size_of::<u64>() {
            // Zero out remaining data if buffer is almost full (>=448/896), and process
            // buffer
            let pad_len = chunk_len - mod_cursor;
            self.alignment_helper.volatile_write_bytes(
                #[cfg(esp32)]
                &mut self.sha.text,
                #[cfg(not(esp32))]
                &mut self.sha.m_mem,
                0_u8,
                pad_len / self.alignment_helper.align_size(),
                mod_cursor / self.alignment_helper.align_size(),
            );
            self.process_buffer();
            self.cursor = self.cursor.wrapping_add(pad_len);

            debug_assert_eq!(self.cursor % chunk_len, 0);

            // Spin-wait for finish
            while self.is_busy() {}
        }

        let mod_cursor = self.cursor % chunk_len; // Should be zero if branched above
        let pad_len = chunk_len - mod_cursor - core::mem::size_of::<u64>();

        self.alignment_helper.volatile_write_bytes(
            #[cfg(esp32)]
            &mut self.sha.text,
            #[cfg(not(esp32))]
            &mut self.sha.m_mem,
            0_u8,
            pad_len / self.alignment_helper.align_size(),
            mod_cursor / self.alignment_helper.align_size(),
        );

        self.alignment_helper.aligned_volatile_copy(
            #[cfg(esp32)]
            &mut self.sha.text,
            #[cfg(not(esp32))]
            &mut self.sha.m_mem,
            &length,
            chunk_len / self.alignment_helper.align_size(),
            (chunk_len - core::mem::size_of::<u64>()) / self.alignment_helper.align_size(),
        );

        self.process_buffer();
        // Spin-wait for final buffer to be processed
        while self.is_busy() {}

        // ESP32 requires additional load to retrieve output
        #[cfg(esp32)]
        {
            match self.mode {
                ShaMode::SHA1 => unsafe { self.sha.sha1_load.write(|w| w.bits(1)) },
                ShaMode::SHA256 => unsafe { self.sha.sha256_load.write(|w| w.bits(1)) },
                ShaMode::SHA384 => unsafe { self.sha.sha384_load.write(|w| w.bits(1)) },
                ShaMode::SHA512 => unsafe { self.sha.sha512_load.write(|w| w.bits(1)) },
            }

            // Spin wait for result, 8-20 clock cycles according to manual
            while self.is_busy() {}
        }

        self.alignment_helper.volatile_read_regset(
            #[cfg(esp32)]
            &self.sha.text[0],
            #[cfg(not(esp32))]
            &self.sha.h_mem[0],
            output,
            core::cmp::min(output.len(), 32) / self.alignment_helper.align_size(),
        );

        self.first_run = true;
        self.cursor = 0;
        self.alignment_helper.reset();

        Ok(())
    }
}

pub mod dma {
    use core::mem;

    use embedded_dma::WriteBuffer;

    use super::{OperationMode, Sha, MAX_DMA_SIZE};
    use crate::dma::{
        Channel,
        ChannelTypes,
        DmaError,
        DmaPeripheral,
        DmaTransfer,
        DmaTransferRxTx,
        // Rx,
        RxPrivate,
        ShaPeripheral,
        // Tx,
        TxPrivate,
    };

    pub trait WithDmaSha<'d, C>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        fn with_dma(self, channel: Channel<'d, C>) -> ShaDma<'d, C>;
    }

    impl<'d, C> WithDmaSha<'d, C> for Sha<'d>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        fn with_dma(mut self, mut channel: Channel<'d, C>) -> ShaDma<'d, C> {
            channel.tx.init_channel(); // no need to call this for both, TX and RX
            self.operation_mode = OperationMode::DMA;
            ShaDma { sha: self, channel }
        }
    }

    /// An in-progress DMA transfer
    pub struct ShaDmaTransferRxTx<'d, C, RBUFFER, TBUFFER>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        sha_dma: ShaDma<'d, C>,
        rbuffer: RBUFFER,
        tbuffer: TBUFFER,
    }

    impl<'d, C, RXBUF, TXBUF> DmaTransferRxTx<RXBUF, TXBUF, ShaDma<'d, C>>
        for ShaDmaTransferRxTx<'d, C, RXBUF, TXBUF>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        /// Wait for the DMA transfer to complete and return the buffers and the
        /// SHA instance.
        fn wait(
            self,
        ) -> Result<(RXBUF, TXBUF, ShaDma<'d, C>), (DmaError, RXBUF, TXBUF, ShaDma<'d, C>)>
        {
            // todo!();
            // self.sha_dma.sha.flush_data(); // waiting for the DMA transfer is not enough

            // `DmaTransfer` needs to have a `Drop` implementation, because we accept
            // managed buffers that can free their memory on drop. Because of that
            // we can't move out of the `DmaTransfer`'s fields, so we use `ptr::read`
            // and `mem::forget`.
            //
            // NOTE(unsafe) There is no panic branch between getting the resources
            // and forgetting `self`.
            unsafe {
                let rbuffer = core::ptr::read(&self.rbuffer);
                let tbuffer = core::ptr::read(&self.tbuffer);
                let payload = core::ptr::read(&self.sha_dma);
                let err = (&self).sha_dma.channel.rx.has_error()
                    || (&self).sha_dma.channel.tx.has_error();
                mem::forget(self);
                if err {
                    Err((DmaError::DescriptorError, rbuffer, tbuffer, payload))
                } else {
                    Ok((rbuffer, tbuffer, payload))
                }
            }
        }

        /// Check if the DMA transfer is complete
        fn is_done(&self) -> bool {
            let ch = &self.sha_dma.channel;
            ch.tx.is_done() && ch.rx.is_done()
        }
    }

    impl<'d, C, RXBUF, TXBUF> Drop for ShaDmaTransferRxTx<'d, C, RXBUF, TXBUF>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        fn drop(&mut self) {
            // todo!();
            // self.sha_dma.sha.flush_data();
        }
    }

    /// An in-progress DMA transfer.
    pub struct ShaDmaTransfer<'d, C, BUFFER>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        sha_dma: ShaDma<'d, C>,
        buffer: BUFFER,
    }

    impl<'d, C, BUFFER> DmaTransfer<BUFFER, ShaDma<'d, C>> for ShaDmaTransfer<'d, C, BUFFER>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        /// Wait for the DMA transfer to complete and return the buffers and the
        /// SHA instance.
        fn wait(self) -> Result<(BUFFER, ShaDma<'d, C>), (DmaError, BUFFER, ShaDma<'d, C>)> {
            // todo!();
            // self.sha_dma.sha.flush_data(); // waiting for the DMA transfer is not enough

            // `DmaTransfer` needs to have a `Drop` implementation, because we accept
            // managed buffers that can free their memory on drop. Because of that
            // we can't move out of the `DmaTransfer`'s fields, so we use `ptr::read`
            // and `mem::forget`.
            //
            // NOTE(unsafe) There is no panic branch between getting the resources
            // and forgetting `self`.
            unsafe {
                let buffer = core::ptr::read(&self.buffer);
                let payload = core::ptr::read(&self.sha_dma);
                let err = (&self).sha_dma.channel.rx.has_error()
                    || (&self).sha_dma.channel.tx.has_error();
                mem::forget(self);
                if err {
                    Err((DmaError::DescriptorError, buffer, payload))
                } else {
                    Ok((buffer, payload))
                }
            }
        }

        /// Check if the DMA transfer is complete
        fn is_done(&self) -> bool {
            let ch = &self.sha_dma.channel;
            ch.tx.is_done() && ch.rx.is_done()
        }
    }

    impl<'d, C, BUFFER> Drop for ShaDmaTransfer<'d, C, BUFFER>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        fn drop(&mut self) {
            // self.sha_dma.sha.flush_data();
            // todo!();
        }
    }

    /// A DMA capable SHA instance.
    pub struct ShaDma<'d, C>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        pub(crate) sha: Sha<'d>,
        pub(crate) channel: Channel<'d, C>,
    }

    impl<'d, C> ShaDma<'d, C>
    where
        C: ChannelTypes,
        C::P: ShaPeripheral,
    {
        /// Perform a DMA write.
        ///
        /// This will return a [ShaDmaTransfer] owning the buffer(s) and the SHA
        /// instance. The maximum amount of data to be sent is 32736
        /// bytes.
        // Tentatively commented
        // pub fn dma_write<TXBUF>(
        //     mut self,
        //     words: TXBUF,
        // ) -> Result<ShaDmaTransfer<'d, C, TXBUF>, super::Error>
        // where
        //     TXBUF: ReadBuffer<Word = u8>,
        // {
        //     let (ptr, len) = unsafe { words.read_buffer() };

        //     if len > MAX_DMA_SIZE {
        //         return Err(super::Error::MaxDmaTransferSizeExceeded);
        //     }

        //     self.start_write_bytes_dma(ptr, len)?;
        //     Ok(ShaDmaTransfer {
        //         sha_dma: self,
        //         buffer: words,
        //     })
        // }

        /// Perform a DMA read.
        ///
        /// This will return a [ShaDmaTransfer] owning the buffer(s) and the SHA
        /// instance. The maximum amount of data to be received is 32736
        /// bytes.
        pub fn dma_read<RXBUF>(
            mut self,
            mut words: RXBUF,
        ) -> Result<ShaDmaTransfer<'d, C, RXBUF>, super::Error>
        where
            RXBUF: WriteBuffer<Word = u8>,
        {
            let (ptr, len) = unsafe { words.write_buffer() };

            if len > MAX_DMA_SIZE {
                return Err(super::Error::MaxDmaTransferSizeExceeded);
            }

            self.start_read_bytes_dma(ptr, len)?;
            Ok(ShaDmaTransfer {
                sha_dma: self,
                buffer: words,
            })
        }

        // Tentatively commented
        // fn write_bytes_dma<'w, TXBUF>(
        //     &mut self,
        //     words: &'w [u8],
        //     tx: &mut TXBUF,
        // ) -> Result<&'w [u8], super::Error>
        // where
        //     TXBUF: Tx,
        // {
        //     for chunk in words.chunks(MAX_DMA_SIZE) {
        //         self.start_write_bytes_dma(chunk.as_ptr(), chunk.len())?;

        //         while !tx.is_done() {}
        //         // todo!();
        //         // self.sha.flush_data(); // seems "is_done" doesn't work
        //         // as intended?
        //     }

        //     return Ok(words);
        // }

        // Tentatively commented
        // fn start_write_bytes_dma<'w>(
        //     &mut self,
        //     ptr: *const u8,
        //     len: usize,
        // ) -> Result<(), super::Error> { self.channel.tx.is_done();

        //     self.channel
        //         .tx
        //         .prepare_transfer(DmaPeripheral::Sha, false, ptr, len)?;

        //     self.clear_dma_interrupts();

        //     return Ok(());
        // }

        fn start_read_bytes_dma<'w>(
            &mut self,
            ptr: *mut u8,
            len: usize,
        ) -> Result<(), super::Error> {
            self.channel.rx.is_done();

            self.channel
                .rx
                .prepare_transfer(false, DmaPeripheral::Sha, ptr, len)?;

            self.clear_dma_interrupts();

            return Ok(());
        }

        #[cfg(not(esp32))]
        fn clear_dma_interrupts(&self) {
            self.sha.sha.clear_irq.write(|w| unsafe { w.bits(1) });
        }
    }
}
