mod config;
mod modbus;
mod sensor;
mod reader;
mod pump;
mod uart_usb;

use reader::{ModbusReader, SensorReader};
use sensor::SensorData;
use uart_usb::UartJsonReader;
use pump::PumpData;
use std::thread;
use std::time::Duration;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp_secs()
    .init();

    log::info!("raspi-collector v{} starting", env!("CARGO_PKG_VERSION"));
    log::info!(
        "RS485 -> {} @ {} baud  slave {}",
        config::RS485_PORT, config::RS485_BAUD, config::RS485_SLAVE
    );
    log::info!(
        "USB   -> {} @ {} baud  (JSON protocol)",
        config::USB_PORT, config::USB_BAUD
    );
    log::info!(
        "poll interval {}s | retries {} | timeout {}ms",
        config::READ_INTERVAL_SECS, config::MAX_RETRIES, config::TIMEOUT_MS
    );

    let mut rs485 = ModbusReader::new(
        "RS485", config::RS485_PORT, config::RS485_BAUD, config::RS485_SLAVE,
    );
    let mut usb = UartJsonReader::new("USB", config::USB_PORT, config::USB_BAUD);

    loop {
        // ── RS485 Modbus node (water quality sensors) ──────────────────────────
        match rs485.read() {
            Ok(data) => log_sensor_data(&data),
            Err(_)   => {}  // state transitions logged inside ModbusReader
        }

        // ── USB JSON node (pump monitor) ───────────────────────────────────────
        match usb.read() {
            Ok(data) => log_pump_data(&data),
            Err(_)   => {}  // state transitions logged inside UartJsonReader
        }

        thread::sleep(Duration::from_secs(config::READ_INTERVAL_SECS));
    }
}

fn log_sensor_data(d: &SensorData) {
    let avg = d.avg_temperature()
        .map(|t| format!("{t:.1} °C"))
        .unwrap_or_else(|| "n/a (no sensor OK)".to_string());

    log::info!("[{}] ─────────────────────────────────────────", d.source);
    log::info!("[{}]   Avg Temperature : {}", d.source, avg);
    log::info!("[{}]   [DO]  {}", d.source, ok_str(d.do_ok));
    log::info!("[{}]         Saturation    {:>7.1} %",    d.source, d.do_saturation);
    log::info!("[{}]         Concentration {:>7.2} mg/L", d.source, d.do_concentration);
    log::info!("[{}]         Temperature   {:>7.1} °C",   d.source, d.do_temperature);
    log::info!("[{}]   [EC]  {}", d.source, ok_str(d.ec_ok));
    log::info!("[{}]         Temperature   {:>7.2} °C",   d.source, d.ec_temperature);
    log::info!("[{}]         EC            {:>7} µS/cm",  d.source, d.ec);
    log::info!("[{}]         Salinity      {:>7} ppm",    d.source, d.salinity);
    log::info!("[{}]         TDS           {:>7} ppm",    d.source, d.tds);
    log::info!("[{}]   [pH]  {}", d.source, ok_str(d.ph_ok));
    log::info!("[{}]         Temperature   {:>7.1} °C",   d.source, d.ph_temperature);
    log::info!("[{}]         pH            {:>7.1}",       d.source, d.ph);
}

fn log_pump_data(d: &PumpData) {
    log::info!("[{}] ─────────────────────────────────────────", d.source);
    log::info!("[{}]   Timestamp : {}", d.source, d.timestamp);

    // Sort keys so output order is deterministic
    let mut keys: Vec<&String> = d.pumps.keys().collect();
    keys.sort();

    for key in keys {
        let p = &d.pumps[key];
        if p.err > 0 {
            log::warn!("[{}]   {} : OFFLINE (err={})", d.source, key, p.err);
        } else {
            log::info!(
                "[{}]   {} : ONLINE  | {:>7.2} V | {:>7.2} W",
                d.source, key, p.volts, p.watts
            );
        }
    }
}

fn ok_str(ok: bool) -> &'static str {
    if ok { "OK" } else { "STALE" }
}
