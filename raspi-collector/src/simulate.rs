//! Full-stack sensor simulator — publishes randomised readings for ALL sensor
//! types on every tick, matching the exact MQTT topics and payload shapes that
//! raspi-collector uses.  No hardware, no Groq API required.
//!
//! Topics published each interval:
//!   {DEVICE_ID}/sensors/chamber_effluent  — ph, water_temp, do, tds, ec
//!   {DEVICE_ID}/sensors/blower_pump       — v1/i1/f1/pf1/p1/e1, v2/…
//!   {DEVICE_ID}/sensors/ambient           — amb_temp, amb_hum
//!   {DEVICE_ID}/meters/flowmeter          — brand, reading_m3, dn_mm, qn_m3h, …
//!
//! Run: cargo run --bin simulate

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ── Simulation constants ──────────────────────────────────────────────────────

const FLOW_INCREMENT_M3: u64 = 2;

// ── Tiny seeded LCG — no external crate needed ───────────────────────────────

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self { Rng(seed ^ 0xdeadbeef_cafebabe) }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    /// Uniform float in [0.0, 1.0)
    fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform float in [lo, hi]
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.f64() * (hi - lo)
    }

    /// Uniform float rounded to `dp` decimal places
    fn round(&mut self, lo: f64, hi: f64, dp: i32) -> f64 {
        let v = self.range(lo, hi);
        let f = 10f64.powi(dp);
        (v * f).round() / f
    }
}

// ── Persistent simulation state ───────────────────────────────────────────────

struct State {
    flow_m3:     u64,   // cumulative meter reading
    energy1_kwh: f64,   // channel 1 accumulated energy
    energy2_kwh: f64,   // channel 2 accumulated energy
}

// ── Meter reading schema (matches meter-data/*.json) ─────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
struct MeterReading {
    brand:        String,
    reading_m3:   String,
    dn_mm:        u32,
    qn_m3h:       u32,
    pn_bar:       u32,
    max_temp_c:   u32,
    iso:          u32,
    timestamp:    String,
    source_image: String,
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let env_path = std::env::args()
        .skip_while(|a| a != "--env-file")
        .nth(1)
        .unwrap_or_else(|| ".env".into());
    match dotenvy::from_path(&env_path) {
        Ok(()) => eprintln!("Loaded env: {env_path}"),
        Err(e) => eprintln!("Warning: could not load {env_path}: {e} — using environment"),
    }

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp_secs()
    .init();

    // ── Config ────────────────────────────────────────────────────────────────
    let device_id     = std::env::var("DEVICE_ID").unwrap_or_else(|_| "rpi-001".into());
    let host          = std::env::var("MQTT_HOST").unwrap_or_else(|_| "localhost".into());
    let port          = std::env::var("MQTT_PORT").unwrap_or_else(|_| "1883".into())
                            .parse::<u16>().unwrap_or(1883);
    let username      = std::env::var("MQTT_USERNAME").unwrap_or_default();
    let password      = std::env::var("MQTT_PASSWORD").unwrap_or_default();
    let keep_alive    = std::env::var("MQTT_KEEP_ALIVE").unwrap_or_else(|_| "60".into())
                            .parse::<u64>().unwrap_or(60);
    let qos = match std::env::var("MQTT_QOS").unwrap_or_else(|_| "1".into()).as_str() {
        "0" => QoS::AtMostOnce,
        "2" => QoS::ExactlyOnce,
        _   => QoS::AtLeastOnce,
    };
    let interval_mins  = std::env::var("SIMULATE_INTERVAL_MINS")
                            .unwrap_or_else(|_| "30".into())
                            .parse::<u64>().unwrap_or(30);
    // SIMULATE_INTERVAL_SECS overrides mins when set (useful for dev/testing)
    let interval_secs: u64 = std::env::var("SIMULATE_INTERVAL_SECS")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(interval_mins * 60);
    let meter_data_dir = std::env::var("METER_DATA_DIR")
                            .unwrap_or_else(|_| "meter-data".into());

    log::info!("simulate | device={device_id} | interval={interval_secs}s");
    log::info!("topics  → {device_id}/sensors/chamber_effluent");
    log::info!("          {device_id}/sensors/blower_pump");
    log::info!("          {device_id}/sensors/ambient");
    log::info!("          {device_id}/meters/flowmeter");
    log::info!("MQTT    → {host}:{port}");

