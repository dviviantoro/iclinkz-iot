use crate::{pump::PumpData, sensor::SensorData};
#[cfg(target_os = "linux")]
use crate::aht10::AmbientData;

use rumqttc::{Client, Event, MqttOptions, Packet, QoS};
use std::thread;
use std::time::Duration;

// ── Environment-based configuration ───────────────────────────────────────────

pub struct MqttEnv {
    pub device_id:  String,
    pub host:       String,
    pub port:       u16,
    pub username:   String,
    pub password:   String,
    pub qos:        QoS,
    pub keep_alive: u64,
}

impl MqttEnv {
    /// Load MQTT settings from environment variables (populated from .env by dotenvy).
    pub fn from_env() -> Result<Self, String> {
        let device_id = std::env::var("DEVICE_ID")
            .map_err(|_| "DEVICE_ID is not set — check your .env file")?;
        let host = std::env::var("MQTT_HOST")
            .map_err(|_| "MQTT_HOST is not set — check your .env file")?;
        let port = std::env::var("MQTT_PORT")
            .unwrap_or_else(|_| "1883".into())
            .parse::<u16>()
            .map_err(|_| "MQTT_PORT must be a valid port number (0–65535)")?;
        let username = std::env::var("MQTT_USERNAME").unwrap_or_default();
        let password = std::env::var("MQTT_PASSWORD").unwrap_or_default();
        let keep_alive = std::env::var("MQTT_KEEP_ALIVE")
            .unwrap_or_else(|_| "60".into())
            .parse::<u64>()
            .map_err(|_| "MQTT_KEEP_ALIVE must be a number (seconds)")?;
        let qos = match std::env::var("MQTT_QOS")
            .unwrap_or_else(|_| "1".into())
            .as_str()
        {
            "0" => QoS::AtMostOnce,
            "2" => QoS::ExactlyOnce,
            _   => QoS::AtLeastOnce,
        };

        Ok(MqttEnv { device_id, host, port, username, password, qos, keep_alive })
    }
}

// ── Publisher ──────────────────────────────────────────────────────────────────

pub struct MqttPublisher {
    client:    Client,
    device_id: String,
    qos:       QoS,
}

impl MqttPublisher {
    /// Connect to the MQTT broker and optionally subscribe to the control topic.
    ///
    /// Spawns a background thread that drives the MQTT event loop and logs any
    /// incoming messages (commands) when `subscribe = true`.
    pub fn new(env: MqttEnv, subscribe: bool) -> Result<Self, String> {
        let mut opts = MqttOptions::new(&env.device_id, &env.host, env.port);
        opts.set_keep_alive(Duration::from_secs(env.keep_alive));
        if !env.username.is_empty() {
            opts.set_credentials(&env.username, &env.password);
        }

        let (client, mut connection) = Client::new(opts, 64);

        if subscribe {
            let ctrl_topic = format!("{}/control/#", env.device_id);
            client
                .subscribe(&ctrl_topic, env.qos)
                .map_err(|e| format!("MQTT subscribe failed: {e}"))?;
            log::info!("[MQTT] subscribed to {ctrl_topic}");
        }

        let device_id_clone = env.device_id.clone();
        thread::spawn(move || {
            log::info!("[MQTT] event loop started ({}:{})", env.host, env.port);
            for event in connection.iter() {
                match event {
                    Ok(Event::Incoming(Packet::Publish(msg))) => {
                        log::info!(
                            "[MQTT<] topic={} payload={}",
                            msg.topic,
                            String::from_utf8_lossy(&msg.payload)
                        );
                    }
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        log::info!("[MQTT] connected as '{device_id_clone}'");
                    }
                    Err(e) => {
                        log::warn!("[MQTT] connection error: {e} — retrying…");
                        thread::sleep(Duration::from_secs(5));
                    }
                    _ => {}
                }
            }
        });

        Ok(MqttPublisher {
            client,
            device_id: env.device_id,
            qos:       env.qos,
        })
    }

    fn publish(&self, subtopic: &str, payload: serde_json::Value) {
        let topic   = format!("{}/sensors/{}", self.device_id, subtopic);
        let body    = payload.to_string();
        if let Err(e) = self.client.publish(&topic, self.qos, false, body.as_bytes()) {
            log::warn!("[MQTT>] failed to publish to {topic}: {e}");
        } else {
            log::debug!("[MQTT>] {topic} {body}");
        }
    }

    // ── chamber_effluent — water quality from RS485 Modbus node ───────────────

    pub fn publish_chamber_effluent(&self, d: &SensorData) {
        let water_temp = d.avg_temperature().unwrap_or(0.0);
        self.publish("chamber_effluent", serde_json::json!({
            "ph":         d.ph,
            "water_temp": (water_temp * 100.0).round() / 100.0,
            "do":         d.do_concentration,
            "tds":        d.tds,
            "ec":         d.ec,
        }));
    }

    // ── blower_pump — pump monitor from USB JSON node ──────────────────────────
    // Pumps are sorted alphabetically and numbered 1, 2, … in the payload.

    pub fn publish_blower_pump(&self, d: &PumpData) {
        let mut map = serde_json::Map::new();
        let mut keys: Vec<&String> = d.pumps.keys().collect();
        keys.sort();

        for (idx, key) in keys.iter().enumerate() {
            let n = idx + 1;
            let p = &d.pumps[*key];
            map.insert(format!("v{n}"),  serde_json::json!(p.volts));
            map.insert(format!("i{n}"),  serde_json::json!(p.current));
            map.insert(format!("f{n}"),  serde_json::json!(p.frequency));
            map.insert(format!("pf{n}"), serde_json::json!(p.power_factor));
            map.insert(format!("p{n}"),  serde_json::json!(p.power));
            map.insert(format!("e{n}"),  serde_json::json!(p.energy));
        }

        self.publish("blower_pump", serde_json::Value::Object(map));
    }

    // ── ambient — AHT10 temperature & humidity (Linux / I2C only) ─────────────

    #[cfg(target_os = "linux")]
    pub fn publish_ambient(&self, d: &AmbientData) {
        self.publish("ambient", serde_json::json!({
            "amb_temp": (d.temperature * 100.0).round() / 100.0,
            "amb_hum":  (d.humidity    * 100.0).round() / 100.0,
        }));
    }
}
