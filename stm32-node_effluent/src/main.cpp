#include <Arduino.h>
#include <HardwareSerial.h>
#include <IWatchdog.h>

HardwareSerial SensorSerial(PA3, PA2);   // RX=PA3, TX=PA2 — toward sensors
HardwareSerial RPiSerial(PA10, PA9);     // RX=PA10, TX=PA9 — toward RPi

#define SENSOR_BAUD       9600
#define RPI_BAUD          9600
#define READ_INTERVAL     5000UL     // ms between sensor polling cycles
#define SENSOR_TIMEOUT_MS 1000UL     // ms to wait for a sensor response
#define INTER_SENSOR_MS   300UL      // ms quiet gap between sensor requests
#define MODBUS_SILENT_MS  5UL        // ms gap for RPi frame-end detection
// 8 s watchdog — covers worst-case cycle (3× timeout + 2× gap = ~3.6 s)
#define WDT_TIMEOUT_US    8000000UL

// ── Modbus Slave Config ────────────────────────────────────────────────────────
#define SLAVE_ADDRESS   10
#define NUM_REGS        10
#define RESP_BUF_SIZE   25           // max Modbus response: 5 + 10×2 bytes

// ── Register map ──────────────────────────────────────────────────────────────
#define REG_DO_SAT      0   // DO Saturation ×10
#define REG_DO_CONC     1   // DO Concentration ×100
#define REG_DO_TEMP     2   // DO Temperature ×10
#define REG_EC          3   // EC µS/cm
#define REG_SALINITY    4   // Salinity ppm
#define REG_TDS         5   // TDS ppm
#define REG_EC_TEMP     6   // EC Temperature ×100
#define REG_PH          7   // pH ×10
#define REG_PH_TEMP     8   // pH Temperature ×10
#define REG_STATUS      9   // bit0=DO OK, bit1=EC OK, bit2=pH OK

static uint16_t regs[NUM_REGS] = {0};

// ══════════════════════════════════════════════════════════════════════════════
// CRC-16 MODBUS
// ══════════════════════════════════════════════════════════════════════════════
static uint16_t crc16(const uint8_t* data, uint8_t len) {
    uint16_t crc = 0xFFFF;
    for (uint8_t i = 0; i < len; i++) {
        crc ^= data[i];
        for (uint8_t j = 0; j < 8; j++)
            crc = (crc & 1) ? (crc >> 1) ^ 0xA001 : crc >> 1;
    }
    return crc;
}

static inline bool verifyCRC(const uint8_t* buf, uint8_t len) {
    uint16_t calc = crc16(buf, len - 2);
    uint16_t recv = (uint16_t)buf[len-2] | ((uint16_t)buf[len-1] << 8);
    return calc == recv;
}

// ══════════════════════════════════════════════════════════════════════════════
// SENSOR BUS HELPERS
// ══════════════════════════════════════════════════════════════════════════════
// Big-endian IEEE 754 from sensor → little-endian ARM host
static float parseFloat(const uint8_t* buf, uint8_t offset) {
    float val;
    uint8_t raw[4] = { buf[offset+3], buf[offset+2], buf[offset+1], buf[offset] };
    memcpy(&val, raw, 4);
    return val;
}

static inline uint16_t parseUINT16(const uint8_t* buf, uint8_t offset) {
    return ((uint16_t)buf[offset] << 8) | buf[offset+1];
}

static inline int16_t parseInt16(const uint8_t* buf, uint8_t offset) {
    return (int16_t)parseUINT16(buf, offset);
}

static void sendSensorRequest(uint8_t slave, uint8_t fc,
                              uint16_t reg, uint16_t n) {
    uint8_t frame[8];
    frame[0] = slave;
    frame[1] = fc;
    frame[2] = (reg >> 8) & 0xFF;
    frame[3] =  reg       & 0xFF;
    frame[4] = (n   >> 8) & 0xFF;
    frame[5] =  n         & 0xFF;
    uint16_t crc = crc16(frame, 6);
    frame[6] = crc & 0xFF;
    frame[7] = (crc >> 8) & 0xFF;
    while (SensorSerial.available()) SensorSerial.read();  // drain stale bytes
    SensorSerial.write(frame, 8);
    SensorSerial.flush();
}

// ── Non-blocking sensor state machine ─────────────────────────────────────────
enum SensorState : uint8_t {
    SS_IDLE,
    SS_SEND_DO, SS_WAIT_DO, SS_GAP_EC,
    SS_SEND_EC, SS_WAIT_EC, SS_GAP_PH,
    SS_SEND_PH, SS_WAIT_PH,
};

