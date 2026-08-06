# stm32-daq

Mixed-signal DAQ and protocol-level fault injection platform for STM32F401RE (Nucleo-64). Rust, bare-metal, no heap, all no-std.

> **Proposed repo name:** `stm32-daq` (current directory name `embedded_daq_system` is the old one). This is a standalone platform — it is not a configuration of any other repo.

## What

- **DAQ:** dual-channel interleaved ADC capture at 10 kHz via ADC1/DMA2 ring buffer, TIM1 PWM output (4ch), TIM3 input capture, USART2 binary packet stream.
- **Fault injection:** SPI, I2C, UART, CAN, and OneWire fault injection with LFSR-driven probabilistic control, feature-gated.

## Modules

| Module | Feature flag | Description |
|--------|-------------|-------------|
| Fault | `fault` | SPI, I2C, UART, CAN, OneWire fault injection |
| Analog | `analog` (default) | ADC1 DMA ring buffer — dual-channel interleaved, 10 kHz |
| Digital | `digital` | TIM1 PWM output (4ch), TIM3 input capture |
| Transport | `analog` | USART2 binary packet stream (115200 baud) |

## Architecture

```
firmware/src/main.rs        — entry, MPU init, clock tree, health checks
firmware/src/fault/mod.rs   — fault task + bit-banged UART command interface
firmware/src/analog.rs      — ADC1/DMA2 ring buffer → SamplePacket → Transport
firmware/src/digital.rs     — TIM1 PWM (4ch) + TIM3 input capture
firmware/src/transport.rs   — USART2 + DMA packet transport
firmware/src/health.rs      — clock, stack-canary, and RAM self-tests
firmware/src/mpu.rs         — MPU regions (flash RO, SRAM RW+XN, stack guard)
shared/src/lib.rs           — DAQ/fault data types, wire protocol
shared/src/fault.rs         — concrete injectors (SPI/I2C/UART/CAN/OneWire), unit-tested
```

Key pattern: each protocol has one concrete injector struct (`configure`/`arm`/
`disarm`/`fire`/`is_armed`/`injected_count`/`reset_stats`) plus a simulated bus.
The injectors are pure logic in `shared`, so they are unit-tested on the host;
the firmware task drives all five over a bit-banged UART (PB6/PB7).

## Pin map

```
PA0  ADC1 CH0        PA6  TIM3 CH1
PA1  ADC1 CH1        PA8  TIM1 CH1 (25%)
PA2  USART2 TX       PA9  TIM1 CH2 (50%)
PA3  USART2 RX       PA10 TIM1 CH3 (75%)
PB6  Fault UART TX   PA11 TIM1 CH4 (10%)
PB7  Fault UART RX
```

## Build

```bash
cargo build --release                                          # analog (default)
cargo build --release --no-default-features --features digital # digital
cargo build --release --no-default-features --features fault   # fault injection
```

## Flash

```bash
probe-rs run --chip STM32F401RETx target/thumbv7em-none-eabihf/release/firmware
```

## Tests

The fault-injection library and shared data types are pure logic and run on the host:

```bash
cd shared && cargo test
```
