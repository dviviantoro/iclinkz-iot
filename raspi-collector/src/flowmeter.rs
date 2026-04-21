//! Flow meter reading via webcam + Groq vision API.
//!
//! Pipeline per poll cycle:
//!   1. Capture a JPEG frame with `libcamera-still` (or `fswebcam` for USB cams)
//!   2. Base64-encode the image
//!   3. POST to the Groq chat-completions endpoint with a vision model
//!   4. Parse the numeric reading out of the response text
//!   5. Return `FlowReading` for the caller to publish over MQTT

use std::process::Command;
use std::fs;
use std::path::PathBuf;

// ── Data ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FlowReading {
    /// Raw padded string as it appears on the meter face, e.g. "000331"
    pub reading_str: String,
    /// Parsed numeric value in m³
    pub reading_m3:  f64,
    /// Brand / model tag from config (for the MQTT payload)
    pub brand:       String,
}

// ── Reader ────────────────────────────────────────────────────────────────────

pub struct FlowmeterReader {
    /// API key loaded from GROQ_API_KEY env var
    api_key:    String,
    /// Groq model to use, e.g. "meta-llama/llama-4-scout-17b-16e-instruct"
    model:      String,
    /// Camera backend: "libcamera" (Pi cam) or "fswebcam" (USB cam)
    pub backend: CamBackend,
    /// Device node for fswebcam, e.g. "/dev/video0"
    device:     String,
    /// Meter brand tag forwarded as-is in the MQTT payload
    pub brand:  String,
    /// Temp file path for captured frames
    tmp_path:   PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub enum CamBackend {
    Libcamera,
    Fswebcam,
}

impl FlowmeterReader {
    /// Construct from environment variables.
    ///
    /// Required: GROQ_API_KEY
    /// Optional: FLOWMETER_MODEL, FLOWMETER_BACKEND, FLOWMETER_DEVICE,
    ///           FLOWMETER_BRAND, FLOWMETER_TMP
    pub fn from_env() -> Result<Self, String> {
        let api_key = std::env::var("GROQ_API_KEY")
            .map_err(|_| "GROQ_API_KEY is not set — check your .env file")?;

        let model = std::env::var("FLOWMETER_MODEL")
            .unwrap_or_else(|_| "meta-llama/llama-4-scout-17b-16e-instruct".into());

        let backend = match std::env::var("FLOWMETER_BACKEND")
            .unwrap_or_else(|_| "fswebcam".into())
            .to_lowercase()
            .as_str()
        {
            "libcamera" => CamBackend::Libcamera,
            _           => CamBackend::Fswebcam,
        };

        let device  = std::env::var("FLOWMETER_DEVICE")
            .unwrap_or_else(|_| "/dev/video0".into());
        let brand   = std::env::var("FLOWMETER_BRAND")
            .unwrap_or_else(|_| "generic".into());
        let tmp_path = std::env::var("FLOWMETER_TMP")
            .unwrap_or_else(|_| "/tmp/flowmeter_cap.jpg".into())
            .into();

        Ok(FlowmeterReader { api_key, model, backend, device, brand, tmp_path })
    }

    // ── Public read ───────────────────────────────────────────────────────────

    pub fn read(&self) -> Result<FlowReading, String> {
        self.capture_frame()?;
        let b64 = self.encode_frame()?;
        let raw = self.ask_groq(&b64)?;
        let (reading_str, reading_m3) = parse_reading(&raw)?;
        Ok(FlowReading { reading_str, reading_m3, brand: self.brand.clone() })
    }

    // ── Step 1: capture ───────────────────────────────────────────────────────

    fn capture_frame(&self) -> Result<(), String> {
        let status = match self.backend {
            CamBackend::Libcamera => Command::new("libcamera-still")
                .args([
                    "--output", self.tmp_path.to_str().unwrap(),
                    "--width",  "1280",
                    "--height", "720",
                    "--nopreview",
                    "--timeout", "2000",
                ])
                .status(),
            CamBackend::Fswebcam => Command::new("fswebcam")
                .args([
                    "-d", &self.device,
                    "-r", "1280x720",
                    "--no-banner",
                    self.tmp_path.to_str().unwrap(),
                ])
                .status(),
        }
        .map_err(|e| format!("[flowmeter] capture command failed to start: {e}"))?;

        if !status.success() {
            return Err(format!(
                "[flowmeter] capture exited with {}",
                status.code().unwrap_or(-1)
            ));
        }
        log::debug!("[flowmeter] frame captured → {}", self.tmp_path.display());
        Ok(())
    }

    // ── Step 2: base64-encode ─────────────────────────────────────────────────

    fn encode_frame(&self) -> Result<String, String> {
        let bytes = fs::read(&self.tmp_path)
            .map_err(|e| format!("[flowmeter] cannot read capture file: {e}"))?;
        Ok(base64_encode(&bytes))
    }

    // ── Step 3: Groq vision request ───────────────────────────────────────────

    fn ask_groq(&self, b64_image: &str) -> Result<String, String> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/jpeg;base64,{b64_image}")
                        }
                    },
                    {
                        "type": "text",
                        "text": "This image shows a water flow meter display. \
                                 Read the numeric meter counter on the display. \
                                 Reply with ONLY the digits as shown on the meter (with leading zeros if present), \
                                 nothing else. Example: 000331"
                    }
                ]
            }],
            "temperature": 0,
            "max_tokens": 32
        });

        let response = ureq::post("https://api.groq.com/openai/v1/chat/completions")
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| format!("[flowmeter] Groq HTTP error: {e}"))?;

        let text = response.into_string()
            .map_err(|e| format!("[flowmeter] Groq response read error: {e}"))?;

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("[flowmeter] Groq JSON parse error: {e}"))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| format!("[flowmeter] unexpected Groq response shape: {text}"))?
            .trim()
            .to_string();

        log::debug!("[flowmeter] Groq raw response: {content:?}");
        Ok(content)
    }
}

// ── Step 4: parse the reading out of the model's text ─────────────────────────
//
// The model is told to reply with digits only, but we defensively strip
// anything that is not a digit in case it adds a unit or whitespace.

fn parse_reading(raw: &str) -> Result<(String, f64), String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Err(format!("[flowmeter] no digits found in Groq response: {raw:?}"));
    }
    let value: f64 = digits.parse()
        .map_err(|e| format!("[flowmeter] digit parse error: {e}"))?;
    Ok((digits, value))
}

// ── Minimal base64 encoder (no external dep) ──────────────────────────────────

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 0x3F)        as usize] as char } else { '=' });
    }
    out
}
