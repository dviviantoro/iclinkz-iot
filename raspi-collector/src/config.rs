// ── RS485 hardware serial (RPi GPIO UART via RS485 transceiver) ────────────────
pub const RS485_PORT:  &str = "/dev/ttyS0";
pub const RS485_BAUD:  u32  = 9600;
pub const RS485_SLAVE: u8   = 10;

// ── USB UART JSON node (send "READ\n", receive newline-terminated JSON) ────────
pub const USB_PORT:    &str = "/dev/ttyUSB0";
pub const USB_BAUD:    u32  = 9600;

// ── Timing ────────────────────────────────────────────────────────────────────
pub const READ_INTERVAL_SECS: u64 = 5;
pub const TIMEOUT_MS:         u64 = 2000;  // overall response deadline
pub const SERIAL_TIMEOUT_MS:  u64 = 100;   // per read() syscall (non-blocking feel)
pub const RETRY_DELAY_MS:     u64 = 500;

// ── AHT10 ambient sensor (I2C, Linux only) ────────────────────────────────────
pub const I2C_BUS:    &str = "/dev/i2c-1";
pub const AHT10_ADDR: u16  = 0x38;
#[cfg(target_os = "linux")]
pub const AHT10_INIT_DELAY_MS: u64 = 20;   // datasheet: ≥10 ms after init cmd
#[cfg(target_os = "linux")]
pub const AHT10_MEAS_DELAY_MS: u64 = 80;   // datasheet: ≥75 ms after trigger

// ── Modbus ────────────────────────────────────────────────────────────────────
pub const MAX_RETRIES:    u32  = 3;
pub const NUM_REGISTERS:  usize = 10;
pub const START_REGISTER: u16  = 0x0000;

// ── Flow meter (webcam + Groq OCR) ────────────────────────────────────────────
// Interval between captures is driven by CRON_TIME in the .env file.
// parse_cron_hours() extracts the step from the hours field (e.g. "*/6" → 6 h).
pub fn parse_cron_hours(cron_time: &str) -> Option<u64> {
    // Format: "MIN HOUR DOM MON DOW"  e.g. "0 */6 * * *"
    let hour_field = cron_time.split_whitespace().nth(1)?;
    if let Some(step) = hour_field.strip_prefix("*/") {
        return step.parse().ok();
    }
    None
}
