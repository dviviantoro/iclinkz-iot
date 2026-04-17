use serialport::SerialPort;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

// ── CRC-16/Modbus ─────────────────────────────────────────────────────────────
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

// ── Build Modbus RTU request frame (8 bytes) ───────────────────────────────────
pub fn build_request(slave: u8, func_code: u8, start_reg: u16, num_regs: u16) -> [u8; 8] {
    let mut frame = [0u8; 8];
    frame[0] = slave;
    frame[1] = func_code;
    frame[2] = (start_reg >> 8) as u8;
    frame[3] =  start_reg       as u8;
    frame[4] = (num_regs  >> 8) as u8;
    frame[5] =  num_regs        as u8;
    let crc   = crc16(&frame[..6]);
    frame[6]  = (crc & 0xFF) as u8;  // CRC low byte
    frame[7]  = (crc >> 8)   as u8;  // CRC high byte
    frame
}

// ── Accumulate bytes until expected_len or timeout ────────────────────────────
pub fn read_response(
    port: &mut Box<dyn SerialPort>,
    expected_len: usize,
    timeout_ms: u64,
) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; expected_len];
    let mut received = 0usize;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    while received < expected_len {
        if Instant::now() > deadline {
            return Err(format!("timeout — got {}/{} bytes", received, expected_len));
        }
        match port.read(&mut buf[received..]) {
            Ok(0)  => {}   // spurious wakeup, keep waiting
            Ok(n)  => received += n,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(format!("Read error: {e}")),
        }
    }
    Ok(buf)
}

// ── Verify CRC of a complete response frame ───────────────────────────────────
pub fn verify_crc(data: &[u8]) -> bool {
    let len = data.len();
    if len < 2 { return false; }
    let calc = crc16(&data[..len - 2]);
    let recv = (data[len - 2] as u16) | ((data[len - 1] as u16) << 8);
    calc == recv
}

// ── Extract u16 register values from a Modbus response ───────────────────────
pub fn parse_registers(data: &[u8], num_regs: usize) -> Vec<u16> {
    (0..num_regs)
        .map(|i| {
            let off = 3 + i * 2;
            ((data[off] as u16) << 8) | data[off + 1] as u16
        })
        .collect()
}

// ── High-level: send FC03 request, receive and validate response ───────────────
pub fn poll_registers(
    port: &mut Box<dyn SerialPort>,
    slave: u8,
    start_reg: u16,
    num_regs: usize,
    timeout_ms: u64,
) -> Result<Vec<u16>, String> {
    let request    = build_request(slave, 0x03, start_reg, num_regs as u16);
    let expect_len = 3 + num_regs * 2 + 2;  // addr + fn + bytecount + data + crc

    port.clear(serialport::ClearBuffer::Input)
        .map_err(|e| format!("Buffer clear error: {e}"))?;

    port.write_all(&request)
        .map_err(|e| format!("Write error: {e}"))?;

    let response = read_response(port, expect_len, timeout_ms)?;

    if !verify_crc(&response) {
        return Err("CRC mismatch".to_string());
    }

    Ok(parse_registers(&response, num_regs))
}
