# stm32-daq

Rust + Embassy mixed-signal DAQ and protocol-level fault injection platform for the STM32F401RE (Nucleo-64). Async, no-std, no heap.

## Highlights

- **Embassy async runtime** — `embassy-executor` thread executor, `embassy-stm32` (stm32f401re, TIM2 time driver), `embassy-time`.
- **DMA-driven capture** — dual-channel interleaved ADC1 capture at 10 kHz into a ring buffer, streamed as binary `SamplePacket`s over USART2+DMA at 115200 baud.
- **Protocol fault injection** — SPI, I2C, UART, CAN, and OneWire injectors with LFSR-driven probabilistic control, driven over a bit-banged UART command link (PB6/PB7).
- **Host-testable core** — all injector logic lives in the `shared` crate (pure no-std logic), unit-tested on the host; the firmware task is a thin driver over it.
- **Hardened runtime** — MPU regions (flash RO, SRAM RW+XN, peripherals device, stack guard), flash ART configured for 84 MHz, boot-time health checks (clock, stack canary, RAM self-test).
- **RTT diagnostics** — defmt + defmt-rtt + panic-probe: structured logging and panic backtraces over RTT, no extra wiring.

## Modules

| Module | Feature flag | Description |
|--------|-------------|-------------|
| Fault | `fault` | SPI, I2C, UART, CAN, OneWire fault injection (bit-banged UART command interface) |
| Analog | `analog` (default) | ADC1/DMA2 dual-channel interleaved ring buffer, 10 kHz |
| Digital | `digital` | TIM1 PWM output (4ch), TIM3 input capture |
| Transport | `analog` | Async USART2+DMA binary packet stream (115200 baud) |

## Architecture

```
firmware/src/main.rs        — entry, MPU init, clock tree (84 MHz), health checks
firmware/src/mpu.rs         — MPU regions (flash RO, SRAM RW+XN, stack guard)
firmware/src/health.rs      — clock, stack-canary, and RAM self-tests
firmware/src/analog.rs      — ADC1/DMA2 ring buffer → SamplePacket → Transport
firmware/src/transport.rs   — async USART2 + DMA transport (SamplePacket binary)
firmware/src/digital.rs     — TIM1 PWM (4ch) + TIM3 input capture
firmware/src/fault/mod.rs   — fault task + bit-banged UART command interface
shared/src/lib.rs           — DAQ/fault data types, DMA-safe buffers, wire protocol
shared/src/fault.rs         — concrete injectors (SPI/I2C/UART/CAN/OneWire), unit-tested
```

Design pattern: each protocol has one concrete injector struct (`configure`/`arm`/
`disarm`/`fire`/`is_armed`/`injected_count`/`reset_stats`) plus a simulated bus.
Injectors are pure logic in `shared`, so they run on the host under `cargo test`;
the firmware drives all five over a bit-banged UART.

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

## Flash & run

```bash
probe-rs run --chip STM32F401RETx target/thumbv7em-none-eabihf/release/firmware
```

Logs and panics are streamed over RTT (`probe-rs run` shows them; see `cargo install probe-rs-tools`).

## Tests

```bash
cd shared && cargo test
```
