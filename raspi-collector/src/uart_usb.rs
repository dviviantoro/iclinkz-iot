use crate::{config, pump::PumpData};
use serialport::SerialPort;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// Reads a newline-terminated JSON line from a USB UART node.
///
/// Protocol:  → send `READ\n`
///            ← receive `{"ts":"…","pump1":{…},…}\n`
pub struct UartJsonReader {
    name:      String,
    port_path: String,
    baud_rate: u32,
    port:      Option<Box<dyn SerialPort>>,
    online:    bool,
}

impl UartJsonReader {
    pub fn new(name: &str, port_path: &str, baud_rate: u32) -> Self {
        UartJsonReader {
            name:      name.to_string(),
            port_path: port_path.to_string(),
            baud_rate,
            port:      None,
            online:    false,
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

    pub fn read(&mut self) -> Result<PumpData, String> {
        for attempt in 0..config::MAX_RETRIES {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(config::RETRY_DELAY_MS));
            }

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

            match poll_json(port, config::TIMEOUT_MS) {
                Ok(line) => match PumpData::from_json(&line, self.name.clone()) {
                    Ok(data) => {
                        if !self.online {
                            log::info!("[{}] device back ONLINE", self.name);
                            self.online = true;
                        }
                        return Ok(data);
                    }
                    Err(e) => {
                        log::warn!(
                            "[{}] attempt {}/{}: {}",
                            self.name, attempt + 1, config::MAX_RETRIES, e
                        );
                    }
                },
                Err(e) => {
                    log::warn!(
                        "[{}] attempt {}/{}: {}",
                        self.name, attempt + 1, config::MAX_RETRIES, e
                    );
                    if e.starts_with("Read error") || e.starts_with("Write error") {
                        log::warn!("[{}] IO error — will reconnect", self.name);
                        self.port = None;
                    }
                }
            }
        }

        if self.online {
            log::error!(
                "[{}] device OFFLINE — no response after {} attempts",
                self.name, config::MAX_RETRIES
            );
            self.online = false;
        } else {
            log::debug!("[{}] device still OFFLINE", self.name);
        }

        Err(format!("device offline ({} attempts exhausted)", config::MAX_RETRIES))
    }
}

/// Flush input, send `READ\n`, read bytes until `\n` or timeout.
fn poll_json(port: &mut Box<dyn SerialPort>, timeout_ms: u64) -> Result<String, String> {
    // Flush any stale bytes in the buffer
    port.clear(serialport::ClearBuffer::Input)
        .map_err(|e| format!("Buffer clear error: {e}"))?;

    port.write_all(b"READ\n")
        .map_err(|e| format!("Write error: {e}"))?;

    read_line(port, timeout_ms)
}

/// Read bytes one-by-one until `\n` is encountered or timeout expires.
fn read_line(port: &mut Box<dyn SerialPort>, timeout_ms: u64) -> Result<String, String> {
    let mut buf  = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        if Instant::now() > deadline {
            return Err(format!("timeout — got {} bytes without newline", buf.len()));
        }
        match port.read(&mut byte) {
            Ok(1) => {
                if byte[0] == b'\n' {
                    break;
                }
                // Ignore bare carriage returns (\r\n line endings)
                if byte[0] != b'\r' {
                    buf.push(byte[0]);
                }
            }
            Ok(_)  => {}  // 0 bytes, keep waiting
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(format!("Read error: {e}")),
        }
    }

    String::from_utf8(buf).map_err(|e| format!("UTF-8 decode error: {e}"))
}
