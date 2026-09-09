//! Fault-injection library: bit primitives, LFSR probability, and the three
//! protocol injectors (SPI, I2C, UART) using a compile-time type-state
//! (`Disarmed` -> `Armed`). CAN and OneWire removed by design.
//!
//! Pure logic, no hardware dependencies, unit-tested on the host.

use core::marker::PhantomData;

use crate::fault_traits::state::{Armed, Disarmed};
use crate::fault_traits::{I2cBus, SpiBus, UartBus};
use crate::{FaultConfig, FaultResult, FaultType, Protocol};

// ─── Bit manipulation primitives ──────────────────────────────────────────────

#[inline(always)]
pub fn bit_flip(byte: u8, bit: u8) -> u8 {
    byte ^ (1 << (bit & 7))
}

#[inline(always)]
pub fn bit_set(byte: u8, bit: u8) -> u8 {
    byte | (1 << (bit & 7))
}

#[inline(always)]
pub fn bit_clear(byte: u8, bit: u8) -> u8 {
    byte & !(1 << (bit & 7))
}

/// Blocking delay. On the target this is a NOP loop; on the host it is a no-op.
pub fn busy_delay_us(us: u32) {
    #[cfg(target_arch = "arm")]
    {
        for _ in 0..us.wrapping_mul(21) {
            unsafe { core::arch::asm!("nop") }
        }
    }
    #[cfg(not(target_arch = "arm"))]
    {
        let _ = us;
    }
}

// ─── LFSR for probabilistic injection ─────────────────────────────────────────

pub struct Lfsr {
    state: u16,
}

impl Lfsr {
    pub const fn new(seed: u16) -> Self {
        Self { state: if seed == 0 { 1 } else { seed } }
    }

    pub fn next_state(&mut self) -> u16 {
        let bit = ((self.state) ^ (self.state >> 2) ^ (self.state >> 3) ^ (self.state >> 5)) & 1;
        self.state = (self.state >> 1) | (bit << 15);
        self.state
    }

    pub fn next_bit(&mut self) -> u8 {
        (self.next_state() & 0x07) as u8
    }
}

#[inline(always)]
pub fn should_inject(lfsr: &mut Lfsr, permille: u16) -> bool {
    if permille == 0 {
        return false;
    }
    if permille >= 1000 {
        return true;
    }
    lfsr.next_state() & 0x03FF < permille
}

// ─── SPI ──────────────────────────────────────────────────────────────────────

pub struct SpiFaultInjector<S = Disarmed> {
    config: FaultConfig,
    lfsr:   Lfsr,
    count:  u32,
    _state: PhantomData<S>,
}

impl SpiFaultInjector<Disarmed> {
    pub fn new() -> Self {
        Self {
            config: FaultConfig::new(Protocol::Spi, FaultType::BitFlip),
            lfsr:   Lfsr::new(0xBEEF),
            count:  0,
            _state: PhantomData,
        }
    }

    pub fn configure(&mut self, config: &FaultConfig) {
        if config.protocol == Protocol::Spi {
            self.config = *config;
        }
    }

    /// Compile-time state transition: `Disarmed` -> `Armed`.
    pub fn arm(self) -> SpiFaultInjector<Armed> {
        SpiFaultInjector {
            config: self.config,
            lfsr:   self.lfsr,
            count:  0,
            _state: PhantomData,
        }
    }
}

impl Default for SpiFaultInjector<Disarmed> {
    fn default() -> Self {
        Self::new()
    }
}

impl SpiFaultInjector<Armed> {
    pub fn is_armed(&self) -> bool {
        true
    }

    pub fn injected_count(&self) -> u32 {
        self.count
    }

    pub fn reset_stats(&mut self) {
        self.count = 0;
    }

    pub fn disarm(self) -> SpiFaultInjector<Disarmed> {
        SpiFaultInjector {
            config: self.config,
            lfsr:   self.lfsr,
            count:  0,
            _state: PhantomData,
        }
    }

