#include <Arduino.h>
#include <avr/wdt.h>
#include <PZEM004Tv30.h>
#include <SoftwareSerial.h>

// Explicit SoftwareSerial objects — avoids deprecated raw-pin PZEM constructor.
// ATmega328P can only actively listen to one SoftwareSerial at a time;
// the PZEM library calls listen() before each receive.
SoftwareSerial pzemSerial1(3, 2);  // RX=3, TX=2 → Pump 1
SoftwareSerial pzemSerial2(4, 5);  // RX=4, TX=5 → Pump 2

PZEM004Tv30 pzem1(pzemSerial1);
PZEM004Tv30 pzem2(pzemSerial2);

// Saturating consecutive-failure counters, one per sensor.
// Reset to 0 on success; cap at 255 so they never overflow.
static uint8_t errCount[2] = {0, 0};

// Emits one JSON field for a sensor (no surrounding braces on the whole response).
// Success: "1":{"v":220.1,"i":1.234,"w":270.1,"e":0.123,"hz":50.0,"pf":0.95,"err":0}
// Failure: "1":{"err":5}   — no sensor fields; RPi checks for absence of "v"
static void sendPzemJson(PZEM004Tv30& pzem, uint8_t idx) {
    float voltage   = pzem.voltage();
    float current   = pzem.current();
    float power     = pzem.power();
    float energy    = pzem.energy();
    float frequency = pzem.frequency();
    float pf        = pzem.pf();

    const bool ok = !isnan(voltage) && !isnan(current) && !isnan(power) &&
                    !isnan(energy)  && !isnan(frequency) && !isnan(pf);

    if (!ok) { if (errCount[idx] < 255) errCount[idx]++; }
    else      { errCount[idx] = 0; }

    Serial.print('"'); Serial.print(idx + 1); Serial.print(F("\":"));

    if (!ok) {
        Serial.print(F("{\"err\":")); Serial.print(errCount[idx]); Serial.print('}');
        return;
    }

    Serial.print(F("{\"v\":"));   Serial.print(voltage,   1);
    Serial.print(F(",\"i\":"));   Serial.print(current,   3);
    Serial.print(F(",\"w\":"));   Serial.print(power,     1);
    Serial.print(F(",\"e\":"));   Serial.print(energy,    3);
    Serial.print(F(",\"hz\":")); Serial.print(frequency, 1);
    Serial.print(F(",\"pf\":")); Serial.print(pf,        2);
    Serial.print(F(",\"err\":0}"));
}

// READ → single JSON line: {"ts":MILLIS,"1":{...},"2":{...}}
// millis() rolls over after 49 days — RPi should handle the jump.
static void handleRead() {
    wdt_reset();  // pat before the ~200 ms sensor reads
    Serial.print(F("{\"ts\":"));
    Serial.print(millis());
    Serial.print(',');
    sendPzemJson(pzem1, 0);
    Serial.print(',');
    sendPzemJson(pzem2, 1);
    Serial.println('}');
}

void setup() {
    wdt_enable(WDTO_4S);
    Serial.begin(9600);

    // PZEM modules need ~1–2 s after power-on before they respond to Modbus.
    // Pet the watchdog in each iteration so it does not fire during this delay.
    for (uint8_t i = 0; i < 20; i++) {
        wdt_reset();
        delay(100);
    }
}

void loop() {
    wdt_reset();

    // Line-oriented command parser.
    // - Handles \n and \r\n line endings.
    // - overflow flag: set when a line exceeds buf capacity; drains silently
    //   until the next newline rather than re-using buf mid-overflow.
    // - cmdStart timeout: a partial line idle for >2 s is treated as noise
    //   (covers RPi crash / cable glitch mid-transmission).
    static char     buf[8]      = {};
    static uint8_t  pos         = 0;
    static bool     overflow    = false;
    static uint32_t cmdStart    = 0;

    // Timeout check runs even when no new bytes arrive.
    if (pos > 0 && (uint32_t)(millis() - cmdStart) > 2000UL) {
        pos      = 0;
        overflow = false;
    }

    while (Serial.available()) {
        char c = (char)Serial.read();

        if (c == '\n' || c == '\r') {
            buf[pos] = '\0';
            if (!overflow && pos > 0) {
                if      (strcmp_P(buf, PSTR("READ")) == 0) handleRead();
                else if (strcmp_P(buf, PSTR("PING")) == 0) Serial.println(F("PONG"));
                // Unknown commands are silently ignored.
            }
            pos      = 0;
            overflow = false;

        } else if (overflow) {
            // Drain until next newline — do nothing with this byte.

        } else if (pos < sizeof(buf) - 1) {
            if (pos == 0) cmdStart = millis();  // start the timeout clock
            buf[pos++] = c;

        } else {
            // Buffer full without a newline — this line is garbage.
            overflow = true;
            pos      = 0;
        }
    }
}
