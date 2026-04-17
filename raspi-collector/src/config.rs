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

// ── Modbus ────────────────────────────────────────────────────────────────────
pub const MAX_RETRIES:    u32  = 3;
pub const NUM_REGISTERS:  usize = 10;
pub const START_REGISTER: u16  = 0x0000;