    pub fn inject_mosi(&mut self, byte: u8) -> u8 {
        if !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip      => bit_flip(byte, bit),
            FaultType::StuckAtZero  => bit_clear(byte, bit),
            FaultType::StuckAtOne   => bit_set(byte, bit),
            FaultType::BitDelay     => { busy_delay_us(self.config.duration_us); byte }
            FaultType::ClockGlitch  => { busy_delay_us(5); bit_flip(byte, self.lfsr.next_bit()) }
            _                       => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn inject_miso(&mut self, byte: u8) -> u8 {
        if !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip      => bit_flip(byte, bit),
            FaultType::StuckAtZero  => bit_clear(byte, bit),
            FaultType::StuckAtOne   => bit_set(byte, bit),
            _                       => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn fire(&mut self, bus: &mut SpiBus) -> FaultResult {
        bus.mosi = self.inject_mosi(bus.mosi);
        bus.miso = self.inject_miso(bus.miso);
        FaultResult::Fired
    }
}

// ─── I2C ──────────────────────────────────────────────────────────────────────

pub struct I2cFaultInjector<S = Disarmed> {
    config: FaultConfig,
    lfsr:   Lfsr,
    count:  u32,
    _state: PhantomData<S>,
}

impl I2cFaultInjector<Disarmed> {
    pub fn new() -> Self {
        Self {
            config: FaultConfig::new(Protocol::I2c, FaultType::NackInjection),
            lfsr:   Lfsr::new(0xCAFE),
            count:  0,
            _state: PhantomData,
        }
    }

    pub fn configure(&mut self, config: &FaultConfig) {
        if config.protocol == Protocol::I2c {
            self.config = *config;
        }
    }

    pub fn arm(self) -> I2cFaultInjector<Armed> {
        I2cFaultInjector {
            config: self.config,
            lfsr:   self.lfsr,
            count:  0,
            _state: PhantomData,
        }
    }
}

impl Default for I2cFaultInjector<Disarmed> {
    fn default() -> Self {
        Self::new()
    }
}

impl I2cFaultInjector<Armed> {
    pub fn is_armed(&self) -> bool {
        true
    }

    pub fn injected_count(&self) -> u32 {
        self.count
    }

    pub fn reset_stats(&mut self) {
        self.count = 0;
    }

    pub fn disarm(self) -> I2cFaultInjector<Disarmed> {
        I2cFaultInjector {
            config: self.config,
            lfsr:   self.lfsr,
            count:  0,
            _state: PhantomData,
        }
    }

    pub fn inject_address(&mut self, addr: u8) -> u8 {
        if !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return addr;
        }
        let bit = if self.config.target_bit < 7 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit() % 7
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip      => bit_flip(addr, bit),
            FaultType::StuckAtZero  => bit_clear(addr, bit),
            FaultType::StuckAtOne   => bit_set(addr, bit),
            _                       => addr,
        };
        if result != addr { self.count += 1; }
        result
    }

    pub fn inject_data(&mut self, byte: u8) -> u8 {
        if !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip      => bit_flip(byte, bit),
            FaultType::StuckAtZero  => bit_clear(byte, bit),
            FaultType::StuckAtOne   => bit_set(byte, bit),
            _                       => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn should_nack(&mut self) -> bool {
        self.config.fault_type == FaultType::NackInjection
            && should_inject(&mut self.lfsr, self.config.probability_permille)
    }

    pub fn should_lock_bus(&self) -> bool {
        self.config.fault_type == FaultType::BusLockup
    }

    pub fn fire(&mut self, bus: &mut I2cBus) -> FaultResult {
        if self.config.fault_type == FaultType::BusLockup {
            busy_delay_us(self.config.duration_us);
        }
        if self.should_nack() { self.count += 1; }
        bus.address = self.inject_address(bus.address);
        bus.data = self.inject_data(bus.data);
        FaultResult::Fired
    }
}

// ─── UART ─────────────────────────────────────────────────────────────────────

pub struct UartFaultInjector<S = Disarmed> {
    config: FaultConfig,
    lfsr:   Lfsr,
    count:  u32,
    _state: PhantomData<S>,
}

impl UartFaultInjector<Disarmed> {
    pub fn new() -> Self {
        Self {
            config: FaultConfig::new(Protocol::Uart, FaultType::BitFlip),
            lfsr:   Lfsr::new(0xDEAD),
            count:  0,
            _state: PhantomData,
        }
    }

    pub fn configure(&mut self, config: &FaultConfig) {
        if config.protocol == Protocol::Uart {
            self.config = *config;
        }
    }

