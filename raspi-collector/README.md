# raspi-collector

Rust data-collection daemon for **Raspberry Pi 4 (64-bit)** that reads three IoT nodes and publishes to an MQTT broker.

| Interface | Protocol | Node | Data |
|---|---|---|---|
| `/dev/ttyS0` (RS485) | Modbus RTU | STM32 water-quality node | DO, EC, Salinity, TDS, pH + temperatures |
| `/dev/ttyUSB0` (USB UART) | JSON over serial | Pump monitor node | Per-pump V, A, Hz, PF, W, kWh |
| `/dev/i2c-1` (I2C) | AHT10 native | Ambient sensor | Temperature, Humidity |

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

---

## Build

### Native (on the Raspberry Pi itself)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo build --release
./target/release/raspi-collector
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

Binary: `target/aarch64-unknown-linux-gnu/release/raspi-collector`

**4. Deploy:**
```bash
# Copy binary and config to the Pi
scp target/aarch64-unknown-linux-gnu/release/raspi-collector pi@<PI_IP>:~/collector/
scp .env pi@<PI_IP>:~/collector/

# Run
ssh pi@<PI_IP> "cd ~/collector && ./raspi-collector"
```

---

## Usage

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

### Examples

```bash
# Default — publish to MQTT, .env in current directory
./raspi-collector

# .env stored next to the binary (run from anywhere)
./raspi-collector --env-file /home/pi/collector/.env

# Read-only — log sensor data, no MQTT
./raspi-collector --mode read-only

# Subscribe mode — also listen for incoming control commands
./raspi-collector --mode subscriber --env-file /etc/raspi-collector.env
```

### Log level

```bash
RUST_LOG=info  ./raspi-collector   # default — readings + state changes
RUST_LOG=debug ./raspi-collector   # verbose — every retry attempt
RUST_LOG=warn  ./raspi-collector   # quiet   — only warnings and errors
```

### Sample output

```
Loaded env: /home/pi/collector/.env
2026-04-20T08:00:00Z INFO  raspi-collector v0.1.0 | mode: Mqtt
2026-04-20T08:00:00Z INFO  RS485  -> /dev/ttyS0 @ 9600 baud  slave 10
2026-04-20T08:00:00Z INFO  USB    -> /dev/ttyUSB0 @ 9600 baud  (JSON)
2026-04-20T08:00:00Z INFO  AHT10  -> /dev/i2c-1  addr 0x38
2026-04-20T08:00:00Z INFO  [MQTT] connected as 'rpi-001'
2026-04-20T08:00:00Z INFO  [AHT10] ──────────────────────────────────────
2026-04-20T08:00:00Z INFO  [AHT10]   Temperature :   27.43 °C
2026-04-20T08:00:00Z INFO  [AHT10]   Humidity    :   65.20 %
2026-04-20T08:00:00Z INFO  [RS485] ──────────────────────────────────────
2026-04-20T08:00:00Z INFO  [RS485]   Avg Water Temp : 25.3 °C
2026-04-20T08:00:00Z INFO  [RS485]   [DO]  OK
2026-04-20T08:00:00Z INFO  [RS485]         Saturation       98.4 %
2026-04-20T08:00:00Z INFO  [RS485]         Concentration     7.82 mg/L
2026-04-20T08:00:00Z INFO  [RS485]   [EC]  OK
2026-04-20T08:00:00Z INFO  [RS485]         EC             1240 µS/cm
2026-04-20T08:00:00Z INFO  [RS485]   [pH]  OK
2026-04-20T08:00:00Z INFO  [RS485]         pH               7.2
2026-04-20T08:00:00Z INFO  [USB] ──────────────────────────────────────
2026-04-20T08:00:00Z INFO  [USB]   pump1 : ONLINE |  220.50V   5.20A  50.0Hz PF=0.95   150.30W    0.500kWh
2026-04-20T08:00:01Z ERROR [USB] device OFFLINE — no response after 3 attempts
2026-04-20T08:00:45Z INFO  [USB] device back ONLINE
```

---

## MQTT topics

All topics are prefixed with `DEVICE_ID` from `.env`.

### Published

| Topic | Payload |
|---|---|
| `{DEVICE_ID}/sensors/chamber_effluent` | `{"ph":7.2,"water_temp":25.3,"do":7.82,"tds":620,"ec":1240}` |
| `{DEVICE_ID}/sensors/blower_pump` | `{"v1":220.5,"i1":5.2,"f1":50.0,"pf1":0.95,"p1":150.3,"e1":0.5,"v2":…}` |
| `{DEVICE_ID}/sensors/ambient` | `{"amb_temp":27.43,"amb_hum":65.20}` |

### Subscribed (`--mode subscriber`)

| Topic | Purpose |
|---|---|
| `{DEVICE_ID}/control/#` | Incoming control commands (logged; extend in `src/mqtt.rs`) |

---

## Offline / recovery behaviour

Each reader tracks its own connection state independently:

- After `MAX_RETRIES` consecutive failures → logs **OFFLINE** once, polls silently
- On next successful read → logs **back ONLINE**, resumes normal output
- Applies to all three nodes (RS485, USB UART, AHT10 I2C)

---

## Running as a systemd service

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

```bash
sudo systemctl enable raspi-collector
sudo systemctl start  raspi-collector
sudo journalctl -fu   raspi-collector
```

---

## Project structure

```
src/
  main.rs      — CLI parse, env load, logger init, reader + MQTT wiring, poll loop
  cli.rs       — clap argument definitions (--mode, --env-file)
  config.rs    — hardware constants (ports, baud rates, timeouts, retry limits)
  modbus.rs    — Modbus RTU protocol: CRC-16, frame builder, response reader, parser
  sensor.rs    — SensorData struct, register-to-field mapping, avg_temperature()
  reader.rs    — SensorReader trait + ModbusReader (retry, auto-reconnect, offline state)
  pump.rs      — PumpData / PumpEntry structs, JSON parsing (v, i, f, pf, p, e)
  uart_usb.rs  — UartJsonReader: send READ\n, parse JSON line, retry, offline state
  aht10.rs     — Aht10Reader: I2C init + trigger + parse, retry, offline state [Linux]
  mqtt.rs      — MqttEnv (from .env), MqttPublisher, payload builders, event-loop thread
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
