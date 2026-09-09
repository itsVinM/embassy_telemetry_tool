//! GAT traits + type-state markers for the fault-injection subsystem.
//!
//! Scope: SPI, I2C, UART only (CAN and OneWire intentionally removed).
//!
//! GAT (Generic Associated Type): `ProtocolBus::Frame<'a>` is an associated
//! type generic over a lifetime — the returned snapshot borrows from the bus
//! that produced it, and can never outlive it.

use crate::Protocol;

/// Compile-time state markers for type-state fault injection.
///
/// An injector generic over `S` exposes different methods depending on
/// whether `S = Disarmed` or `S = Armed`.
pub mod state {
    #[derive(Debug)]
    pub struct Disarmed;

    #[derive(Debug)]
    pub struct Armed;
}

/// A protocol bus that can expose a borrowed, read-only "frame" snapshot.
///
/// The GAT `Frame<'a>` ties the lifetime of the snapshot to the borrow of the
/// bus — the compiler guarantees the snapshot cannot outlive the bus object it
/// was derived from.
pub trait ProtocolBus {
    const PROTOCOL: Protocol;

    /// A borrowed snapshot of the current bus transaction (GAT).
    type Frame<'a>
    where
        Self: 'a;

    /// Borrow a snapshot of the current bus state for logging/inspection.
    fn frame<'a>(&'a self) -> Self::Frame<'a>;

    /// Sanity check on the bus state (e.g. CS active, clock healthy).
    fn validate(&self) -> bool;
}

// ─── SPI ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct SpiBus {
    pub sck:  bool,
    pub mosi: u8,
    pub miso: u8,
    pub cs:   bool,
}

#[derive(Debug)]
pub struct SpiFrame<'a> {
    pub sck:  bool,
    pub mosi: &'a u8,
    pub miso: &'a u8,
    pub cs:   bool,
}

impl ProtocolBus for SpiBus {
    const PROTOCOL: Protocol = Protocol::Spi;

    type Frame<'a> = SpiFrame<'a>;

    fn frame<'a>(&'a self) -> Self::Frame<'a> {
        SpiFrame { sck: self.sck, mosi: &self.mosi, miso: &self.miso, cs: self.cs }
    }

    fn validate(&self) -> bool {
        // CS active and clock running
        self.cs && self.sck
    }
}

// ─── I2C ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct I2cBus {
    pub sda:     bool,
    pub scl:     bool,
    pub address: u8,
    pub data:    u8,
}

#[derive(Debug)]
pub struct I2cFrame<'a> {
    pub sda:     bool,
    pub scl:     bool,
    pub address: &'a u8,
    pub data:    &'a u8,
}

impl ProtocolBus for I2cBus {
    const PROTOCOL: Protocol = Protocol::I2c;

    type Frame<'a> = I2cFrame<'a>;

    fn frame<'a>(&'a self) -> Self::Frame<'a> {
        I2cFrame { sda: self.sda, scl: self.scl, address: &self.address, data: &self.data }
    }

    fn validate(&self) -> bool {
        // Bus idle = both lines high
        self.sda && self.scl
    }
}

// ─── UART ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct UartBus {
    pub tx: u8,
    pub rx: u8,
}

#[derive(Debug)]
pub struct UartFrame<'a> {
    pub tx: &'a u8,
    pub rx: &'a u8,
}

impl ProtocolBus for UartBus {
    const PROTOCOL: Protocol = Protocol::Uart;

    type Frame<'a> = UartFrame<'a>;

    fn frame<'a>(&'a self) -> Self::Frame<'a> {
        UartFrame { tx: &self.tx, rx: &self.rx }
    }

    fn validate(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spi_bus_exposes_borrowed_frame() {
        let bus = SpiBus { sck: true, mosi: 0xA5, miso: 0x5A, cs: true };
        let frame = bus.frame();           // SpiFrame<'a> borrows from `bus`
        assert_eq!(*frame.mosi, 0xA5);
        assert!(bus.validate());
    }

    #[test]
    fn i2c_bus_validate_idle() {
        let bus = I2cBus { sda: true, scl: true, address: 0x50, data: 0xAA };
        assert!(bus.validate());
        let busy = I2cBus { sda: false, scl: true, address: 0x50, data: 0xAA };
        assert!(!busy.validate());
    }

    #[test]
    fn uart_bus_frame() {
        let bus = UartBus { tx: 0xAA, rx: 0x55 };
        let frame = bus.frame();
        assert_eq!(*frame.tx, 0xAA);
        assert_eq!(*frame.rx, 0x55);
    }
}