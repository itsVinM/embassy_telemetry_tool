#![no_std]
#![no_main]

use defmt::{info, warn, error};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::Config;
use panic_probe as _;

mod mpu;
mod canary;
mod crypto;
mod fault;
mod sco;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("=== embassy-telemetry-tool starting ===");
    info!("Target: STM32F401RE (Nucleo-F401RE)");
    info!("Features: MPU + Stack Canary + AES + Fault Injection + SCO");

    // === 1. MEMORY PROTECTION UNIT (MPU) ===
    // Configures memory regions: Flash=RO+X, SRAM=RW+XN, Peripherals=Device, Stack Guard=NoAccess
    mpu::init();
    info!("MPU configured: 4 regions (Flash, SRAM, Peripherals, Stack Guard)");

    // === 2. CLOCK CONFIGURATION ===
    let mut config = Config::default();
    config.rcc.pll = Some(embassy_stm32::rcc::Pll {
        prediv: embassy_stm32::rcc::PllPreDiv::Div8,
        mul: embassy_stm32::rcc::PllMul::Mul168,
        divp: Some(embassy_stm32::rcc::PllPDiv::Div4),  // 84 MHz
        divq: None, divr: None,
    });
    config.rcc.pll_src = embassy_stm32::rcc::PllSource::Hsi;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::Pll1P;
    config.rcc.apb1_pre = embassy_stm32::rcc::APBPrescaler::Div2;
    config.rcc.apb2_pre = embassy_stm32::rcc::APBPrescaler::Div1;

    let p = embassy_stm32::init(config);

    // Flash ART accelerator: 5 wait states for 84 MHz @ 3.3V (RM0368 §3.5.1)
    unsafe {
        let flash_acr = 0x4002_3C00 as *mut u32;
        core::ptr::write_volatile(flash_acr, (5 << 0) | (1 << 8) | (1 << 9) | (1 << 10));
    }

    // === 3. STACK CANARY INIT ===
    // Places 0xDEADBEEF at end of RAM, checked periodically
    canary::init();
    info!("Stack canary initialized at 0x2001_7FFC");

    // === 4. HEALTH CHECKS ===
    if !canary::check() {
        error!("FAILED: Stack canary corrupted at boot!");
        loop {}
    }
    info!("Health checks PASSED");

    // === 5. INIT SUBSYSTEMS ===
    // UART2 for command interface (PA2=TX, PA3=RX)
    let mut uart = embassy_stm32::usart::Uart::new_blocking(
        p.USART2, p.PA2, p.PA3, Default::default()
    ).unwrap();
    info!("UART2 initialized for command interface");

    // ADC1 for power analysis (PA0 = ADC1_IN0)
    let mut adc = embassy_stm32::adc::Adc::new(p.ADC1, Default::default());
    let mut adc_pin = p.PA0;
    info!("ADC1 initialized on PA0 for power capture");

    // TRNG for entropy
    let mut trng = embassy_stm32::rng::Rng::new(p.RNG);
    info!("TRNG initialized");

    // Fault injection GPIO (PB0 = crowbar trigger)
    let mut fault_pin = embassy_stm32::gpio::Output::new(p.PB0, embassy_stm32::gpio::Level::Low, embassy_stm32::gpio::Speed::VeryHigh);
    info!("Fault injection pin PB0 ready");

    info!("=== All subsystems ready ===");
    info!("Commands: aes, fault, sco, trng, canary, help");

    // === 6. COMMAND LOOP ===
    let mut buf = [0u8; 64];
    loop {
        // Non-blocking read with timeout
        match uart.blocking_read(&mut buf) {
            Ok(n) if n > 0 => {
                let cmd = core::str::from_utf8(&buf[..n]).unwrap_or("").trim();
                handle_command(cmd, &mut uart, &mut adc, &mut adc_pin, &mut trng, &mut fault_pin).await;
            }
            _ => {}
        }
        embassy_time::Timer::after_millis(10).await;
    }
}

async fn handle_command(
    cmd: &str,
    uart: &mut embassy_stm32::usart::Uart<'_, embassy_stm32::mode::Blocking>,
    adc: &mut embassy_stm32::adc::Adc<'_, embassy_stm32::peripherals::ADC1>,
    adc_pin: &mut embassy_stm32::peripherals::PA0,
    trng: &mut embassy_stm32::rng::Rng,
    fault_pin: &mut embassy_stm32::gpio::Output<'_>,
) {
    let parts: heapless::Vec<&str, 4> = cmd.split_whitespace().collect();
    if parts.is_empty() { return; }

    match parts[0] {
        "help" => print_help(uart),
        "aes" => crypto::aes_demo(parts.get(1), uart).await,
        "fault" => fault::demo(parts.get(1), fault_pin, uart).await,
        "sco" => sco::capture(parts.get(1), adc, adc_pin, uart).await,
        "trng" => trng_demo(parts.get(1), trng, uart).await,
        "canary" => canary_cmd(parts.get(1), uart),
        _ => { let _ = uart.blocking_write(b"Unknown command. Type 'help'\r\n"); }
    }
}

fn print_help(uart: &mut embassy_stm32::usart::Uart<'_, embassy_stm32::mode::Blocking>) {
    let help = b"\
Commands:\r\n\
  help              - Show this help\r\n\
  aes [enc|dec]     - AES-128 encrypt/decrypt demo\r\n\
  fault [vglitch]   - Voltage glitch demo (needs hardware)\r\n\
  sco [capture]     - Power trace capture (ADC)\r\n\
  trng [health]     - TRNG demo + health checks\r\n\
  canary [check]    - Stack canary status\r\n\
";
    let _ = uart.blocking_write(help);
}

fn canary_cmd(arg: Option<&str>, uart: &mut embassy_stm32::usart::Uart<'_, embassy_stm32::mode::Blocking>) {
    match arg {
        Some("check") => {
            let ok = canary::check();
            let msg = if ok { "Stack canary: OK\r\n" } else { "Stack canary: CORRUPTED!\r\n" };
            let _ = uart.blocking_write(msg.as_bytes());
        }
        _ => {
            let addr = canary::address();
            let val = canary::read_raw();
            let msg = heapless::String::<64>::from("Canary at: 0x").unwrap();
            // Note: can't easily format hex in no_std without more deps
            let _ = uart.blocking_write(b"Canary at end of RAM (0x2001_7FFC)\r\n");
        }
    }
}

async fn trng_demo(arg: Option<&str>, trng: &mut embassy_stm32::rng::Rng, uart: &mut embassy_stm32::usart::Uart<'_, embassy_stm32::mode::Blocking>) {
    let mut buf = [0u8; 32];
    trng.fill_bytes(&mut buf).ok();
    
    let _ = uart.blocking_write(b"TRNG 32 bytes: ");
    // Simple hex output
    for b in buf {
        let _ = uart.blocking_write(&[(b >> 4) + if (b >> 4) < 10 { b'0' } else { b'A' - 10 }]);
        let _ = uart.blocking_write(&[(b & 0xF) + if (b & 0xF) < 10 { b'0' } else { b'A' - 10 }]);
    }
    let _ = uart.blocking_write(b"\r\n");

    if arg == Some("health") {
        let mut samples = [0u8; 1000];
        trng.fill_bytes(&mut samples).ok();
        let ones: usize = samples.iter().map(|b| b.count_ones() as usize).sum();
        let prop = ones as f32 / 8000.0;
        let passes = (prop - 0.5).abs() < 0.02;
        let _ = uart.blocking_write(if passes { b"Monobit test: PASS\r\n" } else { b"Monobit test: FAIL\r\n" });
    }
}