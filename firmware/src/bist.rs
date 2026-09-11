//! Built-In Self-Test — verifies the MCU's own integrity before doing any work.
//!
//! Boot order enforced by `main.rs`:
//!   1. MPU configured + stack canary placed (done before this runs).
//!   2. Health checks: clock tree, stack canary readback, RAM pattern test.
//!   3. Fault-injector stability check (SPI/I2C/UART injectors vs simulated buses).
//!
//! If any step fails, `run()` returns `BistResult::Fail` and the firmware halts —
//! an unstable tool is not trusted to validate anything.

use shared::{HealthError, HealthStatus};

use crate::fault::StabilityReport;

/// Compact report the host can read over USB CDC.
#[derive(Debug, Clone, Copy)]
pub enum BistResult {
    Pass,
    Fail(HealthError),
    FaultStability(StabilityReport),
}

impl BistResult {
    pub fn ok(&self) -> bool {
        matches!(self, BistResult::Pass)
    }

    /// Single byte wire encoding for the host.
    pub fn code(&self) -> u8 {
        match self {
            BistResult::Pass => 0x00,
            BistResult::Fail(e) => match e {
                HealthError::StackCanary => 0x01,
                HealthError::RamTest => 0x02,
                HealthError::TimerNotTicking => 0x03,
                HealthError::ClockOutOfRange => 0x04,
                HealthError::ClockHclkNotRunning => 0x05,
            },
            BistResult::FaultStability(r) => {
                if r.ok() { 0x10 } else { 0x11 }
            }
        }
    }
}

/// Runs the full BIST sequence. Returns `Pass` only when every check passed.
pub fn run(rcc_peripheral: embassy_stm32::Peri<'_, embassy_stm32::peripherals::RCC>) -> BistResult {
    use defmt::info;

    info!("bist: starting…");

    // ── 1. Stack canary readback ──────────────────────────────────────────
    let canary_status = crate::health::check_stack_canary();
    if !matches!(canary_status, HealthStatus::Ready) {
        return BistResult::Fail(HealthError::StackCanary);
    }

    // ── 2. RAM pattern test ───────────────────────────────────────────────
    let ram_status = crate::health::check_ram();
    if let HealthStatus::Fail(e) = ram_status {
        info!("bist: FAIL — {:?}", e);
        return BistResult::Fail(e);
    }

    // ── 3. Clock tree health ──────────────────────────────────────────────
    let clock_status = crate::health::check_clock(rcc_peripheral);
    if let HealthStatus::Fail(e) = clock_status {
        info!("bist: FAIL — {:?}", e);
        return BistResult::Fail(e);
    }

    // ── 4. Fault-injector stability ───────────────────────────────────────
    // Purposely ran last: it exercises logic, and if it fails it's a firmware
    // defect, not a silicon one.
    let stability = crate::fault::stability_check();
    if !stability.ok() {
        info!("bist: FAIL — fault injector stability");
        return BistResult::FaultStability(stability);
    }

    info!("bist: PASS");
    BistResult::Pass
}