    pub fn arm(self) -> UartFaultInjector<Armed> {
        UartFaultInjector {
            config: self.config,
            lfsr:   self.lfsr,
            count:  0,
            _state: PhantomData,
        }
    }
}

impl Default for UartFaultInjector<Disarmed> {
    fn default() -> Self {
        Self::new()
    }
}

impl UartFaultInjector<Armed> {
    pub fn is_armed(&self) -> bool {
        true
    }

    pub fn injected_count(&self) -> u32 {
        self.count
    }

    pub fn reset_stats(&mut self) {
        self.count = 0;
    }

    pub fn disarm(self) -> UartFaultInjector<Disarmed> {
        UartFaultInjector {
            config: self.config,
            lfsr:   self.lfsr,
            count:  0,
            _state: PhantomData,
        }
    }

    pub fn inject_tx(&mut self, byte: u8) -> u8 {
        if !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip      => bit_flip(byte, bit),
            FaultType::ParityError  => byte ^ 0x80,
            FaultType::FrameCorrupt => bit_flip(byte, self.lfsr.next_bit()),
            FaultType::BitDelay     => { busy_delay_us(self.config.duration_us); byte }
            FaultType::ClockGlitch  => { busy_delay_us(3); bit_flip(byte, self.lfsr.next_bit()) }
            _                       => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn inject_rx(&mut self, byte: u8) -> u8 {
        if !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip      => bit_flip(byte, bit),
            FaultType::StuckAtZero  => bit_clear(byte, bit),
            FaultType::StuckAtOne   => bit_set(byte, bit),
            _                       => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn inject_overrun(&mut self) -> bool {
        if self.config.fault_type == FaultType::Overrun
            && should_inject(&mut self.lfsr, self.config.probability_permille)
        {
            self.count += 1;
            true
        } else {
            false
        }
    }

    pub fn fire(&mut self, bus: &mut UartBus) -> FaultResult {
        bus.tx = self.inject_tx(bus.tx);
        bus.rx = self.inject_rx(bus.rx);
        self.inject_overrun();
        FaultResult::Fired
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // bit primitives

    #[test]
    fn bit_flip_toggles() {
        assert_eq!(bit_flip(0b1010_0000, 0), 0b1010_0001);
        assert_eq!(bit_flip(0b1010_0001, 0), 0b1010_0000);
        assert_eq!(bit_flip(0b0000_0000, 7), 0b1000_0000);
    }

    #[test]
    fn bit_set_forces_high() {
        assert_eq!(bit_set(0b0000_0000, 3), 0b0000_1000);
        assert_eq!(bit_set(0b0000_1000, 3), 0b0000_1000);
    }

    #[test]
    fn bit_clear_forces_low() {
        assert_eq!(bit_clear(0b1111_1111, 4), 0b1110_1111);
        assert_eq!(bit_clear(0b0000_0000, 4), 0b0000_0000);
    }

    #[test]
    fn bit_wraps_on_overflow() {
        assert_eq!(bit_flip(0xFF, 8), 0xFE);
        assert_eq!(bit_flip(0xFF, 16), 0xFE);
    }

    // lfsr

    #[test]
    fn lfsr_produces_values() {
        let mut lfsr = Lfsr::new(0xACE1);
        let v1 = lfsr.next_state();
        let v2 = lfsr.next_state();
        assert_ne!(v1, 0);
        assert_ne!(v2, 0);
        assert_ne!(v1, v2);
    }

    #[test]
    fn lfsr_next_bit_range() {
        let mut lfsr = Lfsr::new(42);
        for _ in 0..100 {
            assert!(lfsr.next_bit() < 8);
        }
    }

    #[test]
    fn should_inject_full_permille() {
        let mut lfsr = Lfsr::new(1);
        for _ in 0..1000 {
            assert!(should_inject(&mut lfsr, 1000));
        }
    }

    #[test]
    fn should_inject_zero_permille() {
        let mut lfsr = Lfsr::new(1);
        for _ in 0..1000 {
            assert!(!should_inject(&mut lfsr, 0));
        }
    }

    // type-state API (applies to all three injectors)

    #[test]
    fn spi_type_state_transitions() {
        let inj = SpiFaultInjector::<Disarmed>::new();
        let inj = inj.arm();
        assert!(inj.is_armed());
        let inj = inj.disarm();
        let _: SpiFaultInjector<Disarmed> = inj;
        // NOTE: a Disarmed injector has NO `fire()` method — that is the
        // whole point. The next line would NOT compile:
        // inj.fire(&mut SpiBus { sck: true, mosi: 0, miso: 0, cs: true });
    }

    // SPI

    #[test]
    fn spi_bit_flip() {
        let cfg = FaultConfig::new(Protocol::Spi, FaultType::BitFlip).at_bit(0);
        let mut inj = SpiFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert_eq!(inj.inject_mosi(0b1010_0000), 0b1010_0001);
    }

    #[test]
    fn spi_stuck_at_zero() {
        let cfg = FaultConfig::new(Protocol::Spi, FaultType::StuckAtZero).at_bit(7);
        let mut inj = SpiFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert_eq!(inj.inject_mosi(0xFF), 0x7F);
    }

    #[test]
    fn spi_stuck_at_one() {
        let cfg = FaultConfig::new(Protocol::Spi, FaultType::StuckAtOne).at_bit(3);
        let mut inj = SpiFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert_eq!(inj.inject_mosi(0x00), 0x08);
    }

    #[test]
    fn spi_fire() {
        let cfg = FaultConfig::new(Protocol::Spi, FaultType::BitFlip).at_bit(0);
        let mut inj = SpiFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        let mut bus = SpiBus { sck: true, mosi: 0xA5, miso: 0x5A, cs: true };
        assert_eq!(inj.fire(&mut bus), FaultResult::Fired);
        assert_eq!(bus.mosi, 0xA4);
        assert!(inj.is_armed());
    }

    #[test]
    fn spi_ignores_wrong_protocol_config() {
        // Config guarded by protocol check -> defaults apply (BitFlip at bit 0).
        let cfg = FaultConfig::new(Protocol::Uart, FaultType::StuckAtZero).at_bit(3);
        let mut inj = SpiFaultInjector::new();
        inj.configure(&cfg);            // wrong protocol -> ignored
        let mut inj = inj.arm();
        assert_eq!(inj.inject_mosi(0x00), 0x01);  // default BitFlip@bit0, full probability
    }

    // I2C

    #[test]
    fn i2c_bit_flip_address() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::BitFlip).at_bit(0);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert_eq!(inj.inject_address(0x50), 0x51);
    }

    #[test]
    fn i2c_stuck_at_zero_data() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::StuckAtZero).at_bit(3);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert_eq!(inj.inject_data(0xFF), 0xF7);
    }

