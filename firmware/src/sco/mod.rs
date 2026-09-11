//! SCO — telemetry capture subsystem.
//!
//! Scope for this firmware is kept deliberately small: capture the two internal
//! ADC reference channels (VREFINT, temperature) for the BIST telemetry
//! loopback, and stream them to the host as `SamplePacket`s. No external
//! instrumentation channels, no DMA ring buffers, no trigger/alignment/export
//! machinery — that lives in test-instrument-hub if ever needed.

pub mod adc_capture;

pub use adc_capture::{telemetry_loopback, TelemetryLoopback};