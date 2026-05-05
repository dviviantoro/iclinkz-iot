// Register indices in the 10-register Modbus response
const REG_DO_SAT:   usize = 0;  // ×10  → %
const REG_DO_CONC:  usize = 1;  // ×100 → mg/L
const REG_DO_TEMP:  usize = 2;  // ×10  → °C
const REG_EC:       usize = 3;  // direct µS/cm
const REG_SALINITY: usize = 4;  // direct ppm
const REG_TDS:      usize = 5;  // direct ppm
const REG_EC_TEMP:  usize = 6;  // ×100 → °C
const REG_PH:       usize = 7;  // ×10  → pH
const REG_PH_TEMP:  usize = 8;  // ×10  → °C
const REG_STATUS:   usize = 9;  // bit0=DO, bit1=EC, bit2=pH

#[derive(Debug, Default, Clone)]
pub struct SensorData {
    pub source:           String,
    pub do_saturation:    f32,    // %
    pub do_concentration: f32,    // mg/L
    pub do_temperature:   f32,    // °C
    pub ec:               u16,    // µS/cm
    pub salinity:         u16,    // ppm
    pub tds:              u16,    // ppm
    pub ec_temperature:   f32,    // °C
    pub ph:               f32,
    pub raw_ph:           Option<f32>,
    pub ph_temperature:   f32,    // °C
    pub do_ok:            bool,
    pub ec_ok:            bool,
    pub ph_ok:            bool,
}

impl SensorData {
    pub fn from_registers(regs: &[u16], source: String) -> Self {
        let status = regs[REG_STATUS];
        SensorData {
            source,
            do_saturation:    regs[REG_DO_SAT]  as f32 / 10.0,
            do_concentration: regs[REG_DO_CONC] as f32 / 100.0,
            do_temperature:   regs[REG_DO_TEMP] as f32 / 10.0,
            ec:               regs[REG_EC],
            salinity:         regs[REG_SALINITY],
            tds:              regs[REG_TDS],
            ec_temperature:   regs[REG_EC_TEMP] as f32 / 100.0,
            ph:               regs[REG_PH]      as f32 / 10.0,
            raw_ph:           None,
            ph_temperature:   regs[REG_PH_TEMP] as f32 / 10.0,
            do_ok:            status & (1 << 0) != 0,
            ec_ok:            status & (1 << 1) != 0,
            ph_ok:            status & (1 << 2) != 0,
        }
    }

    /// Mean temperature across all sensors that are reporting OK.
    /// Returns None when no sensor is healthy.
    pub fn avg_temperature(&self) -> Option<f32> {
        let mut sum   = 0.0f32;
        let mut count = 0u32;
        if self.do_ok { sum += self.do_temperature; count += 1; }
        if self.ec_ok { sum += self.ec_temperature; count += 1; }
        if self.ph_ok { sum += self.ph_temperature; count += 1; }
        (count > 0).then(|| sum / count as f32)
    }
}
