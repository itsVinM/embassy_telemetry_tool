use defmt::*;
use embassy_stm32::gpio::OutputType;
use embassy_stm32::peripherals::{PA6, PA8, PA9, PA10, PA11, TIM1, TIM3};
use embassy_stm32::Peri;
use embassy_stm32::time::khz;
use embassy_stm32::timer::pwm_input::PwmInput;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_stm32::{bind_interrupts, peripherals, timer};
use embassy_time::Timer;

// INTERRUPT BINDING 
bind_interrupts!(struct Irqs {
    TIM3 => timer::CaptureCompareInterruptHandler<peripherals::TIM3>;
});

// DATA STRUCTURE
const CLOCK_SPEED_KHZ: u32 = 10;

// PWM OUTPUT TASK - All 4 TIM1 channels simultaneously
/// Generates 4 independent PWM signals:
/// - PA8 (Ch1): 25% duty cycle
/// - PA9 (Ch2): 50% duty cycle
/// - PA10 (Ch3): 75% duty cycle
/// - PA11 (Ch4): 10% duty cycle
#[embassy_executor::task]
pub async fn pwm_task(
    tim1: Peri<'static, TIM1>,
    pa8: Peri<'static, PA8>,
    pa9: Peri<'static, PA9>,
    pa10: Peri<'static, PA10>,
    pa11: Peri<'static, PA11>,
) -> ! {
    // Create PWM pins for all 4 channels
    let ch1_pin = PwmPin::new(pa8, OutputType::PushPull);
    let ch2_pin = PwmPin::new(pa9, OutputType::PushPull);
    let ch3_pin = PwmPin::new(pa10, OutputType::PushPull);
    let ch4_pin = PwmPin::new(pa11, OutputType::PushPull);

    // Initialize SimplePwm with all 4 channels
    let pwm = SimplePwm::new(
        tim1,
        Some(ch1_pin),  // Ch1 - PA8
        Some(ch2_pin),  // Ch2 - PA9
        Some(ch3_pin),  // Ch3 - PA10
        Some(ch4_pin),  // Ch4 - PA11
        khz(CLOCK_SPEED_KHZ),
        Default::default(),
    );

    // Get handles to each channel
    let mut channels = pwm.split();

    // Enable all channels
    channels.ch1.enable();
    channels.ch2.enable();
    channels.ch3.enable();
    channels.ch4.enable();

    channels.ch1.set_duty_cycle_fraction(1, 4);  // 25%
    channels.ch2.set_duty_cycle_fraction(1, 2);  // 50%
    channels.ch3.set_duty_cycle_fraction(3, 4);  // 75%
    channels.ch4.set_duty_cycle_fraction(1, 10); // 10%

    info!("pwm: PA8=25% PA9=50% PA10=75% PA11=10% {} kHz", CLOCK_SPEED_KHZ);

    loop {
        Timer::after_millis(5000).await;
    }
}

// INPUT CAPTURE TASK - Measures signal on PA6 (TIM3 Channel 1)
/// Measures frequency and duty cycle of signal connected to PA6
/// Connect any of the PWM outputs (PA8, PA9, PA10, PA11) to PA6 to measure
#[embassy_executor::task]
pub async fn capture_task(
    tim3: Peri<'static, TIM3>,
    pa6: Peri<'static, PA6>,
) -> ! {
    // Create PWM input on TIM3, channel 1, listening on PA6
    let mut pwm_input = PwmInput::new_ch1(
        tim3,
        pa6,
        Irqs,
        embassy_stm32::gpio::Pull::None,
        khz(CLOCK_SPEED_KHZ),
    );

    pwm_input.enable();

    info!("capture: PA6 (TIM3 Ch1) measuring input — connect PWM out → PA6");

    loop {
        // Wait between measurements
        Timer::after_millis(500).await;

        // Read measurements
        let period_ticks = pwm_input.get_period_ticks();
        let width_ticks = pwm_input.get_width_ticks();
        let duty_cycle_pct = pwm_input.get_duty_cycle();

        info!(
            "  Period: {} ticks | Width: {} ticks | Duty: {}%",
            period_ticks, width_ticks, duty_cycle_pct
        );
    }
}