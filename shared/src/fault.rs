//! Fault-injection library: bit primitives, LFSR probability, and one
//! concrete injector per protocol (SPI, I2C, UART, CAN, OneWire).
//! Pure logic, no hardware dependencies, fully unit-tested on the host.

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
        let cycles = us.wrapping_mul(21);
        for _ in 0..cycles {
            unsafe { core::arch::asm!("nop") }
        }
    }
    #[cfg(not(target_arch = "arm"))]
    {
        let _ = us;
    }
}

// ─── LFSR for probabilistic injection ────────────────────────────────────────

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
    let r = lfsr.next_state() & 0x03FF;
    r < permille
}

// ─── CAN ──────────────────────────────────────────────────────────────────────

pub struct CanBus {
    pub id: u32,
    pub data: [u8; 8],
    pub dlc: u8,
}

pub struct CanFaultInjector {
    config: FaultConfig,
    lfsr: Lfsr,
    armed: bool,
    count: u32,
}

impl CanFaultInjector {
    pub fn new() -> Self {
        Self {
            config: FaultConfig::new(Protocol::Can, FaultType::BitFlip),
            lfsr: Lfsr::new(0x1234),
            armed: false,
            count: 0,
        }
    }

    pub fn configure(&mut self, config: &FaultConfig) {
        self.config = *config;
    }

    pub fn arm(&mut self) {
        self.armed = true;
        self.count = 0;
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn injected_count(&self) -> u32 {
        self.count
    }

    pub fn reset_stats(&mut self) {
        self.count = 0;
    }

    pub fn inject_id(&mut self, id: u32) -> u32 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return id;
        }
        let bit = if self.config.target_bit < 29 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit() % 29
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip => id ^ (1 << bit),
            FaultType::StuckAtZero => id & !(1 << bit),
            FaultType::StuckAtOne => id | (1 << bit),
            _ => id,
        };
        if result != id { self.count += 1; }
        result
    }

    pub fn inject_data(&mut self, byte: u8, index: usize) -> u8 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let result = match self.config.fault_type {
            FaultType::BitFlip => {
                let bit = if self.config.target_bit < 8 { self.config.target_bit } else { self.lfsr.next_bit() };
                bit_flip(byte, bit)
            }
            FaultType::CrcCorrupt if index >= 4 => {
                bit_flip(byte, self.lfsr.next_bit())
            }
            FaultType::FrameCorrupt => {
                bit_flip(byte, self.lfsr.next_bit())
            }
            _ => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn fire(&mut self, bus: &mut CanBus) -> FaultResult {
        if !self.armed {
            return FaultResult::Error;
        }
        bus.id = self.inject_id(bus.id);
        for (i, byte) in bus.data.iter_mut().enumerate() {
            *byte = self.inject_data(*byte, i);
        }
        FaultResult::Fired
    }
}

impl Default for CanFaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ─── SPI ──────────────────────────────────────────────────────────────────────

pub struct SpiBus {
    pub sck: bool,
    pub mosi: u8,
    pub miso: u8,
    pub cs: bool,
}

pub struct SpiFaultInjector {
    config: FaultConfig,
    lfsr: Lfsr,
    armed: bool,
    count: u32,
}

impl SpiFaultInjector {
    pub fn new() -> Self {
        Self {
            config: FaultConfig::new(Protocol::Spi, FaultType::BitFlip),
            lfsr: Lfsr::new(0xBEEF),
            armed: false,
            count: 0,
        }
    }

    pub fn configure(&mut self, config: &FaultConfig) {
        self.config = *config;
    }