    // ── MQTT connect ──────────────────────────────────────────────────────────
    let mut opts = MqttOptions::new(format!("{device_id}-sim"), &host, port);
    opts.set_keep_alive(Duration::from_secs(keep_alive));
    if !username.is_empty() {
        opts.set_credentials(&username, &password);
    }
    let (client, mut connection) = Client::new(opts, 64);

    thread::spawn(move || {
        for event in connection.iter() {
            match event {
                Ok(Event::Incoming(Packet::ConnAck(_))) => log::info!("[MQTT] connected"),
                Err(e) => {
                    log::warn!("[MQTT] error: {e} — retrying…");
                    thread::sleep(Duration::from_secs(5));
                }
                _ => {}
            }
        }
    });

    // ── Load flow meter base from sample.json ─────────────────────────────────
    let sample_path = PathBuf::from(&meter_data_dir).join("sample.json");
    let base_meter  = load_latest_reading(&meter_data_dir, &sample_path);
    let (meter_brand, meter_dn, meter_qn, meter_pn, meter_max_temp, meter_iso, flow_start) =
        base_meter.map(|m| {
            let v = m.reading_m3.parse::<u64>().unwrap_or(0);
            (m.brand, m.dn_mm, m.qn_m3h, m.pn_bar, m.max_temp_c, m.iso, v)
        })
        .unwrap_or_else(|| ("IPM".into(), 100, 60, 16, 50, 4064, 331));

    let mut state = State {
        flow_m3:     flow_start,
        energy1_kwh: 0.0,
        energy2_kwh: 0.0,
    };

    // Seed RNG from current time so each run produces different values
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    let mut rng = Rng::new(seed);

    // ── Main simulation loop ──────────────────────────────────────────────────
    loop {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let ts = unix_to_iso8601(now_secs);

        publish_all(
            &client, qos, &device_id, &ts,
            &mut state, &mut rng,
            &meter_brand, meter_dn, meter_qn, meter_pn, meter_max_temp, meter_iso,
            &meter_data_dir,
        );

        log::info!("[simulate] sleeping {interval_secs}s…");
        thread::sleep(Duration::from_secs(interval_secs));
    }
}

// ── Publish all sensor types for one tick ─────────────────────────────────────

