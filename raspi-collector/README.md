# raspi-collector

Rust data-collection daemon for **Raspberry Pi 4 (64-bit)** that reads three IoT nodes and publishes to an MQTT broker.
Includes a standalone **meter simulator** (`simulate` binary) for testing without hardware.

| Interface | Protocol | Node | Data |
|---|---|---|---|
| `/dev/ttyS0` (RS485) | Modbus RTU | STM32 water-quality node | DO, EC, Salinity, TDS, pH + temperatures |
| `/dev/ttyUSB0` (USB UART) | JSON over serial | Pump monitor node | Per-pump V, A, Hz, PF, W, kWh |
| `/dev/i2c-1` (I2C) | AHT10 native | Ambient sensor | Temperature, Humidity |
| `meter-data/` (JSON files) | simulate binary | IPM flow meter | Cumulative volume, nominal flow rate |

---

## Hardware setup

```
STM32 water-quality node
  └─ RS485 transceiver ─── /dev/ttyS0     (RPi hardware UART, GPIO 14 TX / 15 RX)

Pump monitor node
  └─ USB-UART adapter  ─── /dev/ttyUSB0   (e.g. CH340 / FT232)
     Protocol: send "READ\n", receive newline-terminated JSON
     {"ts":"…","pump1":{"err":0,"v":220.5,"i":5.2,"f":50.0,"pf":0.95,"p":150.3,"e":0.5},…}

AHT10 ambient sensor
  └─ I2C bus 1         ─── /dev/i2c-1     (GPIO 2 SDA / 3 SCL)
     Address 0x38
```

Enable I2C on the Pi if not already on:
```bash
sudo raspi-config  # Interface Options → I2C → Enable
```

---

## Configuration

### Hardware / timing — [`src/config.rs`](src/config.rs)

Edit before building (or cross-compiling):

| Constant | Default | Description |
|---|---|---|
| `RS485_PORT` | `/dev/ttyS0` | Hardware UART port |
| `RS485_BAUD` | `9600` | RS485 baud rate |
| `RS485_SLAVE` | `10` | Modbus slave address |
| `USB_PORT` | `/dev/ttyUSB0` | USB UART port |
| `USB_BAUD` | `9600` | USB UART baud rate |
| `I2C_BUS` | `/dev/i2c-1` | I2C bus path |
| `AHT10_ADDR` | `0x38` | AHT10 I2C address |
| `READ_INTERVAL_SECS` | `5` | Poll cycle interval |
| `TIMEOUT_MS` | `2000` | Per-request response deadline |
| `MAX_RETRIES` | `3` | Attempts before marking device OFFLINE |

### Device / MQTT — [`.env`](.env)

Copy `.env.example` to `.env` and fill in your values:

```bash
cp .env.example .env
```

| Variable | Default | Description |
|---|---|---|
| `DEVICE_ID` | `rpi-001` | MQTT client ID and topic prefix |
| `MQTT_HOST` | `localhost` | Broker hostname or IP |
| `MQTT_PORT` | `1883` | Broker port |
| `MQTT_USERNAME` | _(blank)_ | Leave empty if no auth |
| `MQTT_PASSWORD` | _(blank)_ | Leave empty if no auth |
| `MQTT_QOS` | `1` | 0 = AtMostOnce · 1 = AtLeastOnce · 2 = ExactlyOnce |
| `MQTT_KEEP_ALIVE` | `60` | Keep-alive interval (seconds) |
| `SIMULATE_INTERVAL_MINS` | `30` | How often simulate publishes a reading |
| `METER_DATA_DIR` | `meter-data` | Directory for meter JSON files |

---

## Build