    pub fn arm(&mut self) {
        self.armed = true;
        self.count = 0;
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn injected_count(&self) -> u32 {
        self.count
    }

    pub fn reset_stats(&mut self) {
        self.count = 0;
    }

    pub fn inject_mosi(&mut self, byte: u8) -> u8 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip => bit_flip(byte, bit),
            FaultType::StuckAtZero => bit_clear(byte, bit),
            FaultType::StuckAtOne => bit_set(byte, bit),
            FaultType::BitDelay => { busy_delay_us(self.config.duration_us); byte }
            FaultType::ClockGlitch => { busy_delay_us(5); bit_flip(byte, self.lfsr.next_bit()) }
            _ => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn inject_miso(&mut self, byte: u8) -> u8 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip => bit_flip(byte, bit),
            FaultType::StuckAtZero => bit_clear(byte, bit),
            FaultType::StuckAtOne => bit_set(byte, bit),
            _ => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn fire(&mut self, bus: &mut SpiBus) -> FaultResult {
        if !self.armed {
            return FaultResult::Error;
        }
        bus.mosi = self.inject_mosi(bus.mosi);
        bus.miso = self.inject_miso(bus.miso);
        FaultResult::Fired
    }
}

impl Default for SpiFaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ─── I2C ──────────────────────────────────────────────────────────────────────

pub struct I2cBus {
    pub sda: bool,
    pub scl: bool,
    pub address: u8,
    pub data: u8,
}

pub struct I2cFaultInjector {
    config: FaultConfig,
    lfsr: Lfsr,
    armed: bool,
    count: u32,
}

impl I2cFaultInjector {
    pub fn new() -> Self {
        Self {
            config: FaultConfig::new(Protocol::I2c, FaultType::NackInjection),
            lfsr: Lfsr::new(0xCAFE),
            armed: false,
            count: 0,
        }
    }

    pub fn configure(&mut self, config: &FaultConfig) {
        self.config = *config;
    }

    pub fn arm(&mut self) {
        self.armed = true;
        self.count = 0;
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn injected_count(&self) -> u32 {
        self.count
    }

    pub fn reset_stats(&mut self) {
        self.count = 0;
    }

    pub fn inject_address(&mut self, addr: u8) -> u8 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return addr;
        }
        let bit = if self.config.target_bit < 7 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit() % 7
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip => bit_flip(addr, bit),
            FaultType::StuckAtZero => bit_clear(addr, bit),
            FaultType::StuckAtOne => bit_set(addr, bit),
            _ => addr,
        };
        if result != addr { self.count += 1; }
        result
    }

    pub fn inject_data(&mut self, byte: u8) -> u8 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip => bit_flip(byte, bit),
            FaultType::StuckAtZero => bit_clear(byte, bit),
            FaultType::StuckAtOne => bit_set(byte, bit),
            _ => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn should_nack(&self) -> bool {
        self.armed && self.config.fault_type == FaultType::NackInjection
            && should_inject(&mut Lfsr::new(0), self.config.probability_permille)
    }

    pub fn should_lock_bus(&self) -> bool {
        self.armed && self.config.fault_type == FaultType::BusLockup
    }

    pub fn inject_bus_lockup(&self) {
        if self.should_lock_bus() {
            busy_delay_us(self.config.duration_us);
        }
    }

    pub fn fire(&mut self, bus: &mut I2cBus) -> FaultResult {
        if !self.armed {
            return FaultResult::Error;
        }
        if self.config.fault_type == FaultType::BusLockup {
            self.inject_bus_lockup();
        }
        if self.should_nack() {
            self.count += 1;
        }
        bus.address = self.inject_address(bus.address);
        bus.data = self.inject_data(bus.data);
        FaultResult::Fired
    }
}

impl Default for I2cFaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ─── UART ─────────────────────────────────────────────────────────────────────

pub struct UartBus {
    pub tx: u8,
    pub rx: u8,
}

pub struct UartFaultInjector {
    config: FaultConfig,
    lfsr: Lfsr,
    armed: bool,
    count: u32,
}

impl UartFaultInjector {
    pub fn new() -> Self {
        Self {
            config: FaultConfig::new(Protocol::Uart, FaultType::BitFlip),
            lfsr: Lfsr::new(0xDEAD),
            armed: false,
            count: 0,
        }
    }

    pub fn configure(&mut self, config: &FaultConfig) {
        self.config = *config;
    }

