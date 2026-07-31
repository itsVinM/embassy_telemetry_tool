# STM32 DAQ Platform — Fault Injection Configuration

Mixed-signal DAQ and fault injection platform for STM32F401RE (Nucleo-64). Rust, bare-metal, no heap. This is the **fault injection** configuration of the [STM32 DAQ Platform](../embedded_mixed_signal_analyzer/).

## What

Protocol-level fault injection on embedded buses. SPI, I2C, UART, CAN, and OneWire fault injection with LFSR-driven probabilistic control, all feature-gated, all no-std.

For the full DAQ platform (analog capture, DAC output, trigger system, calibration, Python CLI), see [`embedded_mixed_signal_analyzer`](../embedded_mixed_signal_analyzer/).

## Modules

| Module | Feature flag | Description |
|--------|-------------|-------------|
| Fault | `fault` | SPI, I2C, UART, CAN, OneWire fault injection |
| Analog | `analog` (default) | ADC1 DMA ring buffer — dual-channel interleaved, 10 kHz |
| Digital | `digital` | TIM1 PWM output (4ch), TIM3 input capture |
| Transport | always | USART2 binary packet stream (115200 baud) |

## Architecture

```
firmware/src/main.rs           — entry, peripheral init
firmware/src/fault/mod.rs      — FaultEngine, UartBitbang
firmware/src/fault/spi.rs      — SPI injector
firmware/src/fault/i2c.rs      — I2C injector
firmware/src/fault/uart.rs     — UART injector
firmware/src/fault/can.rs      — CAN injector (MCP2515)
firmware/src/fault/onewire.rs  — OneWire injector
shared/src/lib.rs              — FaultInjector<'d, B> trait, tests
```

Key pattern: `FaultInjector<'d, B>` — generic over bus type `B`, lifetime `'d`. Same trait in both this project and the [rp2040-fault-inject](https://github.com/itsVinM/rp2040_embedded_faut_injection) project.

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
cargo build --release --features fault                         # + fault injection
```

## Flash

```bash
probe-rs run --chip STM32F401RETx target/thumbv7em-none-eabihf/release/firmware
```

## Tests

```bash
cd shared && cargo test
```

## Docker

```bash
docker compose up --build
```
