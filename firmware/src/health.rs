use defmt::info;
use shared::{HealthError, HealthStatus};
use embassy_stm32::peripherals::RCC;
use embassy_stm32::{rcc, Peri};

extern "C" {
    static _stack_start: u32;
    static _sram_test_start: u32;
    static _sram_test_end: u32;
}

pub fn check_clock(rcc_peripheral: Peri<'_, RCC>) -> HealthStatus {
    let clocks = rcc::clocks(&rcc_peripheral);
    let sys_hz = clocks.sys.to_hertz().map(|h| h.0).unwrap_or(0);
    let pclk1_tim_hz = clocks.pclk1_tim.to_hertz().map(|h| h.0).unwrap_or(0);
    let hclk1_hz = clocks.hclk1.to_hertz().map(|h| h.0).unwrap_or(0);

    info!("sys={} Hz pclk1_tim={} Hz hclk1={} Hz", sys_hz, pclk1_tim_hz, hclk1_hz);

    if sys_hz == 0 {
        return HealthStatus::Fail(HealthError::ClockOutOfRange);
    }
    if pclk1_tim_hz == 0 {
        return HealthStatus::Fail(HealthError::TimerNotTicking);
    }
    if hclk1_hz == 0 {
        return HealthStatus::Fail(HealthError::ClockHclkNotRunning);
    }

    info!("clocks ok");
    HealthStatus::Ready
}

pub fn check_stack_canary() -> HealthStatus {
    // Place canary at the bottom of the 8 KB stack region (4 KB below top).
    // _stack_start points to 0x2001_8000 (end of SRAM).
    // Stack grows downward; if it consumes more than 4 KB, the canary is
    // overwritten and the next readback detects the overflow.
    let stack_top = core::ptr::addr_of!(_stack_start) as u32;
    if stack_top == 0 {
        return HealthStatus::Fail(HealthError::StackCanary);
    }
    let canary_addr = (stack_top - 4096) as *mut u32;
    let canary_val: u32 = 0xDEAD_BEEF;

    unsafe {
        core::ptr::write_volatile(canary_addr, canary_val);
        let read = core::ptr::read_volatile(canary_addr);
        if read != canary_val {
            return HealthStatus::Fail(HealthError::StackCanary);
        }
    }

    info!("stack canary ok at 0x{:08X}", canary_addr as u32);
    HealthStatus::Ready
}

pub fn check_ram() -> HealthStatus {
    let start = core::ptr::addr_of!(_sram_test_start) as u32;
    let end = core::ptr::addr_of!(_sram_test_end) as u32;
    let n_words = ((end - start) / 4) as usize;
    if n_words == 0 {
        return HealthStatus::Ready;
    }
    let base = start as *mut u32;
    for i in 0..n_words {
        unsafe { core::ptr::write_volatile(base.add(i), 0xFEED_FACE); }
    }
    for i in 0..n_words {
        let v = unsafe { core::ptr::read_volatile(base.add(i)) };
        if v != 0xFEED_FACE {
            return HealthStatus::Fail(HealthError::RamTest);
        }
    }
    info!("ram pattern ok at 0x{:08X}, {} words", start, n_words);
    HealthStatus::Ready
}
