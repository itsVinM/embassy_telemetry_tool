# STM32 HIL 

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
probe-rs run --chip STM32F401RETx target/thumbv7em-none-eabihf/release/embassy-telemetry-tool
```
