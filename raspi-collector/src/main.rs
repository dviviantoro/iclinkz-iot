mod cli;
mod config;
mod modbus;
mod mqtt;
mod pump;
mod reader;
mod sensor;
mod uart_usb;
#[cfg(target_os = "linux")]
mod aht10;

use clap::Parser;
use cli::{Cli, Mode};
use mqtt::{MqttEnv, MqttPublisher};
use pump::PumpData;
use reader::{ModbusReader, SensorReader};
use sensor::SensorData;
use uart_usb::UartJsonReader;
#[cfg(target_os = "linux")]
use aht10::{Aht10Reader, AmbientData};
use std::thread;
use std::time::Duration;

fn main() {
    // ── CLI args (parsed before logger so --env-file is available) ────────────
    let cli = Cli::parse();

    // ── Load .env from the path given by --env-file (default: ".env") ─────────
    match dotenvy::from_path(&cli.env_file) {
        Ok(())  => eprintln!("Loaded env: {}", cli.env_file.display()),
        Err(e)  => eprintln!("Warning: could not load {}: {e} — using environment variables", cli.env_file.display()),
    }

    // ── Logger (respects RUST_LOG; default = info) ─────────────────────────────
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp_secs()
    .init();
    log::info!("raspi-collector v{} | mode: {:?}", env!("CARGO_PKG_VERSION"), cli.mode);
    log::info!("RS485  -> {} @ {} baud  slave {}", config::RS485_PORT, config::RS485_BAUD, config::RS485_SLAVE);
    log::info!("USB    -> {} @ {} baud  (JSON)", config::USB_PORT, config::USB_BAUD);
    log::info!("AHT10  -> {}  addr 0x{:02X}", config::I2C_BUS, config::AHT10_ADDR);
    log::info!("poll interval {}s | retries {} | timeout {}ms",
        config::READ_INTERVAL_SECS, config::MAX_RETRIES, config::TIMEOUT_MS);

    // ── MQTT publisher (None in read-only mode) ────────────────────────────────
    let mqtt: Option<MqttPublisher> = match cli.mode {
        Mode::ReadOnly => {
            log::info!("Read-only mode — MQTT disabled");
            None
        }
        Mode::Mqtt | Mode::Subscriber => {
            let subscribe = matches!(cli.mode, Mode::Subscriber);
            match MqttEnv::from_env() {
                Ok(env) => match MqttPublisher::new(env, subscribe) {
                    Ok(p)  => Some(p),
                    Err(e) => { log::error!("MQTT init failed: {e}"); None }
                },
                Err(e) => { log::error!("MQTT config error: {e}"); None }
            }
        }
    };

    // ── Sensor readers ─────────────────────────────────────────────────────────
    let mut rs485 = ModbusReader::new(
        "RS485", config::RS485_PORT, config::RS485_BAUD, config::RS485_SLAVE,
    );
    let mut usb = UartJsonReader::new("USB", config::USB_PORT, config::USB_BAUD);
    #[cfg(target_os = "linux")]
    let mut aht10 = Aht10Reader::new(config::I2C_BUS, config::AHT10_ADDR);

    // ── Main poll loop ─────────────────────────────────────────────────────────
    loop {
        // AHT10: ambient temperature & humidity (Linux / I2C)
        #[cfg(target_os = "linux")]
        {
            if let Ok(data) = aht10.read() {
                log_ambient(&data);
                if let Some(ref m) = mqtt {
                    m.publish_ambient(&data);
                }
            }
        }

        // RS485: water quality (Modbus RTU)
        if let Ok(data) = rs485.read() {
            log_sensor(&data);
            if let Some(ref m) = mqtt {
                m.publish_chamber_effluent(&data);
            }
        }

        // USB: pump monitor (JSON over serial)
        if let Ok(data) = usb.read() {
            log_pump(&data);
            if let Some(ref m) = mqtt {
                m.publish_blower_pump(&data);
            }
        }

        thread::sleep(Duration::from_secs(config::READ_INTERVAL_SECS));
    }
}

// ── Logging helpers ────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn log_ambient(d: &AmbientData) {
    log::info!("[AHT10] ──────────────────────────────────────");
    log::info!("[AHT10]   Temperature : {:>7.2} °C", d.temperature);
    log::info!("[AHT10]   Humidity    : {:>7.2} %",  d.humidity);
}

fn log_sensor(d: &SensorData) {
    let avg = d.avg_temperature()
        .map(|t| format!("{t:.1} °C"))
        .unwrap_or_else(|| "n/a".to_string());

    log::info!("[{}] ──────────────────────────────────────", d.source);
    log::info!("[{}]   Avg Water Temp : {}", d.source, avg);
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

fn log_pump(d: &PumpData) {
    log::info!("[{}] ──────────────────────────────────────", d.source);
    log::info!("[{}]   Timestamp : {}", d.source, d.timestamp);

    let mut keys: Vec<&String> = d.pumps.keys().collect();
    keys.sort();
    for (idx, key) in keys.iter().enumerate() {
        let p = &d.pumps[*key];
        if p.err > 0 {
            log::warn!("[{}]   {} (pump{}) : OFFLINE (err={})", d.source, key, idx + 1, p.err);
        } else {
            log::info!(
                "[{}]   {} : ONLINE | {:>7.2}V {:>6.2}A {:>5.1}Hz PF={:.2} {:>7.2}W {:>8.3}kWh",
                d.source, key, p.volts, p.current, p.frequency, p.power_factor, p.power, p.energy
            );
        }
    }
}

fn ok_str(ok: bool) -> &'static str {
    if ok { "OK" } else { "STALE" }
}
