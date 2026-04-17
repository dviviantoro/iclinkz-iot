# iclinkz-iot

Monorepo firmware & software for an **IPAL (Instalasi Pengolahan Air Limbah / Wastewater Treatment Plant)** IoT monitoring system.

## System Architecture

```
 ┌─────────────────────────────────────┐      ┌──────────────────────────────────────┐
 │           CONTROL BOX               │      │           NODE BOX (near chamber)    │
 │                                     │      │                                      │
 │  ┌──────────────────────────────┐   │      │  ┌────────────────────────────────┐  │
 │  │      Raspberry Pi            │   │      │  │   STM32 BlackPill F411CE       │  │
 │  │      raspi-collector         │   │      │  │   stm32-node_effluent          │  │
 │  │                              │   │      │  │                                │  │
 │  │  /dev/ttyS0 ◀── RS485 ───────┼───┼──────┼──▶ PA10/PA9  (Modbus Slave #10)   │  │
 │  │  /dev/ttyUSB0 ◀─ USB ────────┼───┤      │  │                                │  │
 │  └──────────────┬───────────────┘   │      │  │  PA3/PA2  (Modbus Master)      │  │
 │                 │ USB               │      │  └───┬────────────────────────────┘  │
 │  ┌──────────────▼───────────────┐   │      │      │ RS485 bus                     │
 │  │      Arduino Nano            │   │      │      │                               │
 │  │      nano-pump_activity      │   │      │  ┌───▼──────────┐                    │
 │  │                              │   │      │  │  DO Sensor   │  Slave 1           │
 │  │   D3/D2 ── PZEM → Pump 1     │   │      │  ├──────────────┤                    │
 │  │   D4/D5 ── PZEM → Pump 2     │   │      │  │  EC Sensor   │  Slave 2           │
 │  └──────────────────────────────┘   │      │  ├──────────────┤                    │
 │                                     │      │  │  pH Sensor   │  Slave 3           │
 └─────────────────────────────────────┘      │  └──────────────┘                    │
                    │                         └──────────────────────────────────────┘
                    │ MQTT / Network
                    ▼
             [ Backend / Cloud ]
```

## Components

### [`raspi-collector/`](raspi-collector/) — Data Collector (Rust · Raspberry Pi)

The main program running on the Raspberry Pi inside the **control box**. It polls both data sources every 5 seconds and forwards the results upstream via MQTT.

| Port | Connection | Protocol |
|------|-----------|----------|
| `/dev/ttyS0` | STM32 BlackPill via RS485 transceiver | Modbus RTU (slave addr `10`) |
| `/dev/ttyUSB0` | Arduino Nano via USB | JSON over UART (`READ\n` command) |

**Water quality registers (RS485 → STM32):**

| Reg | Parameter | Unit | Scale |
|-----|-----------|------|-------|
| 0 | DO Saturation | % | ÷1000 |
| 1 | DO Concentration | mg/L | ÷100 |
| 2 | DO Temperature | °C | ÷10 |
| 3 | EC | µS/cm | — |
| 4 | Salinity | ppm | — |
| 5 | TDS | ppm | — |
| 6 | EC Temperature | °C | ÷100 |
| 7 | pH | — | ÷10 |
| 8 | pH Temperature | °C | ÷10 |
| 9 | Status bitmask | — | bit0=DO, bit1=EC, bit2=pH |

**Pump activity payload (USB → Nano):**

```json
{
  "ts": 12345,
  "1": { "v": 220.1, "i": 1.234, "w": 270.1, "e": 0.123, "hz": 50.0, "pf": 0.95, "err": 0 },
  "2": { "err": 3 }
}
```

When a PZEM sensor fails, only the `err` field is present (no `v`, `i`, etc.). The Raspberry Pi detects offline status by checking for the absence of `v`.

**Build & deploy:**

```bash
cd raspi-collector
cargo build --release

# On the Raspberry Pi
./target/release/raspi-collector

# Adjust log verbosity
RUST_LOG=debug ./target/release/raspi-collector
```

**Configuration** — edit [`src/config.rs`](raspi-collector/src/config.rs):

```rust
pub const RS485_PORT:          &str = "/dev/ttyS0";
pub const USB_PORT:            &str = "/dev/ttyUSB0";
pub const READ_INTERVAL_SECS:  u64  = 5;
pub const RS485_SLAVE:         u8   = 10;
```

---

### [`nano-pump_activity/`](nano-pump_activity/) — Pump Monitor (Arduino Nano · Control Box)

Arduino Nano (ATmega328P) firmware that monitors the electrical activity of two pumps using PZEM-004T v3.0 modules. Lives in the same **control box** as the Raspberry Pi and communicates with it over USB serial.

