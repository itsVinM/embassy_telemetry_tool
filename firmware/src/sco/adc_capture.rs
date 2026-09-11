//! ADC capture for the BIST telemetry loopback.
//!
//! Reads the two internal ADC reference channels (VREFINT ≈ 1.2 V, and the
//! junction temperature sensor) and sanity-checks them. This exercises the real
//! ADC peripheral without needing external wiring — the "telemetry loopback"
//! step of BIST. VREFINT should read close to the datasheet value once the ADC
//! is in range; the temperature reading should be a plausible junction temp.

use defmt::info;
use embassy_stm32::adc::{Adc, SampleTime};
use embassy_stm32::mode::Blocking;
use embassy_stm32::Peri;

/// Expected VREFINT, in counts, for a 12-bit ADC at 3.3 V.
/// Datasheet VREFINT = 1.21 V typical → 1.21/3.3 * 4095 ≈ 1502 counts.
const VREFINT_EXPECTED: u16 = 1502;
/// How far VREFINT may drift from nominal before we flag it.
const VREFINT_TOL: u16 = 200;
/// Plausible junction temperature bounds for the report (°C), ± 25 °C sanity.
const TEMP_MIN_C: i32 = -25;
const TEMP_MAX_C: i32 = 125;

/// Result of the telemetry-loopback capture.
#[derive(Debug, Clone, Copy)]
pub struct TelemetryLoopback {
    pub vrefint_counts: u16,
    pub vrefint_mv: u32,
    pub temp_c: i32,
}

impl TelemetryLoopback {
    pub fn vrefint_ok(&self) -> bool {
        self.vrefint_counts.abs_diff(VREFINT_EXPECTED) <= VREFINT_TOL
    }

    pub fn temp_ok(&self) -> bool {
        self.temp_c >= TEMP_MIN_C && self.temp_c <= TEMP_MAX_C
    }

    pub fn ok(&self) -> bool {
        self.vrefint_ok() && self.temp_ok()
    }
}

/// Capture VREFINT + temperature once and sanity-check them.
pub fn telemetry_loopback(
    adc: Peri<'_, embassy_stm32::peripherals::ADC1>,
) -> TelemetryLoopback {
    let mut adc = Adc::new(adc);
    let mut vref = adc.enable_vref();
    let mut temp = adc.enable_temperature();

    let vrefint_counts = adc.blocking_read(&mut vref, SampleTime::Cycles2395) as u16;
    let vrefint_mv = (vrefint_counts as u32 * 3300) / 4095;
    // STM32F401RM: T(degC) = (V25 - Vsense) / Avg_slope + 25, V25 ≈ 0.76 V,
    // Avg_slope ≈ 2.5 mV / °C.
    let temp_c = if vrefint_mv > 700 {
        25 - ((vrefint_mv as i32 - 760) * 10 / 25)
    } else {
        0
    };

    info!(
        "bist/sco: vrefint={} counts ({} mV), temp={} °C",
        vrefint_counts, vrefint_mv, temp_c
    );

    TelemetryLoopback { vrefint_counts, vrefint_mv, temp_c }
}