fn publish_all(
    client:     &rumqttc::Client,
    qos:        QoS,
    device_id:  &str,
    ts:         &str,
    state:      &mut State,
    rng:        &mut Rng,
    brand:      &str,
    dn_mm:      u32,
    qn_m3h:     u32,
    pn_bar:     u32,
    max_temp_c: u32,
    iso:        u32,
    data_dir:   &str,
) {
    // ── 1. chamber_effluent — water quality ───────────────────────────────────
    let ph         = rng.round(6.8, 7.6, 2);
    let water_temp = rng.round(24.5, 27.5, 1);
    let do_val     = rng.round(7.2, 8.8, 2);
    let tds        = rng.round(560.0, 660.0, 0) as u32;
    let ec         = rng.round(1120.0, 1320.0, 0) as u32;

    publish(client, qos, &format!("{device_id}/sensors/chamber_effluent"),
        serde_json::json!({
            "ph": ph, "water_temp": water_temp,
            "do": do_val, "tds": tds, "ec": ec,
        }));

    // ── 2. blower_pump — electrical ───────────────────────────────────────────
    let v1  = rng.round(217.5, 222.5, 1);
    let i1  = rng.round(4.8, 5.6, 2);
    let f1  = rng.round(49.8, 50.2, 1);
    let pf1 = rng.round(0.92, 0.96, 2);
    let p1  = (v1 * i1 * pf1 * 10.0).round() / 10.0;
    let v2  = rng.round(217.5, 222.5, 1);
    let i2  = rng.round(4.2, 5.0, 2);
    let f2  = rng.round(49.8, 50.2, 1);
    let pf2 = rng.round(0.91, 0.95, 2);
    let p2  = (v2 * i2 * pf2 * 10.0).round() / 10.0;

    // Energy accumulates continuously
    let kwh_per_tick = p1 / 1000.0 * (30.0 / 60.0); // kWh per 30-min tick
    state.energy1_kwh += kwh_per_tick;
    state.energy2_kwh += (p2 / 1000.0) * (30.0 / 60.0);
    let e1 = (state.energy1_kwh * 1000.0).round() / 1000.0;
    let e2 = (state.energy2_kwh * 1000.0).round() / 1000.0;

    publish(client, qos, &format!("{device_id}/sensors/blower_pump"),
        serde_json::json!({
            "v1": v1, "i1": i1, "f1": f1, "pf1": pf1, "p1": p1, "e1": e1,
            "v2": v2, "i2": i2, "f2": f2, "pf2": pf2, "p2": p2, "e2": e2,
        }));

    // ── 3. ambient — temperature + humidity ───────────────────────────────────
    let amb_temp = rng.round(26.5, 29.5, 2);
    let amb_hum  = rng.round(62.0, 70.0, 2);

    publish(client, qos, &format!("{device_id}/sensors/ambient"),
        serde_json::json!({ "amb_temp": amb_temp, "amb_hum": amb_hum }));

    // ── 4. flowmeter — cumulative volume ──────────────────────────────────────
    state.flow_m3 += FLOW_INCREMENT_M3;
    let reading_str = format!("{:06}", state.flow_m3);

    // Write JSON to meter-data/
    let meter = serde_json::json!({
        "brand":        brand,
        "reading_m3":   reading_str,
        "dn_mm":        dn_mm,
        "qn_m3h":       qn_m3h,
        "pn_bar":       pn_bar,
        "max_temp_c":   max_temp_c,
        "iso":          iso,
        "timestamp":    ts,
        "source_image": "simulate.rs",
    });
    let ts_tag  = ts.replace(['-', ':', 'Z'], "").replace('T', "_");
    let out     = PathBuf::from(data_dir).join(format!("{ts_tag}.json"));
    match serde_json::to_string_pretty(&meter) {
        Ok(s) => {
            if let Err(e) = fs::write(&out, &s) {
                log::warn!("[simulate] write {}: {e}", out.display());
            } else {
                log::info!("[simulate] wrote {}", out.display());
            }
        }
        Err(e) => log::error!("[simulate] serialize: {e}"),
    }

    publish(client, qos, &format!("{device_id}/meters/flowmeter"),
        serde_json::json!({
            "brand":      brand,
            "reading_m3": reading_str,
            "dn_mm":      dn_mm,
            "qn_m3h":     qn_m3h,
            "pn_bar":     pn_bar,
            "max_temp_c": max_temp_c,
            "iso":        iso,
            "timestamp":  ts,
        }));

    log::info!(
        "[simulate] tick | ph={ph} do={do_val} tds={tds} ec={ec} | \
         v1={v1}V i1={i1}A | amb={amb_temp}°C {amb_hum}% | flow={reading_str}m³"
    );
}

// ── MQTT publish helper ───────────────────────────────────────────────────────

fn publish(client: &rumqttc::Client, qos: QoS, topic: &str, payload: serde_json::Value) {
    let body = payload.to_string();
    if let Err(e) = client.publish(topic, qos, false, body.as_bytes()) {
        log::warn!("[MQTT>] {topic}: {e}");
    } else {
        log::debug!("[MQTT>] {topic} {body}");
    }
}

// ── Meter-data file helpers ───────────────────────────────────────────────────

fn load_latest_reading(dir: &str, sample_path: &std::path::Path) -> Option<MeterReading> {
    let path = find_latest_json(dir).unwrap_or_else(|| {
        log::info!("[simulate] no prior data — seeding from {}", sample_path.display());
        sample_path.to_path_buf()
    });
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str::<MeterReading>(&content).ok()
}

fn find_latest_json(dir: &str) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir).ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|x| x.to_str()) == Some("json")
                && p.file_name().and_then(|x| x.to_str()) != Some("sample.json")
        })
        .collect();
    files.sort();
    files.into_iter().last()
}

// ── Timestamp helpers (no chrono dependency) ──────────────────────────────────

fn unix_to_iso8601(secs: u64) -> String {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let (yr, mo, dy) = days_to_ymd(secs / 86400);
    format!("{yr:04}-{mo:02}-{dy:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z   = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y   = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp  = (5 * doy + 2) / 153;
    let d   = doy - (153 * mp + 2) / 5 + 1;
    let mo  = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr  = if mo <= 2 { y + 1 } else { y };
    (yr, mo, d)
}
