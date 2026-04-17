use std::collections::HashMap;

/// One pump's reading from the JSON payload.
#[derive(Debug, Clone, Default)]
pub struct PumpEntry {
    pub err:   u32,
    pub volts: f64,
    pub watts: f64,
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
    /// `{"ts":"…", "pump1":{"err":0,"v":220.5,"w":150.3}, …}`
    pub fn from_json(json_str: &str, source: String) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| format!("JSON parse: {e}"))?;

        let obj = value
            .as_object()
            .ok_or("JSON root is not an object")?;

        let timestamp = obj
            .get("ts")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let mut pumps = HashMap::new();
        for (key, val) in obj {
            if key == "ts" {
                continue;
            }
            let err   = val.get("err").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let volts = val.get("v")  .and_then(|v| v.as_f64()).unwrap_or(0.0);
            let watts = val.get("w")  .and_then(|v| v.as_f64()).unwrap_or(0.0);
            pumps.insert(key.clone(), PumpEntry { err, volts, watts });
        }

        Ok(PumpData { source, timestamp, pumps })
    }
}
