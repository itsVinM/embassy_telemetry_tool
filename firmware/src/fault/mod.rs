//! Fault-injection command task. The injector logic itself lives in the
//! host-testable `shared::fault` module; this module adds the bit-banged UART
//! command interface and drives the five protocol injectors.

use defmt::info;
use embassy_stm32::gpio::{Input, Level, Output, Pull, Speed};
use embassy_stm32::peripherals::{PB6, PB7};
use embassy_stm32::Peri;
use shared::fault::{
    busy_delay_us, CanBus, CanFaultInjector, I2cBus, I2cFaultInjector, OneWireBus,
    OneWireFaultInjector, SpiBus, SpiFaultInjector, UartBus, UartFaultInjector,
};
use shared::{FaultCommand, FaultConfig, FaultResult, FaultType, Protocol};

// ─── Bit-banged UART command interface (8N1, 115200) ──────────────────────────

pub struct UartBitbang<'d> {
    tx: Output<'d>,
    input: Input<'d>,
    baud_period_us: u32,
}

impl<'d> UartBitbang<'d> {
    pub fn new(tx_pin: Peri<'d, PB6>, rx_pin: Peri<'d, PB7>) -> Self {
        Self {
            tx: Output::new(tx_pin, Level::High, Speed::High),
            input: Input::new(rx_pin, Pull::Up),
            baud_period_us: 8,
        }
    }

    fn bit_delay(&self) {
        busy_delay_us(self.baud_period_us);
    }

    pub async fn write_byte(&mut self, byte: u8) {
        self.tx.set_low();
        self.bit_delay();
        for i in 0..8u8 {
            if (byte >> i) & 1 == 1 { self.tx.set_high(); }
            else { self.tx.set_low(); }
            self.bit_delay();
        }
        self.tx.set_high();
        self.bit_delay();
    }

    pub async fn write(&mut self, data: &[u8]) {
        for &byte in data {
            self.write_byte(byte).await;
        }
    }

    pub async fn read_byte(&mut self) -> u8 {
        while self.input.is_high() {}
        self.bit_delay();
        self.bit_delay();
        let mut byte = 0u8;
        for i in 0..8u8 {
            if self.input.is_high() { byte |= 1 << i; }
            self.bit_delay();
        }
        byte
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<(), ()> {
        for byte in buf.iter_mut() {
            *byte = self.read_byte().await;
        }
        Ok(())
    }
}

// ─── Command loop: drive all five protocol injectors ──────────────────────────
// Commands arrive on the bit-banged UART (PB6 TX / PB7 RX, 115200 8N1):
//   0x01 Arm   — arm all injectors
//   0x02 Disarm — disarm all injectors
//   0x03 Fire  — inject into one simulated frame per protocol, report result
//   0x04 Status — Armed or Disarmed
//   0x05 Reset  — disarm and clear injection counters
// Each reply is a single byte (FaultResult). Injected frames are printed via RTT.

#[embassy_executor::task]
pub async fn fault_task_entry(uart: UartBitbang<'static>) -> ! {
    info!("fault: task started — SPI/I2C/UART/CAN/OneWire injectors ready");
    let mut uart = uart;

    let mut can = CanFaultInjector::new();
    let mut spi = SpiFaultInjector::new();
    let mut i2c = I2cFaultInjector::new();
    let mut uart_inj = UartFaultInjector::new();
    let mut onewire = OneWireFaultInjector::new();

    can.configure(&FaultConfig::new(Protocol::Can, FaultType::CrcCorrupt));
    spi.configure(&FaultConfig::new(Protocol::Spi, FaultType::BitDelay).at_bit(0).for_us(20));
    i2c.configure(&FaultConfig::new(Protocol::I2c, FaultType::StuckAtOne).at_bit(0));
    uart_inj.configure(&FaultConfig::new(Protocol::Uart, FaultType::ParityError).at_bit(0));
    onewire.configure(&FaultConfig::new(Protocol::OneWire, FaultType::CrcCorrupt));

    let mut can_bus = CanBus {
        id: 0x123,
        data: [0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04],
        dlc: 8,
    };
    let mut spi_bus = SpiBus { sck: true, mosi: 0xA5, miso: 0x5A, cs: true };
    let mut i2c_bus = I2cBus { sda: true, scl: true, address: 0x50, data: 0xAA };
    let mut uart_bus = UartBus { tx: 0xAA, rx: 0x55 };
    let mut ow_bus = OneWireBus { line: true, rom_cmd: 0xCC, scratchpad: [0xFF; 9] };

    let mut buf = [0u8; 1];
    loop {
        let _ = uart.read(&mut buf).await;
        let cmd = match buf[0] {
            0x01 => FaultCommand::Arm,
            0x02 => FaultCommand::Disarm,
            0x03 => FaultCommand::Fire,
            0x04 => FaultCommand::Status,
            0x05 => FaultCommand::Reset,
            _ => continue,
        };

        let result = match cmd {
            FaultCommand::Arm => {
                can.arm();
                spi.arm();
                i2c.arm();
                uart_inj.arm();
                onewire.arm();
                FaultResult::Armed
            }
            FaultCommand::Disarm => {
                can.disarm();
                spi.disarm();
                i2c.disarm();
                uart_inj.disarm();
                onewire.disarm();
                FaultResult::Disarmed
            }
            FaultCommand::Fire => {
                let results = [
                    can.fire(&mut can_bus),
                    spi.fire(&mut spi_bus),
                    i2c.fire(&mut i2c_bus),
                    uart_inj.fire(&mut uart_bus),
                    onewire.fire(&mut ow_bus),
                ];
                if results.iter().any(|r| *r == FaultResult::Fired) {
                    info!(
                        "fault: fired — can id=0x{:03X} dlc={} | spi sck={} cs={} mosi=0x{:02X} miso=0x{:02X} | i2c sda={} scl={} addr=0x{:02X} data=0x{:02X} | uart tx=0x{:02X} | ow line={} rom=0x{:02X}",
                        can_bus.id, can_bus.dlc, spi_bus.sck, spi_bus.cs, spi_bus.mosi, spi_bus.miso,
                        i2c_bus.sda, i2c_bus.scl, i2c_bus.address, i2c_bus.data,
                        uart_bus.tx, ow_bus.line, ow_bus.rom_cmd
                    );
                    info!(
                        "fault: counts — can={} spi={} i2c={} uart={} ow={}",
                        can.injected_count(), spi.injected_count(), i2c.injected_count(),
                        uart_inj.injected_count(), onewire.injected_count()
                    );
                    FaultResult::Fired
                } else {
                    FaultResult::Error
                }
            }
            FaultCommand::Status => {
                let any_armed = can.is_armed()
                    || spi.is_armed()
                    || i2c.is_armed()
                    || uart_inj.is_armed()
                    || onewire.is_armed();
                if any_armed { FaultResult::Armed } else { FaultResult::Disarmed }
            }
            FaultCommand::Reset => {
                can.disarm();
                spi.disarm();
                i2c.disarm();
                uart_inj.disarm();
                onewire.disarm();
                can.reset_stats();
                spi.reset_stats();
                i2c.reset_stats();
                uart_inj.reset_stats();
                onewire.reset_stats();
                FaultResult::Disarmed
            }
        };

        let _ = uart.write(&[result as u8]).await;
        info!("fault: cmd={} result={}", buf[0], result as u8);
    }
}
