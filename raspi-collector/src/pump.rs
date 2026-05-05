use std::collections::HashMap;

/// One pump's reading from the JSON payload.
/// JSON keys: err, v, i, f, pf, p, e
#[derive(Debug, Clone, Default)]
pub struct PumpEntry {
    pub err:          u32,
    pub volts:        f64,  // v  — voltage (V)
    pub current:      f64,  // i  — current (A)
    pub frequency:    f64,  // f  — frequency (Hz)
    pub power_factor: f64,  // pf — power factor (0–1)
    pub power:        f64,  // p  — active power (W)
    pub energy:       f64,  // e  — energy (kWh)
}

/// Full payload returned by the USB JSON node.
#[derive(Debug, Clone)]
pub struct PumpData {
    pub source:    String,
    pub timestamp: String,
    /// Keyed by whatever the node uses ("pump1", "pump2", …)
    pub pumps:     HashMap<String, PumpEntry>,
}

impl PumpData {
    /// Parse from a raw JSON string.  Expected shape:
    /// `{"ts":"…","pump1":{"err":0,"v":220.5,"i":5.2,"f":50.0,"pf":0.95,"p":150.3,"e":0.5},…}`
    pub fn from_json(json_str: &str, source: String) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| format!("JSON parse: {e}"))?;

        let obj = value
            .as_object()
            .ok_or("JSON root is not an object")?;

        let timestamp = obj
            .get("ts")
            .and_then(|v| v.as_u64())
            .map(|ms| {
                let s = ms / 1000;
                format!("up {}h {:02}m {:02}s", s / 3600, (s % 3600) / 60, s % 60)
            })
            .or_else(|| obj.get("ts").and_then(|v| v.as_str()).map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string());

        let mut pumps = HashMap::new();
        for (key, val) in obj {
            if key == "ts" {
                continue;
            }
            let err          = val.get("err").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let volts        = val.get("v") .and_then(|v| v.as_f64()).unwrap_or(0.0);
            let current      = val.get("i") .and_then(|v| v.as_f64()).unwrap_or(0.0);
            let frequency    = val.get("f") .and_then(|v| v.as_f64()).unwrap_or(0.0);
            let power_factor = val.get("pf").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let power        = val.get("p") .and_then(|v| v.as_f64()).unwrap_or(0.0);
            let energy       = val.get("e") .and_then(|v| v.as_f64()).unwrap_or(0.0);
            pumps.insert(key.clone(), PumpEntry {
                err, volts, current, frequency, power_factor, power, energy,
            });
        }

        Ok(PumpData { source, timestamp, pumps })
    }
}