### Native (on the Raspberry Pi itself)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo build --release
# Both binaries are built:
./target/release/raspi-collector   # hardware daemon
./target/release/simulate          # flow meter simulator
```

### Cross-compile from macOS / Linux → RPi 4 (aarch64, 64-bit)

**1. Add the Rust target:**
```bash
rustup target add aarch64-unknown-linux-gnu
```

**2. Install the C cross-compiler:**

macOS:
```bash
brew tap messense/macos-cross-toolchains
brew install aarch64-unknown-linux-gnu
```

Debian/Ubuntu:
```bash
sudo apt install gcc-aarch64-linux-gnu
```

**3. Build:**
```bash
cargo build --target aarch64-unknown-linux-gnu --release
```

Binaries: `target/aarch64-unknown-linux-gnu/release/raspi-collector`
         `target/aarch64-unknown-linux-gnu/release/simulate`

**4. Deploy:**
```bash
scp target/aarch64-unknown-linux-gnu/release/raspi-collector pi@<PI_IP>:~/collector/
scp target/aarch64-unknown-linux-gnu/release/simulate       pi@<PI_IP>:~/collector/
scp .env pi@<PI_IP>:~/collector/
scp -r meter-data pi@<PI_IP>:~/collector/
```

---

## Usage

### raspi-collector (hardware daemon)

```
raspi-collector [OPTIONS]

Options:
  -m, --mode <MODE>          Operating mode [default: mqtt]
                               read-only   — log to stdout, no MQTT
                               mqtt        — publish sensor data to broker
                               subscriber  — publish + subscribe to control commands
  -e, --env-file <ENV_FILE>  Path to .env file [default: .env]
  -h, --help                 Print help
  -V, --version              Print version
```

```bash
# Default — publish to MQTT
./raspi-collector

# Read-only — log sensor data, no MQTT
./raspi-collector --mode read-only

# Subscribe mode — also listen for incoming control commands
./raspi-collector --mode subscriber --env-file /etc/raspi-collector.env
```

### simulate (flow meter simulator)

Reads `meter-data/sample.json` as base data, increments the cumulative reading each interval, writes a new timestamped JSON file, and publishes to MQTT. **No hardware or Groq API required.**

```bash
# Run from the raspi-collector directory (so .env and meter-data/ are found)
cargo run --bin simulate

# Or with the compiled binary
./simulate

# Custom .env path
./simulate --env-file /path/to/.env
```

**What it does each tick (default: every 30 minutes):**

1. Loads the most recent JSON in `meter-data/` (falls back to `sample.json` if none)
2. Increments `reading_m3` by 2 m³
3. Updates the timestamp to now (UTC)
4. Writes `meter-data/YYYYMMDD_HHMMSS.json`
5. Publishes to `{DEVICE_ID}/meters/flowmeter` on MQTT

**Log output:**
```
Loaded env: .env
[simulate] using: meter-data/sample.json
[simulate] wrote meter-data/20260420_113936.json
[MQTT>] rpi-001/meters/flowmeter → {"brand":"IPM","reading_m3":"000333","dn_mm":100,"qn_m3h":60,...}
[simulate] sleeping 30 minutes…
```

### Log level

```bash
RUST_LOG=info  ./raspi-collector   # default — readings + state changes
RUST_LOG=debug ./raspi-collector   # verbose — every retry attempt
RUST_LOG=warn  ./raspi-collector   # quiet   — only warnings and errors
```

---

## MQTT topics

All topics are prefixed with `DEVICE_ID` from `.env`.

### Published by `raspi-collector`

| Topic | Payload |
|---|---|
| `{DEVICE_ID}/sensors/chamber_effluent` | `{"ph":7.2,"water_temp":25.3,"do":7.82,"tds":620,"ec":1240}` |
| `{DEVICE_ID}/sensors/blower_pump` | `{"v1":220.5,"i1":5.2,"f1":50.0,"pf1":0.95,"p1":150.3,"e1":0.5,"v2":…}` |
| `{DEVICE_ID}/sensors/ambient` | `{"amb_temp":27.43,"amb_hum":65.20}` |

### Published by `simulate`

| Topic | Payload |
|---|---|
| `{DEVICE_ID}/meters/flowmeter` | `{"brand":"IPM","reading_m3":"000333","dn_mm":100,"qn_m3h":60,"pn_bar":16,"max_temp_c":50,"iso":4064,"timestamp":"2026-04-20T11:39:36Z"}` |

### Subscribed (`--mode subscriber`)

| Topic | Purpose |
|---|---|
| `{DEVICE_ID}/control/#` | Incoming control commands (logged; extend in `src/mqtt.rs`) |

### iclinkz subscription mapping