    pub fn arm(&mut self) {
        self.armed = true;
        self.count = 0;
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn injected_count(&self) -> u32 {
        self.count
    }

    pub fn reset_stats(&mut self) {
        self.count = 0;
    }

    pub fn inject_tx(&mut self, byte: u8) -> u8 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip => bit_flip(byte, bit),
            FaultType::ParityError => byte ^ 0x80,
            FaultType::FrameCorrupt => bit_flip(byte, self.lfsr.next_bit()),
            FaultType::BitDelay => { busy_delay_us(self.config.duration_us); byte }
            FaultType::ClockGlitch => { busy_delay_us(3); bit_flip(byte, self.lfsr.next_bit()) }
            _ => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn inject_rx(&mut self, byte: u8) -> u8 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip => bit_flip(byte, bit),
            FaultType::StuckAtZero => bit_clear(byte, bit),
            FaultType::StuckAtOne => bit_set(byte, bit),
            _ => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn inject_overrun(&mut self) -> bool {
        if self.armed && self.config.fault_type == FaultType::Overrun
            && should_inject(&mut self.lfsr, self.config.probability_permille)
        {
            self.count += 1;
            true
        } else {
            false
        }
    }

    pub fn fire(&mut self, bus: &mut UartBus) -> FaultResult {
        if !self.armed {
            return FaultResult::Error;
        }
        bus.tx = self.inject_tx(bus.tx);
        bus.rx = self.inject_rx(bus.rx);
        self.inject_overrun();
        FaultResult::Fired
    }
}

impl Default for UartFaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ─── OneWire ──────────────────────────────────────────────────────────────────

pub struct OneWireBus {
    pub line: bool,
    pub rom_cmd: u8,
    pub scratchpad: [u8; 9],
}

pub struct OneWireFaultInjector {
    config: FaultConfig,
    lfsr: Lfsr,
    armed: bool,
    count: u32,
}

impl OneWireFaultInjector {
    pub fn new() -> Self {
        Self {
            config: FaultConfig::new(Protocol::OneWire, FaultType::BitFlip),
            lfsr: Lfsr::new(0x5678),
            armed: false,
            count: 0,
        }
    }

    pub fn configure(&mut self, config: &FaultConfig) {
        self.config = *config;
    }

    pub fn arm(&mut self) {
        self.armed = true;
        self.count = 0;
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn injected_count(&self) -> u32 {
        self.count
    }

    pub fn reset_stats(&mut self) {
        self.count = 0;
    }

    pub fn suppress_presence(&self) -> bool {
        self.armed && self.config.fault_type == FaultType::Timeout
            && should_inject(&mut Lfsr::new(0), self.config.probability_permille)
    }

    pub fn inject_data(&mut self, byte: u8, index: usize) -> u8 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return byte;
        }
        let bit = if self.config.target_bit < 8 {
            self.config.target_bit
        } else {
            self.lfsr.next_bit()
        };
        let result = match self.config.fault_type {
            FaultType::BitFlip => bit_flip(byte, bit),
            FaultType::StuckAtZero => bit_clear(byte, bit),
            FaultType::StuckAtOne => bit_set(byte, bit),
            FaultType::CrcCorrupt if index >= 8 => bit_flip(byte, self.lfsr.next_bit()),
            _ => byte,
        };
        if result != byte { self.count += 1; }
        result
    }

    pub fn inject_rom_command(&mut self, cmd: u8) -> u8 {
        if !self.armed || !should_inject(&mut self.lfsr, self.config.probability_permille) {
            return cmd;
        }
        match self.config.fault_type {
            FaultType::BitFlip => {
                let bit = if self.config.target_bit < 8 {
                    self.config.target_bit
                } else {
                    self.lfsr.next_bit()
                };
                let r = bit_flip(cmd, bit);
                if r != cmd { self.count += 1; }
                r
            }
            _ => cmd,
        }
    }

