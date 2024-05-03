//! # Reading of eFuses (ESP32-S2)
//!
//! ## Overview
//!
//! The `efuse` module provides functionality for reading eFuse data
//! from the `ESP32-S2` chip, allowing access to various chip-specific
//! information such as :
//!   * MAC address
//!   * core count
//!   * CPU frequency
//!   * chip type
//!
//! and more. It is useful for retrieving chip-specific configuration and
//! identification data during runtime.
//!
//! The `Efuse` struct represents the eFuse peripheral and is responsible for
//! reading various eFuse fields and values.
//!
//! ## Example
//!
//! ### Read chip's MAC address from the eFuse storage.
//! ```no_run
//! let mac_address = Efuse::get_mac_address();
//! writeln!(
//!     serial_tx,
//!     "MAC: {:#X}:{:#X}:{:#X}:{:#X}:{:#X}:{:#X}",
//!     mac_address[0],
//!     mac_address[1],
//!     mac_address[2],
//!     mac_address[3],
//!     mac_address[4],
//!     mac_address[5]
//! );
//! ```

pub use self::fields::*;
use crate::peripherals::EFUSE;

mod fields;

pub struct Efuse;

impl Efuse {
    /// Reads chip's MAC address from the eFuse storage.
    ///
    /// # Example
    ///
    /// ```
    /// let mac_address = Efuse::get_mac_address();
    /// writeln!(
    ///     serial_tx,
    ///     "MAC: {:#X}:{:#X}:{:#X}:{:#X}:{:#X}:{:#X}",
    ///     mac_address[0],
    ///     mac_address[1],
    ///     mac_address[2],
    ///     mac_address[3],
    ///     mac_address[4],
    ///     mac_address[5]
    /// );
    /// ```
    pub fn read_base_mac_address() -> [u8; 6] {
        Self::read_field_be(MAC)
    }

    /// Get status of SPI boot encryption.
    pub fn get_flash_encryption() -> bool {
        (Self::read_field_le::<u8>(SPI_BOOT_CRYPT_CNT).count_ones() % 2) != 0
    }

    /// Get the multiplier for the timeout value of the RWDT STAGE 0 register.
    pub fn get_rwdt_multiplier() -> u8 {
        Self::read_field_le::<u8>(WDT_DELAY_SEL)
    }

    /// Get efuse block version
    ///
    /// see <https://github.com/espressif/esp-idf/blob/dc016f5987/components/hal/efuse_hal.c#L27-L30>
    pub fn get_block_version() -> (u8, u8) {
        // see <https://github.com/espressif/esp-idf/blob/dc016f5987/components/hal/esp32s3/include/hal/efuse_ll.h#L65-L73>
        // <https://github.com/espressif/esp-idf/blob/903af13e8/components/efuse/esp32s3/esp_efuse_table.csv#L196>
        (
            Self::read_field_le::<u8>(BLK_VERSION_MAJOR),
            Self::read_field_le::<u8>(BLK_VERSION_MINOR),
        )
    }

    // TODO: Missing esp_efuse_rtc_calib_get_tsens_val (https://github.com/espressif/esp-idf/blob/903af13e8/components/efuse/esp32s2/esp_efuse_rtc_calib.c#L150)
    // S3 equivalent: https://github.com/espressif/esp-idf/blob/903af13e8/components/efuse/esp32s3/esp_efuse_rtc_calib.c#L95
    pub fn get_rtc_calib_init_code(unit: u8, atten: Attenuation) -> Option<u16> {
        // esp_efuse_rtc_table_read_calib_version just calls
        // efuse_ll_get_blk_version_minor
        let minor_version = Self::read_field_le::<u8>(BLK_VERSION_MINOR);
        if minor_version != 1 && minor_version != 2 {
            return None;
        }
        // BLOCK 2
        // BEGIN_BIT 135
        // LENGTH 9
        // MULTIPLIER 4
        // OFFSET BASE 0
        // OFFSET DEP 0
        const RTCCALIB_IDX_TMPSENSOR = 33;
        let tsens_cal = esp_efuse_rtc_table_get_parsed_efuse_value(RTCCALIB_IDX_TMPSENSOR, false);

        Some (tsens_cal)
    }

    // components/efuse/esp32s2/esp32_efuese_rtc_table.c::145
    pub fn esp_efuse_rtc_table_get_parsed_efuse_value(tag: u8, skip_efuse_reading: false) -> u32 {
        if tag == 0 {
            return 0; // tag 0 is the dummy tag and has no value. (used by depends)
        }
        let mut efuse_val  = 0;
        if !skip_efuse_reading {
            efuse_val =  esp_efuse_rtc_table_get_raw_efuse_value(tag) * 4; // 4 = multiplier
        }

        let result  = efuse_val + 0 + 0; // efuse val + base + dep

        result
    }

    pub fn esp_efuse_rtc_table_get_raw_efuse_value(tag: u32) -> u32 {
        if tag == 0 {
            return 0;
        }
        let mut val = 0;
        //  esp_efuse_read_block(adc_efuse_raw_map[tag].block, &val, adc_efuse_raw_map[tag].begin_bit, adc_efuse_raw_map[tag].length);



    }
}

#[derive(Copy, Clone)]
pub(crate) enum EfuseBlock {
    Block0,
    Block1,
    Block2,
    Block3,
    Block4,
    Block5,
    Block6,
    Block7,
    Block8,
    Block9,
    Block10,
}

impl EfuseBlock {
    pub(crate) fn address(self) -> *const u32 {
        use EfuseBlock::*;
        let efuse = unsafe { &*EFUSE::ptr() };
        match self {
            Block0 => efuse.rd_wr_dis().as_ptr(),
            Block1 => efuse.rd_mac_spi_sys_0().as_ptr(),
            Block2 => efuse.rd_sys_data_part1_(0).as_ptr(),
            Block3 => efuse.rd_usr_data(0).as_ptr(),
            Block4 => efuse.rd_key0_data(0).as_ptr(),
            Block5 => efuse.rd_key1_data(0).as_ptr(),
            Block6 => efuse.rd_key2_data(0).as_ptr(),
            Block7 => efuse.rd_key3_data(0).as_ptr(),
            Block8 => efuse.rd_key4_data(0).as_ptr(),
            Block9 => efuse.rd_key5_data(0).as_ptr(),
            Block10 => efuse.rd_sys_data_part2_(0).as_ptr(),
        }
    }
}