iclinkz subscribes to all raspi-collector topics automatically:

| iclinkz subscription | Matches |
|---|---|
| `+/sensors/+` | `{device_id}/sensors/chamber_effluent`, `/blower_pump`, `/ambient` |
| `+/meters/+` | `{device_id}/meters/flowmeter` |
| `+/status` | `{device_id}/status` |

---

## Offline / recovery behaviour

Each reader tracks its own connection state independently:

- After `MAX_RETRIES` consecutive failures → logs **OFFLINE** once, polls silently
- On next successful read → logs **back ONLINE**, resumes normal output
- Applies to all three nodes (RS485, USB UART, AHT10 I2C)

---

## Running as a systemd service

### raspi-collector

```ini
# /etc/systemd/system/raspi-collector.service
[Unit]
Description=Raspberry Pi IoT Sensor Collector
After=network.target

[Service]
ExecStart=/home/pi/collector/raspi-collector --env-file /home/pi/collector/.env --mode mqtt
WorkingDirectory=/home/pi/collector
Restart=always
RestartSec=5
User=pi
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

### simulate

```ini
# /etc/systemd/system/raspi-simulate.service
[Unit]
Description=Raspberry Pi Flow Meter Simulator
After=network.target

[Service]
ExecStart=/home/pi/collector/simulate --env-file /home/pi/collector/.env
WorkingDirectory=/home/pi/collector
Restart=always
RestartSec=10
User=pi
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable raspi-collector raspi-simulate
sudo systemctl start  raspi-collector raspi-simulate
sudo journalctl -fu   raspi-collector
sudo journalctl -fu   raspi-simulate
```

---

## Project structure

```
src/
  main.rs      — CLI parse, env load, logger init, reader + MQTT wiring, poll loop
  simulate.rs  — Standalone meter simulator (reads sample.json, publishes flowmeter data)
  cli.rs       — clap argument definitions (--mode, --env-file)
  config.rs    — hardware constants (ports, baud rates, timeouts, retry limits)
  modbus.rs    — Modbus RTU protocol: CRC-16, frame builder, response reader, parser
  sensor.rs    — SensorData struct, register-to-field mapping, avg_temperature()
  reader.rs    — SensorReader trait + ModbusReader (retry, auto-reconnect, offline state)
  pump.rs      — PumpData / PumpEntry structs, JSON parsing (v, i, f, pf, p, e)
  uart_usb.rs  — UartJsonReader: send READ\n, parse JSON line, retry, offline state
  aht10.rs     — Aht10Reader: I2C init + trigger + parse, retry, offline state [Linux]
  mqtt.rs      — MqttEnv (from .env), MqttPublisher, payload builders, event-loop thread
meter-data/
  sample.json  — Base flow meter reading (IPM brand, used by simulate as seed)
  *.json       — Generated readings from simulate or OCR scripts (gitignored)
scripts/
  run.sh           — Entry point: capture image → OCR → write JSON
  capture.sh       — Camera capture (libcamera / raspistill / fswebcam / ffmpeg)
  ocr-meter-groq.sh — Groq Vision API OCR for physical meter images
  setup-cron.sh    — Cron scheduler setup
  test.sh          — Simulates 4 cron runs using sample.jpeg (no camera needed)
.cargo/
  config.toml  — aarch64-unknown-linux-gnu linker for cross-compilation
.env           — device ID + MQTT credentials (do not commit)
.env.example   — template to commit to version control
```

---

## Modbus register map

| Index | Sensor | Scale | Unit |
|---|---|---|---|
| 0 | DO Saturation | ÷10 | % |
| 1 | DO Concentration | ÷100 | mg/L |
| 2 | DO Temperature | ÷10 | °C |
| 3 | EC | direct | µS/cm |
| 4 | Salinity | direct | ppm |
| 5 | TDS | direct | ppm |
| 6 | EC Temperature | ÷100 | °C |
| 7 | pH | ÷10 | — |
| 8 | pH Temperature | ÷10 | °C |
| 9 | Status bits | bit0=DO · bit1=EC · bit2=pH | — |

Function code **0x03** (Read Holding Registers), starting at `0x0000`.
