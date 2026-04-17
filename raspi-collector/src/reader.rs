use crate::{config, modbus, sensor::SensorData};
use serialport::SerialPort;
use std::thread;
use std::time::Duration;

// ── Trait ─────────────────────────────────────────────────────────────────────
pub trait SensorReader {
    #[allow(dead_code)]
    fn name(&self) -> &str;
    fn read(&mut self) -> Result<SensorData, String>;
}

// ── ModbusReader — works for both RS485 hardware UART and USB UART adapters ───
pub struct ModbusReader {
    name:          String,
    port_path:     String,
    baud_rate:     u32,
    slave_address: u8,
    port:          Option<Box<dyn SerialPort>>,
    /// Tracks last known state so we only log on transitions (no spam).
    online:        bool,
}

impl ModbusReader {
    pub fn new(name: &str, port_path: &str, baud_rate: u32, slave_address: u8) -> Self {
        ModbusReader {
            name:          name.to_string(),
            port_path:     port_path.to_string(),
            baud_rate,
            slave_address,
            port:          None,
            online:        false,
        }
    }

    fn open_port(&self) -> Result<Box<dyn SerialPort>, String> {
        serialport::new(&self.port_path, self.baud_rate)
            .parity(serialport::Parity::None)
            .data_bits(serialport::DataBits::Eight)
            .stop_bits(serialport::StopBits::One)
            .timeout(Duration::from_millis(config::SERIAL_TIMEOUT_MS))
            .open()
            .map_err(|e| format!("cannot open {}: {e}", self.port_path))
    }
}

impl SensorReader for ModbusReader {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self) -> Result<SensorData, String> {
        for attempt in 0..config::MAX_RETRIES {
            if attempt > 0 {
                thread::sleep(Duration::from_millis(config::RETRY_DELAY_MS));
            }

            // Reconnect if the port slot is empty (first run or after an IO error)
            if self.port.is_none() {
                match self.open_port() {
                    Ok(p) => {
                        log::info!("[{}] opened {}", self.name, self.port_path);
                        self.port = Some(p);
                    }
                    Err(e) => {
                        log::warn!(
                            "[{}] connect attempt {}/{}: {}",
                            self.name, attempt + 1, config::MAX_RETRIES, e
                        );
                        continue;
                    }
                }
            }

            let port = self.port.as_mut().unwrap();
            match modbus::poll_registers(
                port,
                self.slave_address,
                config::START_REGISTER,
                config::NUM_REGISTERS,
                config::TIMEOUT_MS,
            ) {
                Ok(regs) => {
                    if !self.online {
                        log::info!("[{}] device back ONLINE", self.name);
                        self.online = true;
                    }
                    return Ok(SensorData::from_registers(&regs, self.name.clone()));
                }
                Err(e) => {
                    log::warn!(
                        "[{}] attempt {}/{}: {}",
                        self.name, attempt + 1, config::MAX_RETRIES, e
                    );
                    // IO-level failures mean the port handle is dead; drop it so
                    // the next attempt triggers a fresh open().
                    if e.starts_with("Read error")
                        || e.starts_with("Write error")
                        || e.starts_with("Buffer clear")
                    {
                        log::warn!("[{}] IO error — will reconnect", self.name);
                        self.port = None;
                    }
                }
            }
        }

        // Only log the transition to avoid flooding on every poll cycle
        if self.online {
            log::error!("[{}] device OFFLINE — no response after {} attempts", self.name, config::MAX_RETRIES);
            self.online = false;
        } else {
            log::debug!("[{}] device still OFFLINE", self.name);
        }

        Err(format!("device offline ({} attempts exhausted)", config::MAX_RETRIES))
    }
}