**Wiring:**

| Pin | Function |
|-----|----------|
| D3 (RX) / D2 (TX) | SoftwareSerial → PZEM-004T — Pump 1 |
| D4 (RX) / D5 (TX) | SoftwareSerial → PZEM-004T — Pump 2 |
| USB | Serial link to Raspberry Pi (`/dev/ttyUSB0`, 9600 baud) |

**Command protocol:**

| Command | Response |
|---------|----------|
| `READ\n` | `{"ts":<millis>,"1":{...},"2":{...}}\n` |
| `PING\n` | `PONG\n` |

**Response fields per pump:**

| Field | Description |
|-------|-------------|
| `v` | Voltage (V) |
| `i` | Current (A) |
| `w` | Active power (W) |
| `e` | Energy (kWh) |
| `hz` | Frequency (Hz) |
| `pf` | Power factor |
| `err` | Consecutive error count — `0` means OK |

A 4-second watchdog timer is enabled for automatic recovery from firmware lockups.

**Build & flash (PlatformIO):**

```bash
cd nano-pump_activity
pio run --target upload
```

---

### [`stm32-node_effluent/`](stm32-node_effluent/) — Effluent Node (STM32 BlackPill F411CE · Node Box)

STM32 BlackPill F411CE firmware deployed in a **dedicated node box placed near the effluent chamber**. It acts as a **Modbus Master** toward the water-quality sensors and simultaneously as a **Modbus Slave** (address `10`) toward the Raspberry Pi.

**Wiring:**

| Pin | Function |
|-----|----------|
| PA3 (RX) / PA2 (TX) | UART1 → RS485 sensor bus (Modbus Master) |
| PA10 (RX) / PA9 (TX) | UART2 → RS485 RPi bus (Modbus Slave addr `10`) |
| USB | Debug serial output (115200 baud) |

**Sensors on the RS485 bus (polled every 5 s):**

| Slave | Sensor | Measured parameters |
|-------|--------|---------------------|
| 1 | DO meter | Saturation (%), Concentration (mg/L), Temperature (°C) |
| 2 | EC/TDS meter | EC (µS/cm), Salinity (ppm), TDS (ppm), Temperature (°C) |
| 3 | pH meter | pH, Temperature (°C) |

The non-blocking sensor state machine (DO → gap → EC → gap → pH → idle) ensures the main loop is never blocked. An 8-second watchdog covers the worst-case polling cycle (~3.6 s).

**Build & flash (PlatformIO · DFU mode):**

```bash
cd stm32-node_effluent
# Hold BOOT0 then press RESET to enter DFU mode
pio run --target upload
```

---

## Data Flow

```
[ NODE BOX — near chamber ]                 [ CONTROL BOX ]
                                                          
  DO / EC / pH Sensors                                    
       │ RS485 Modbus (Master)                            
       ▼                                                  
  STM32 BlackPill ──── RS485 Modbus (Slave #10) ────────▶ Raspberry Pi
                                                           (raspi-collector)
  PZEM-004T × 2                                                │
       │ SoftwareSerial                                        │  poll every 5 s
       ▼                                                        │
  Arduino Nano ──── USB JSON ──────────────────────────────────┘
                                                                │
                                                                ▼
                                                        MQTT / Backend
```

---

## Prerequisites

| Component | Toolchain |
|-----------|-----------|
| `raspi-collector` | Rust ≥ 1.70, `cargo` |
| `nano-pump_activity` | PlatformIO, platform `atmelavr` |
| `stm32-node_effluent` | PlatformIO, platform `ststm32`, `dfu-util` |

---

## Repository Structure

```
iclinkz-iot/
├── raspi-collector/          # Rust — data aggregator on Raspberry Pi
│   ├── src/
│   │   ├── main.rs           # Entry point & polling loop
│   │   ├── config.rs         # Port, baud rate, timing constants
│   │   ├── modbus.rs         # Modbus RTU framing & CRC-16
│   │   ├── reader.rs         # ModbusReader / SensorReader traits
│   │   ├── sensor.rs         # SensorData struct (DO / EC / pH)
│   │   ├── pump.rs           # PumpData struct
│   │   └── uart_usb.rs       # UART JSON reader for Nano
│   └── Cargo.toml
├── nano-pump_activity/       # Arduino Nano — pump electrical monitor
│   ├── src/main.cpp
│   └── platformio.ini
└── stm32-node_effluent/      # STM32 BlackPill — effluent water quality node
    ├── src/main.cpp
    └── platformio.ini
```

---

## License

MIT
