# raspi-collector

Rust application for Raspberry Pi 4 (64-bit) that polls two different IoT nodes:

| Interface | Protocol | Node | Data |
|---|---|---|---|
| `/dev/ttyS0` (RS485) | Modbus RTU | STM32 water-quality node | DO, EC, Salinity, TDS, pH + temperatures |
| `/dev/ttyUSB0` (USB UART) | JSON over serial | Pump monitor node | Per-pump voltage, wattage, error code |

---

## Hardware setup

```
STM32 water-quality node
  └─ RS485 transceiver ─── /dev/ttyS0    (RPi hardware UART, GPIO 14/15)

Pump monitor node
  └─ USB-UART adapter  ─── /dev/ttyUSB0  (e.g. CH340 / FT232)
     Protocol: send "READ\n", receive JSON line
     {"ts":"…","pump1":{"err":0,"v":220.5,"w":150.3},…}
```

---

## Configuration

Edit [`src/config.rs`](src/config.rs) before building:

| Constant | Default | Description |
|---|---|---|
| `RS485_PORT` | `/dev/ttyS0` | Hardware UART port |
| `RS485_BAUD` | `9600` | Baud rate |
| `RS485_SLAVE` | `10` | Modbus slave address |
| `USB_PORT` | `/dev/ttyUSB0` | USB UART port |
| `USB_BAUD` | `9600` | Baud rate |
| `READ_INTERVAL_SECS` | `5` | Poll interval (seconds) |
| `TIMEOUT_MS` | `2000` | Response timeout per poll |
| `MAX_RETRIES` | `3` | Retries before marking device offline |

---

## Build

### Native (on the Raspberry Pi itself)

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

cargo build --release
./target/release/raspi-collector
```

### Cross-compile from macOS / Linux to RPi 4 (aarch64, 64-bit)

**1. Add the Rust target:**
```bash
rustup target add aarch64-unknown-linux-gnu
```

**2. Install the cross-compiler:**

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

The binary is at `target/aarch64-unknown-linux-gnu/release/raspi-collector`.

**4. Deploy to the Pi:**
```bash
scp target/aarch64-unknown-linux-gnu/release/raspi-collector pi@<PI_IP>:~/
ssh pi@<PI_IP> ./raspi-collector
```

---

## Running

```bash
./raspi-collector
```

Log level is controlled via `RUST_LOG` (default `info`):

```bash
RUST_LOG=debug ./raspi-collector   # verbose — shows all retry attempts
RUST_LOG=warn  ./raspi-collector   # quiet  — only warnings and errors
```

### Sample output

```
2026-04-17T08:00:00Z INFO  raspi-collector v0.1.0 starting
2026-04-17T08:00:00Z INFO  RS485 -> /dev/ttyS0 @ 9600 baud  slave 10
2026-04-17T08:00:00Z INFO  USB   -> /dev/ttyUSB0 @ 9600 baud  slave 10
2026-04-17T08:00:00Z INFO  [RS485] opened /dev/ttyS0
2026-04-17T08:00:00Z INFO  [RS485] ─────────────────────────────────────────
2026-04-17T08:00:00Z INFO  [RS485]   Avg Temperature :  25.3 °C
2026-04-17T08:00:00Z INFO  [RS485]   [DO]  OK
2026-04-17T08:00:00Z INFO  [RS485]         Saturation       98.4 %
2026-04-17T08:00:00Z INFO  [RS485]         Concentration     7.82 mg/L
2026-04-17T08:00:00Z INFO  [RS485]         Temperature      25.1 °C
2026-04-17T08:00:00Z INFO  [RS485]   [EC]  OK
2026-04-17T08:00:01Z ERROR [USB] device OFFLINE — no response after 3 attempts
```

### Device offline / recovery

When a node stops responding after `MAX_RETRIES` attempts, the collector logs **OFFLINE once** and keeps polling silently. When the node comes back, it logs **back ONLINE** and resumes normal output.

---

## Project structure

```
src/
  config.rs    — port paths, baud rates, timeouts, retry limits
  modbus.rs    — Modbus RTU protocol: CRC-16, frame builder, response reader, register parser
  sensor.rs    — SensorData struct, register mapping, avg_temperature()
  reader.rs    — SensorReader trait + ModbusReader (retry, auto-reconnect, offline tracking)
  pump.rs      — PumpData / PumpEntry structs, JSON parsing
  uart_usb.rs  — UartJsonReader: send READ\n, read JSON line, retry, offline tracking
  main.rs      — logger init, RS485 + USB readers, poll loop
.cargo/
  config.toml — aarch64 cross-compile linker
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
| 9 | Status bits | bit0=DO, bit1=EC, bit2=pH | — |

Function code **0x03** (Read Holding Registers), starting at register `0x0000`.