    pub fn inject_timing_violation(&self) {
        if self.armed && self.config.fault_type == FaultType::BitDelay
            && should_inject(&mut Lfsr::new(0), self.config.probability_permille)
        {
            busy_delay_us(self.config.duration_us);
        }
    }

    pub fn glitch_reset_pulse(&self) -> bool {
        self.armed && self.config.fault_type == FaultType::ClockGlitch
            && should_inject(&mut Lfsr::new(0), self.config.probability_permille)
    }

    pub fn fire(&mut self, bus: &mut OneWireBus) -> FaultResult {
        if !self.armed {
            return FaultResult::Error;
        }
        if self.suppress_presence() {
            busy_delay_us(self.config.duration_us);
        }
        if self.glitch_reset_pulse() {
            bus.line = false;
        }
        self.inject_timing_violation();
        bus.rom_cmd = self.inject_rom_command(bus.rom_cmd);
        for (i, byte) in bus.scratchpad.iter_mut().enumerate() {
            *byte = self.inject_data(*byte, i);
        }
        FaultResult::Fired
    }
}

impl Default for OneWireFaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn can_bit_flip_id() {
        let cfg = FaultConfig::new(Protocol::Can, FaultType::BitFlip).at_bit(0);
        let mut inj = CanFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_id(0x0000_0000), 0x0000_0001);
    }

    #[test]
    fn can_stuck_at_zero() {
        let cfg = FaultConfig::new(Protocol::Can, FaultType::StuckAtZero).at_bit(3);
        let mut inj = CanFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_id(0xFFFF_FFFF), 0xFFFF_FFF7);
    }

    #[test]
    fn can_crc_corrupt() {
        let cfg = FaultConfig::new(Protocol::Can, FaultType::CrcCorrupt);
        let mut inj = CanFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        let d0 = inj.inject_data(0xFF, 0);
        let d4 = inj.inject_data(0xFF, 4);
        assert_eq!(d0, 0xFF);
        assert_ne!(d4, 0xFF);
    }

    #[test]
    fn can_fire() {
        let cfg = FaultConfig::new(Protocol::Can, FaultType::BitFlip).at_bit(0);
        let mut inj = CanFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        let mut bus = CanBus { id: 0, data: [0; 8], dlc: 8 };
        assert_eq!(inj.fire(&mut bus), FaultResult::Fired);
        assert_eq!(bus.id, 0x0000_0001);
    }

    #[test]
    fn can_fire_when_disarmed() {
        let mut inj = CanFaultInjector::new();
        let mut bus = CanBus { id: 0, data: [0; 8], dlc: 8 };
        assert_eq!(inj.fire(&mut bus), FaultResult::Error);
    }

    #[test]
    fn spi_bit_flip() {
        let cfg = FaultConfig::new(Protocol::Spi, FaultType::BitFlip).at_bit(0);
        let mut inj = SpiFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_mosi(0b1010_0000), 0b1010_0001);
    }

    #[test]
    fn spi_stuck_at_zero() {
        let cfg = FaultConfig::new(Protocol::Spi, FaultType::StuckAtZero).at_bit(7);
        let mut inj = SpiFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_mosi(0xFF), 0x7F);
    }

    #[test]
    fn spi_stuck_at_one() {
        let cfg = FaultConfig::new(Protocol::Spi, FaultType::StuckAtOne).at_bit(3);
        let mut inj = SpiFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_mosi(0x00), 0x08);
    }

    #[test]
    fn spi_no_inject_when_disarmed() {
        let mut inj = SpiFaultInjector::new();
        assert_eq!(inj.inject_mosi(0xAA), 0xAA);
        assert_eq!(inj.injected_count(), 0);
    }

    #[test]
    fn spi_fire() {
        let cfg = FaultConfig::new(Protocol::Spi, FaultType::BitFlip).at_bit(0);
        let mut inj = SpiFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        let mut bus = SpiBus { sck: true, mosi: 0xA5, miso: 0x5A, cs: true };
        assert_eq!(inj.fire(&mut bus), FaultResult::Fired);
        assert_eq!(bus.mosi, 0xA4);
        assert!(inj.is_armed());
    }

    #[test]
    fn i2c_bit_flip_address() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::BitFlip).at_bit(0);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_address(0x50), 0x51);
    }

    #[test]
    fn i2c_stuck_at_zero_data() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::StuckAtZero).at_bit(3);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_data(0xFF), 0xF7);
    }

    #[test]
    fn i2c_bus_lockup_detection() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::BusLockup);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert!(inj.should_lock_bus());
    }

    #[test]
    fn i2c_no_nack_when_bitflip() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::BitFlip);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert!(!inj.should_lock_bus());
    }

    #[test]
    fn i2c_fire() {
        let cfg = FaultConfig::new(Protocol::I2c, FaultType::NackInjection);
        let mut inj = I2cFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        let mut bus = I2cBus { sda: true, scl: true, address: 0x50, data: 0xAA };
        assert_eq!(inj.fire(&mut bus), FaultResult::Fired);
        assert_eq!(inj.injected_count(), 1);
    }

    #[test]
    fn uart_bit_flip_tx() {
        let cfg = FaultConfig::new(Protocol::Uart, FaultType::BitFlip).at_bit(0);
        let mut inj = UartFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_tx(0b1010_0000), 0b1010_0001);
    }

    #[test]
    fn uart_stuck_at_zero_rx() {
        let cfg = FaultConfig::new(Protocol::Uart, FaultType::StuckAtZero).at_bit(4);
        let mut inj = UartFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_rx(0xFF), 0xEF);
    }

    #[test]
    fn uart_overrun() {
        let cfg = FaultConfig::new(Protocol::Uart, FaultType::Overrun);
        let mut inj = UartFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert!(inj.inject_overrun());
        assert_eq!(inj.injected_count(), 1);
        inj.disarm();
        assert!(!inj.inject_overrun());
    }

    #[test]
    fn uart_fire() {
        let cfg = FaultConfig::new(Protocol::Uart, FaultType::BitFlip).at_bit(0);
        let mut inj = UartFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        let mut bus = UartBus { tx: 0xAA, rx: 0x55 };
        assert_eq!(inj.fire(&mut bus), FaultResult::Fired);
        assert_eq!(bus.tx, 0xAB);
        assert_eq!(bus.rx, 0x54);
    }

    #[test]
    fn onewire_bit_flip() {
        let cfg = FaultConfig::new(Protocol::OneWire, FaultType::BitFlip).at_bit(0);
        let mut inj = OneWireFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_data(0b1111_1110, 0), 0b1111_1111);
    }

    #[test]
    fn onewire_crc_corrupt_after_index8() {
        let cfg = FaultConfig::new(Protocol::OneWire, FaultType::CrcCorrupt);
        let mut inj = OneWireFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_data(0xFF, 0), 0xFF);
        assert_ne!(inj.inject_data(0xFF, 10), 0xFF);
    }

    #[test]
    fn onewire_presence_suppressed() {
        let cfg = FaultConfig::new(Protocol::OneWire, FaultType::Timeout);
        let mut inj = OneWireFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert!(inj.suppress_presence());
    }

    #[test]
    fn onewire_rom_cmd_flip() {
        let cfg = FaultConfig::new(Protocol::OneWire, FaultType::BitFlip).at_bit(0);
        let mut inj = OneWireFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        assert_eq!(inj.inject_rom_command(0xCC), 0xCD);
    }

    #[test]
    fn onewire_fire() {
        let cfg = FaultConfig::new(Protocol::OneWire, FaultType::BitFlip).at_bit(0);
        let mut inj = OneWireFaultInjector::new();
        inj.configure(&cfg);
        inj.arm();
        let mut bus = OneWireBus { line: true, rom_cmd: 0xCC, scratchpad: [0xFF; 9] };
        assert_eq!(inj.fire(&mut bus), FaultResult::Fired);
        assert_eq!(bus.rom_cmd, 0xCD);
        assert_eq!(bus.scratchpad[0], 0xFE);
    }
}
