//! USB CDC-ACM host link.
//!
//! Streams `SamplePacket` telemetry to the host and serves the BIST report on
//! demand. Data path: USART2 (device link) → `Channel<SamplePacket>` → this
//! module → USB CDC → host.

use defmt::{debug, info};
use embassy_stm32::bind_interrupts;
use embassy_stm32::peripherals;
use embassy_stm32::usb;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::{Driver as UsbDriver, EndpointError};
use embassy_usb::{Builder, Config};

use shared::SamplePacket;

/// Queued telemetry packets waiting to be flushed to the host.
pub type PacketChannel = Channel<NoopRawMutex, SamplePacket, 8>;

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

/// Wire command bytes the host may send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HostCommand {
    QueryBist = 0x01,
    StartStream = 0x02,
    StopStream = 0x03,
    QueryTelemetry = 0x04,
}

impl HostCommand {
    pub fn parse(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::QueryBist),
            0x02 => Some(Self::StartStream),
            0x03 => Some(Self::StopStream),
            0x04 => Some(Self::QueryTelemetry),
            _ => None,
        }
    }
}

struct Disconnected;

impl From<EndpointError> for Disconnected {
    fn from(_: EndpointError) -> Self {
        Disconnected
    }
}

/// Async task that owns the USB device. Receives telemetry packets from the
/// ring buffer and forwards them to the host; answers host commands.
#[embassy_executor::task]
pub async fn usb_task(
    usb: embassy_stm32::Peri<'static, peripherals::USB_OTG_FS>,
    dm: embassy_stm32::Peri<'static, peripherals::PA11>,
    dp: embassy_stm32::Peri<'static, peripherals::PA12>,
    packets: &'static PacketChannel,
    bist_code: &'static core::sync::atomic::AtomicU8,
    telemetry_loopback_bytes: &'static [core::sync::atomic::AtomicU8; 8],
) {
    static EP_OUT_BUF: UnsafeCellBuf = UnsafeCellBuf { buf: [0u8; 256] };
    static CLASS_STATE: SyncState = SyncState(core::cell::UnsafeCell::new(None));
    static CLASS: SafeCellClass =
        SafeCellClass(core::cell::UnsafeCell::new(None));
    static DEV: SafeCellDevice =
        SafeCellDevice(core::cell::UnsafeCell::new(None));

    let driver = unsafe {
        usb::Driver::new_fs(
            usb,
            Irqs,
            dp,
            dm,
            &mut *EP_OUT_BUF.buf.as_mut_ptr(),
            usb::Config::default(),
        )
    };

    let mut config = Config::new(0x1209, 0x5747);
    config.manufacturer = Some("itsVinM");
    config.product = Some("embassy-telemetry-tool");
    config.serial_number = Some("20260909");

    static CONFIG_DESC: Uninitialized = Uninitialized([0u8; 256]);
    static BOS_DESC: Uninitialized = Uninitialized([0u8; 256]);
    static CONTROL_BUF: Uninitialized = Uninitialized([0u8; 64]);

    let mut builder = Builder::new(
        driver,
        config,
        &mut *unsafe { CONFIG_DESC.0.as_mut_ptr() },
        &mut *unsafe { BOS_DESC.0.as_mut_ptr() },
        &mut [],
        &mut *unsafe { CONTROL_BUF.0.as_mut_ptr() },
    );

    let state = CLASS_STATE.0.get().get_or_insert(State::new());
    let class = CLASS.0.get().get_or_insert(CdcAcmClass::new(&mut builder, state, 64));
    let dev = DEV.0.get().get_or_insert(builder.build());

    let device_fut = dev.run();
    let host_fut = async {
        loop {
            info!("usb: waiting for connection");
            class.wait_connection().await;
            info!("usb: connected");
            let _ = host_loop(class, packets, bist_code, telemetry_loopback_bytes).await;
            info!("usb: disconnected");
        }
    };
    embassy_futures::join::join(device_fut, host_fut).await;
}

async fn host_loop(
    class: &mut CdcAcmClass<'static, embassy_stm32::usb::Driver<'static>>,
    packets: &'static PacketChannel,
    bist_code: &core::sync::atomic::AtomicU8,
    telemetry_loopback_bytes: &[core::sync::atomic::AtomicU8; 8],
) -> Result<(), Disconnected> {
    let mut buf = [0u8; 64];
    loop {
        if let Ok(n) = class.read_packet(&mut buf) {
            if n > 0 {
                handle_command(class, buf[0], bist_code, telemetry_loopback_bytes).await?;
            }
        }

        while let Ok(pkt) = packets.try_recv() {
            debug!("usb: forwarding packet seq={}", pkt.seq);
            class.write_packet(pkt.as_bytes()).await?;
        }
    }
}

async fn handle_command(
    class: &mut CdcAcmClass<'static, embassy_stm32::usb::Driver<'static>>,
    cmd: u8,
    bist_code: &core::sync::atomic::AtomicU8,
    telemetry_loopback_bytes: &[core::sync::atomic::AtomicU8; 8],
) -> Result<(), Disconnected> {
    match HostCommand::parse(cmd) {
        Some(HostCommand::QueryBist) => {
            let code = bist_code.load(core::sync::atomic::Ordering::Relaxed);
            info!("usb: host requested BIST code {}", code);
            class.write_packet(&[code]).await?;
        }
        Some(HostCommand::QueryTelemetry) => {
            let mut msg = [0u8; 9];
            msg[0] = 0x84;
            for (i, b) in telemetry_loopback_bytes.iter().enumerate() {
                msg[i + 1] = b.load(core::sync::atomic::Ordering::Relaxed);
            }
            class.write_packet(&msg).await?;
        }
        Some(HostCommand::StartStream) | Some(HostCommand::StopStream) => {
            info!("usb: host cmd 0x{:02X}", cmd);
        }
        None => {
            info!("usb: unknown host command 0x{:02X}", cmd);
        }
    }
    Ok(())
}

// Minimal lifetime-hole helpers: `'static` storage allocated inside the task.
struct UnsafeCellBuf {
    buf: [u8; 256],
}
struct Uninitialized([u8; 256]);
struct SyncState(core::cell::UnsafeCell<Option<State>>);
struct SafeCellClass(core::cell::UnsafeCell<Option<CdcAcmClass<'static, embassy_stm32::usb::Driver<'static>>>>);
struct SafeCellDevice(core::cell::UnsafeCell<Option<embassy_usb::Device<'static, embassy_stm32::usb::Driver<'static>>>>);

unsafe impl Sync for UnsafeCellBuf {}
unsafe impl Sync for Uninitialized {}
unsafe impl Sync for SyncState {}
unsafe impl Sync for SafeCellClass {}
unsafe impl Sync for SafeCellDevice {}

const _: () = {
    // SAFETY asserts that these are only ever used single-threaded from one task.
    fn _assert_send_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<SyncState>();
        assert_sync::<SafeCellClass>();
        assert_sync::<SafeCellDevice>();
    }
};