    #[test]
    fn i2c_bus_lockup_detection() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::BusLockup);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert!(inj.should_lock_bus());
    }

    #[test]
    fn i2c_no_nack_when_bitflip() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::BitFlip);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert!(!inj.should_lock_bus());
    }

    #[test]
    fn i2c_fire() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::NackInjection);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        let mut bus = I2cBus { sda: true, scl: true, address: 0x50, data: 0xAA };
        assert_eq!(inj.fire(&mut bus), FaultResult::Fired);
        assert_eq!(inj.injected_count(), 1);
    }

    // UART

    #[test]
    fn uart_bit_flip_tx() {
        let cfg = FaultConfig::new(Protocol::Uart, FaultType::BitFlip).at_bit(0);
        let mut inj = UartFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert_eq!(inj.inject_tx(0b1010_0000), 0b1010_0001);
    }

    #[test]
    fn uart_stuck_at_zero_rx() {
        let cfg = FaultConfig::new(Protocol::Uart, FaultType::StuckAtZero).at_bit(4);
        let mut inj = UartFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert_eq!(inj.inject_rx(0xFF), 0xEF);
    }

    #[test]
    fn uart_overrun() {
        let cfg = FaultConfig::new(Protocol::Uart, FaultType::Overrun);
        let mut inj = UartFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        assert!(inj.inject_overrun());
        assert_eq!(inj.injected_count(), 1);
        let inj = inj.disarm();
        let _ = inj;
    }

    #[test]
    fn uart_fire() {
        let cfg = FaultConfig::new(Protocol::Uart, FaultType::BitFlip).at_bit(0);
        let mut inj = UartFaultInjector::new();
        inj.configure(&cfg);
        let mut inj = inj.arm();
        let mut bus = UartBus { tx: 0xAA, rx: 0x55 };
        assert_eq!(inj.fire(&mut bus), FaultResult::Fired);
        assert_eq!(bus.tx, 0xAB);
        assert_eq!(bus.rx, 0x54);
    }
}