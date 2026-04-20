//! AHT10 ambient temperature & humidity sensor over I2C.
//!
//! Protocol (mirrors the Python smbus2 implementation):
//!   Init    : write [0xE1, 0x08, 0x00]  →  wait 20 ms
//!   Trigger : write [0xAC, 0x33, 0x00]  →  wait 80 ms
//!   Read    : read 6 bytes
//!             byte 0        — status  (bit 7 = busy)
//!             bytes 1-3[hi] — humidity raw (20 bits)
//!             bytes 3[lo]-5 — temperature raw (20 bits)

use crate::config;
use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use std::thread;
use std::time::Duration;

const INIT_CMD:  [u8; 3] = [0xE1, 0x08, 0x00];
const TRIG_CMD:  [u8; 3] = [0xAC, 0x33, 0x00];
const BUSY_FLAG: u8      = 0x80;

/// Ambient temperature and humidity from the AHT10.
#[derive(Debug, Clone)]
pub struct AmbientData {
    pub temperature: f32,  // °C
    pub humidity:    f32,  // %
}

/// I2C reader for the AHT10 sensor.
/// Handles open, init, retry, reconnect, and offline state transitions.
pub struct Aht10Reader {
    bus_path:    String,
    address:     u16,
    device:      Option<LinuxI2CDevice>,
    initialized: bool,
    online:      bool,
}

impl Aht10Reader {
    pub fn new(bus_path: &str, address: u16) -> Self {
        Aht10Reader {
            bus_path:    bus_path.to_string(),
            address,
            device:      None,
            initialized: false,
            online:      false,
        }
    }

    fn open_device(&self) -> Result<LinuxI2CDevice, String> {
        LinuxI2CDevice::new(&self.bus_path, self.address)
            .map_err(|e| format!("cannot open {}: {e}", self.bus_path))
    }

    pub fn read(&mut self) -> Result<AmbientData, String> {
        for attempt in 0..config::MAX_RETRIES {
            if attempt > 0 {
                thread::sleep(Duration::from_millis(config::RETRY_DELAY_MS));
            }

            // Open I2C device handle if missing
            if self.device.is_none() {
                match self.open_device() {
                    Ok(dev) => {
                        log::info!("[AHT10] opened {}", self.bus_path);
                        self.device      = Some(dev);
                        self.initialized = false;
                    }
                    Err(e) => {
                        log::warn!(
                            "[AHT10] connect attempt {}/{}: {}",
                            attempt + 1, config::MAX_RETRIES, e
                        );
                        continue;
                    }
                }
            }

            let dev = self.device.as_mut().unwrap();

            // Send init command once after every (re-)open
            if !self.initialized {
                if let Err(e) = dev.write(&INIT_CMD) {
                    log::warn!("[AHT10] init failed: {e}");
                    self.device = None;
                    continue;
                }
                thread::sleep(Duration::from_millis(config::AHT10_INIT_DELAY_MS));
                self.initialized = true;
            }

            // Trigger measurement
            if let Err(e) = dev.write(&TRIG_CMD) {
                log::warn!("[AHT10] trigger write failed: {e}");
                self.device      = None;
                self.initialized = false;
                continue;
            }
            thread::sleep(Duration::from_millis(config::AHT10_MEAS_DELAY_MS));

            // Read 6 bytes
            let mut buf = [0u8; 6];
            if let Err(e) = dev.read(&mut buf) {
                log::warn!("[AHT10] read failed: {e}");
                self.device      = None;
                self.initialized = false;
                continue;
            }

            // Sensor reports busy — retry without reopening
            if buf[0] & BUSY_FLAG != 0 {
                log::warn!(
                    "[AHT10] sensor busy (attempt {}/{})",
                    attempt + 1, config::MAX_RETRIES
                );
                continue;
            }

            // ── Parse humidity (bits [19:0] of bytes 1-3) ─────────────────────
            // byte1[7:0] = hum[19:12]
            // byte2[7:0] = hum[11:4]
            // byte3[7:4] = hum[3:0]
            let hum_raw = ((buf[1] as u32) << 12)
                        | ((buf[2] as u32) << 4)
                        | ((buf[3] as u32) >> 4);
            let humidity = (hum_raw as f32 / 1_048_576.0) * 100.0;

            // ── Parse temperature (bits [19:0] of bytes 3-5) ──────────────────
            // byte3[3:0] = temp[19:16]
            // byte4[7:0] = temp[15:8]
            // byte5[7:0] = temp[7:0]
            let temp_raw = (((buf[3] & 0x0F) as u32) << 16)
                         | ((buf[4] as u32) << 8)
                         |  (buf[5] as u32);
            let temperature = (temp_raw as f32 / 1_048_576.0) * 200.0 - 50.0;

            if !self.online {
                log::info!("[AHT10] device back ONLINE");
                self.online = true;
            }
            return Ok(AmbientData { temperature, humidity });
        }

        if self.online {
            log::error!(
                "[AHT10] device OFFLINE — no response after {} attempts",
                config::MAX_RETRIES
            );
            self.online = false;
        } else {
            log::debug!("[AHT10] device still OFFLINE");
        }
        Err(format!("AHT10 offline ({} attempts exhausted)", config::MAX_RETRIES))
    }
}
