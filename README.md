# stm32-selftest

BIST + UART telemetry firmware for the STM32F401RE (Nucleo-64). Rust + Embassy, no-std, no heap.

## What it does

1. **BIST on boot** — validates the MCU before doing anything: MPU + stack canary, clock/RAM health checks, ADC telemetry loopback, and SPI/I2C/UART fault-inject stability (fires against simulated in-RAM buses, asserts expected frames).
2. **UART telemetry** — streams `SamplePacket`s from anything connected over UART (PA2/PA3) to the host over USB CDC.
3. **Host monitor** — BIST report + live telemetry available on demand over USB CDC.

## Modules

| Module | Description |
|--------|-------------|
| `fault/` | SPI/I2C/UART injectors vs simulated buses — BIST stability check only |
| `sco/` | ADC capture (1 MS/s, DMA) — telemetry loopback for BIST |
| `uart_telemetry.rs` | Streams telemetry from UART-connected devices to host |
| `mpu.rs` | 4 MPU regions: Flash RO, SRAM RW+XN, Peripheral, StackGuard |
| `canary.rs` | Stack canary at 0x2001_7FFC |
| `health.rs` | Clock, stack canary, RAM self-tests |

## Architecture

```
main.rs            — entry: MPU init, clock tree, BIST gate, UART telemetry loop
bist.rs            — orchestrates all BIST checks; blocks if anything fails
fault/mod.rs       — drives SPI/I2C/UART injectors against simulated in-RAM buses
sco/               — ADC telemetry capture (BIST loopback + live)
uart_telemetry.rs  — streams telemetry from external UART devices
mpu.rs             — MPU: Flash=RO, SRAM=RW+XN, Peripheral=device, StackGuard=NoAccess
canary.rs          — stack canary: 0xDEAD_BEEF at end of SRAM
health.rs          — clock checks, RAM write/readback, stack canary verification
shared/src/lib.rs  — Protocol(3), FaultType, FaultConfig, DmaBuf, SamplePacket, etc.
shared/src/fault.rs      — SPI/I2C/UART injectors (type-state)
shared/src/fault_traits.rs — GAT ProtocolBus trait + state markers
```

## Build

```bash
cargo build --release --features fault
cargo build --release --features full
```

## Test

```bash
cd shared && cargo test   # 40 host tests: injectors, telemetry structs, protocol
```

## Flash

```bash
probe-rs run --chip STM32F401RETx target/thumbv7em-none-eabihf/release/stm32-selftest
```