static SensorState sState    = SS_IDLE;
static uint32_t    sTimer    = 0;
static uint8_t     sBuf[17]  = {0};   // max response: DO sensor = 17 bytes
static uint8_t     sBufIdx   = 0;
static uint8_t     sExpected = 0;
static uint32_t    lastReadTime = 0;

static bool collectSensorBytes() {
    while (sBufIdx < sExpected && SensorSerial.available())
        sBuf[sBufIdx++] = SensorSerial.read();
    return sBufIdx >= sExpected;
}

static void tickSensors() {
    uint32_t now = millis();
    switch (sState) {

        case SS_IDLE:
            if (now - lastReadTime >= READ_INTERVAL)
                sState = SS_SEND_DO;
            break;

        case SS_SEND_DO:
            sendSensorRequest(1, 0x03, 0x0000, 6);
            sBufIdx = 0; sExpected = 17; sTimer = now;
            sState = SS_WAIT_DO;
            break;

        case SS_WAIT_DO:
            if (collectSensorBytes()) {
                if (verifyCRC(sBuf, 17)) {
                    regs[REG_DO_SAT]  = (uint16_t)(parseFloat(sBuf,  3) * 1000.0f);
                    regs[REG_DO_CONC] = (uint16_t)(parseFloat(sBuf,  7) * 100.0f);
                    regs[REG_DO_TEMP] = (uint16_t)(parseFloat(sBuf, 11) * 10.0f);
                    regs[REG_STATUS] |= (1 << 0);
                    Serial.println(F("[ID1] DO OK"));
                } else {
                    regs[REG_STATUS] &= ~(uint16_t)(1 << 0);
                    Serial.println(F("[ID1] DO CRC ERR"));
                }
                sTimer = now; sState = SS_GAP_EC;
            } else if (now - sTimer >= SENSOR_TIMEOUT_MS) {
                regs[REG_STATUS] &= ~(uint16_t)(1 << 0);
                Serial.println(F("[ID1] DO TIMEOUT"));
                sTimer = now; sState = SS_GAP_EC;
            }
            break;

        case SS_GAP_EC:
            if (now - sTimer >= INTER_SENSOR_MS) sState = SS_SEND_EC;
            break;

        case SS_SEND_EC:
            sendSensorRequest(2, 0x04, 0x0000, 5);
            sBufIdx = 0; sExpected = 15; sTimer = now;
            sState = SS_WAIT_EC;
            break;

        case SS_WAIT_EC:
            if (collectSensorBytes()) {
                if (verifyCRC(sBuf, 15)) {
                    regs[REG_EC_TEMP]  = (uint16_t)parseInt16(sBuf, 3);
                    regs[REG_EC]       = parseUINT16(sBuf, 7);
                    regs[REG_SALINITY] = parseUINT16(sBuf, 9);
                    regs[REG_TDS]      = parseUINT16(sBuf, 11);
                    regs[REG_STATUS]  |= (1 << 1);
                    Serial.println(F("[ID2] EC OK"));
                } else {
                    regs[REG_STATUS] &= ~(uint16_t)(1 << 1);
                    Serial.println(F("[ID2] EC CRC ERR"));
                }
                sTimer = now; sState = SS_GAP_PH;
            } else if (now - sTimer >= SENSOR_TIMEOUT_MS) {
                regs[REG_STATUS] &= ~(uint16_t)(1 << 1);
                Serial.println(F("[ID2] EC TIMEOUT"));
                sTimer = now; sState = SS_GAP_PH;
            }
            break;

        case SS_GAP_PH:
            if (now - sTimer >= INTER_SENSOR_MS) sState = SS_SEND_PH;
            break;

        case SS_SEND_PH:
            sendSensorRequest(3, 0x03, 0x0000, 4);
            sBufIdx = 0; sExpected = 13; sTimer = now;
            sState = SS_WAIT_PH;
            break;

        case SS_WAIT_PH:
            if (collectSensorBytes()) {
                if (verifyCRC(sBuf, 13)) {
                    regs[REG_PH_TEMP] = parseUINT16(sBuf, 3);
                    regs[REG_PH]      = parseUINT16(sBuf, 5);
                    regs[REG_STATUS] |= (1 << 2);
                    Serial.println(F("[ID3] pH OK"));
                } else {
                    regs[REG_STATUS] &= ~(uint16_t)(1 << 2);
                    Serial.println(F("[ID3] pH CRC ERR"));
                }
                lastReadTime = millis(); sState = SS_IDLE;
            } else if (now - sTimer >= SENSOR_TIMEOUT_MS) {
                regs[REG_STATUS] &= ~(uint16_t)(1 << 2);
                Serial.println(F("[ID3] pH TIMEOUT"));
                lastReadTime = millis(); sState = SS_IDLE;
            }
            break;
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// RPI BUS — Modbus Slave
// ══════════════════════════════════════════════════════════════════════════════
static uint8_t  rxBuf[32]    = {0};
static uint8_t  rxIdx        = 0;
static uint32_t lastByteTime = 0;

static void sendException(uint8_t fc, uint8_t code) {
    uint8_t err[5];
    err[0] = SLAVE_ADDRESS;
    err[1] = fc | 0x80;
    err[2] = code;
    uint16_t crc = crc16(err, 3);
    err[3] = crc & 0xFF;
    err[4] = (crc >> 8) & 0xFF;
    RPiSerial.write(err, 5);
}

static void handleModbusRequest() {
    if (rxIdx < 8) return;
    if (rxBuf[0] != SLAVE_ADDRESS) return;
    if (!verifyCRC(rxBuf, rxIdx)) {
        Serial.println(F("[RPi] CRC error"));
        return;
    }

    uint8_t  fc       = rxBuf[1];
    uint16_t startReg = ((uint16_t)rxBuf[2] << 8) | rxBuf[3];
    uint16_t numRegs  = ((uint16_t)rxBuf[4] << 8) | rxBuf[5];

    if (fc != 0x03 && fc != 0x04) {
        sendException(fc, 0x01);   // illegal function
        return;
    }
    if (numRegs == 0 || startReg + numRegs > NUM_REGS) {
        sendException(fc, 0x02);   // illegal data address
        return;
    }

    uint8_t byteCount = numRegs * 2;
    uint8_t resp[RESP_BUF_SIZE];
    resp[0] = SLAVE_ADDRESS;
    resp[1] = fc;
    resp[2] = byteCount;
    for (uint16_t i = 0; i < numRegs; i++) {
        resp[3 + i*2]     = (regs[startReg + i] >> 8) & 0xFF;
        resp[3 + i*2 + 1] =  regs[startReg + i]       & 0xFF;
    }
    uint16_t crc = crc16(resp, 3 + byteCount);
    resp[3 + byteCount]     = crc & 0xFF;
    resp[3 + byteCount + 1] = (crc >> 8) & 0xFF;
    RPiSerial.write(resp, 5 + byteCount);

    Serial.print(F("[RPi] FC")); Serial.print(fc);
    Serial.print(F(" reg:"));    Serial.print(startReg);
    Serial.print(F(" n:"));      Serial.println(numRegs);
}

static void listenRPi() {
    while (RPiSerial.available()) {
        if (rxIdx >= sizeof(rxBuf)) {
            rxIdx = 0;   // discard overflowed frame, start fresh
            Serial.println(F("[RPi] RX overflow"));
        }
        rxBuf[rxIdx++] = RPiSerial.read();
        lastByteTime = millis();
    }
    if (rxIdx > 0 && millis() - lastByteTime >= MODBUS_SILENT_MS) {
        handleModbusRequest();
        rxIdx = 0;
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// SETUP & LOOP
// ══════════════════════════════════════════════════════════════════════════════
void setup() {
    Serial.begin(115200);
    // Wait for USB up to 3 s — device must boot headless if no host connected
    uint32_t t0 = millis();
    while (!Serial && millis() - t0 < 3000) delay(10);

    SensorSerial.begin(SENSOR_BAUD);
    RPiSerial.begin(RPI_BAUD);

    Serial.println(F("==========================================="));
    Serial.println(F("  STM32 BlackPill - RS485 Node Collector"));
    Serial.println(F("  Sensor bus : UART1  PA3/PA2"));
    Serial.println(F("  RPi bus    : UART2  PA10/PA9"));
    Serial.println(F("  Slave addr : 10"));
    Serial.println(F("==========================================="));

    // Start watchdog — must be kicked every WDT_TIMEOUT_US microseconds
    IWatchdog.begin(WDT_TIMEOUT_US);

    // Kick off first sensor cycle immediately
    lastReadTime = millis() - READ_INTERVAL;
}

void loop() {
    IWatchdog.reload();   // kick watchdog — proves loop is alive
    listenRPi();          // always service RPi requests first
    tickSensors();        // advance non-blocking sensor state machine
}
