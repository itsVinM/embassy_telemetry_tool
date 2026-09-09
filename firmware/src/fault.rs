// Fault Injection Module - Voltage/Clock Glitching
// Purpose: Learn fault injection concepts for security validation (FIPS 140-3, ISO 19790)

use embassy_stm32::gpio::{Output, Level, Speed};
use embassy_stm32::pac::RCC;
use embassy_time::{Duration, Timer};

/// Voltage Glitch (Crowbar) - Pulls VDD low via external MOSFET
/// Hardware: PB0 -> MOSFET Gate -> VDD rail
///           MOSFET Source -> GND, Drain -> VDD with decoupling cap
pub struct VoltageGlitch {
    pin: Output<'static>,
    pulse_width_ns: u32,
    delay_ns: u32,
}

impl VoltageGlitch {
    pub fn new(pin: Output<'static>) -> Self {
        Self { pin, pulse_width_ns: 50, delay_ns: 0 }
    }

    pub fn set_pulse_width(&mut self, ns: u32) { self.pulse_width_ns = ns; }
    pub fn set_delay(&mut self, ns: u32) { self.delay_ns = ns; }

    /// Fire: Enable crowbar for pulse_width_ns nanoseconds
    pub async fn fire(&mut self) {
        if self.delay_ns > 0 {
            Timer::after(Duration::from_nanos(self.delay_ns as u64)).await;
        }
        self.pin.set_high();  // Turn on MOSFET -> VDD drops
        Timer::after(Duration::from_nanos(self.pulse_width_ns as u64)).await;
        self.pin.set_low();   // Turn off MOSFET -> VDD recovers
    }
}

/// Clock Glitch - Manipulates PLL to cause timing violations
/// DANGEROUS: Can crash MCU, requires careful recovery
pub struct ClockGlitch {
    glitch_duration_cycles: u32,
}

impl ClockGlitch {
    pub fn new() -> Self { Self { glitch_duration_cycles: 100 } }
    pub fn set_duration(&mut self, cycles: u32) { self.glitch_duration_cycles = cycles; }

    /// Inject clock glitch by temporarily changing PLL multiplier
    pub async fn fire(&mut self) {
        // SAVE original PLL config
        let original_mul = unsafe { (*RCC::ptr()).pllcfgr.read().pllm().bits() };
        let original_divp = unsafe { (*RCC::ptr()).pllcfgr.read().pllp().bits() };
        
        // APPLY glitch: extreme PLL settings
        unsafe {
            (*RCC::ptr()).pllcfgr.modify(|_, w| {
                w.pllm().bits(168);  // Max multiplier
                w.pllp().bits(0);    // Div by 2 = highest freq
            });
        }
        
        // WAIT for glitch duration
        Timer::after(Duration::from_nanos(self.glitch_duration_cycles as u64 * 12)).await;
        
        // RESTORE original PLL config
        unsafe {
            (*RCC::ptr()).pllcfgr.modify(|_, w| {
                w.pllm().bits(original_mul);
                w.pllp().bits(match original_divp { 0 => 0, 1 => 1, 2 => 2, 3 => 3, _ => 1 })
            });
        }
    }
}

/// EM Fault Injection Trigger - Output pulse for external EM pulser (ChipSHOUTER, etc.)
pub struct EmFiTrigger {
    pin: Output<'static>,
    pulse_width_ns: u32,
}

impl EmFiTrigger {
    pub fn new(pin: Output<'static>) -> Self {
        Self { pin, pulse_width_ns: 100 }
    }
    pub fn set_width(&mut self, ns: u32) { self.pulse_width_ns = ns; }
    
    pub async fn fire(&mut self) {
        self.pin.set_high();
        Timer::after(Duration::from_nanos(self.pulse_width_ns as u64)).await;
        self.pin.set_low();
    }
}

/// Demo command handler
pub async fn demo(
    arg: Option<&str>,
    mut fault_pin: Output<'_>,
    uart: &mut embassy_stm32::usart::Uart<'_, embassy_stm32::mode::Blocking>
) {
    let mut vglitch = VoltageGlitch::new(fault_pin);
    
    match arg {
        Some("vglitch") => {
            let _ = uart.blocking_write(b"Voltage glitch: 50ns pulse...\r\n");
            vglitch.set_pulse_width(50);
            vglitch.fire().await;
            let _ = uart.blocking_write(b"Done\r\n");
        }
        Some("vglitch_long") => {
            let _ = uart.blocking_write(b"Voltage glitch: 200ns pulse...\r\n");
            vglitch.set_pulse_width(200);
            vglitch.fire().await;
            let _ = uart.blocking_write(b"Done\r\n");
        }
        Some("delayed") => {
            let _ = uart.blocking_write(b"Delayed glitch: 1us delay + 50ns pulse\r\n");
            vglitch.set_delay(1000);
            vglitch.set_pulse_width(50);
            vglitch.fire().await;
            let _ = uart.blocking_write(b"Done\r\n");
        }
        _ => {
            let _ = uart.blocking_write(b"Fault: vglitch | vglitch_long | delayed\r\n");
        }
    }
}