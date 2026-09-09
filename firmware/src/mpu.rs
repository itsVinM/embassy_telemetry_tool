// MPU (Memory Protection Unit) Configuration for STM32F401RE
// Purpose: Hardware-enforced memory isolation - critical for safety/security

use cortex_m::register::mpu::{self, Mpu, Region};

/// MPU Region Attributes
mod attr {
    use cortex_m::register::mpu::Attr;
    
    /// Flash: Read-Only + Execute (code cannot be modified at runtime)
    pub const FLASH_RO: Attr = Attr {
        executable: true,
        access: cortex_m::register::mpu::Access::ReadOnly,
        shareable: false,
        cacheable: true,
        bufferable: false,
    };
    
    /// SRAM: Read-Write + Execute-Never (data only, no code execution)
    pub const SRAM_RW_XN: Attr = Attr {
        executable: false,
        access: cortex_m::register::mpu::Access::ReadWrite,
        shareable: true,
        cacheable: true,
        bufferable: true,
    };
    
    /// Peripherals: Device memory (no caching, no speculation)
    pub const DEVICE_RW_XN: Attr = Attr {
        executable: false,
        access: cortex_m::register::mpu::Access::ReadWrite,
        shareable: true,
        cacheable: false,
        bufferable: false,
    };
    
    /// Stack Guard: No Access (detects stack overflow)
    pub const NO_ACCESS: Attr = Attr {
        executable: false,
        access: cortex_m::register::mpu::Access::NoAccess,
        shareable: false,
        cacheable: false,
        bufferable: false,
    };
}

/// Initialize MPU with 4 regions
pub fn init() {
    // Enable MPU with default memory map for privileged access
    Mpu::enable(mpu::Config::default());

    // Region 0: Flash (0x0800_0000 - 0x0808_0000) = 512KB - RO + Execute
    Region::new(0, 0x0800_0000 as *mut u8, 512 * 1024)
        .attr(attr::FLASH_RO)
        .enable();

    // Region 1: SRAM (0x2000_0000 - 0x2001_8000) = 96KB - RW + XN
    Region::new(1, 0x2000_0000 as *mut u8, 96 * 1024)
        .attr(attr::SRAM_RW_XN)
        .enable();

    // Region 2: Peripherals (0x4000_0000 - 0x6000_0000) - Device memory
    Region::new(2, 0x4000_0000 as *mut u8, 512 * 1024 * 1024)
        .attr(attr::DEVICE_RW_XN)
        .enable();

    // Region 3: Stack Guard - last 1KB of RAM (0x2001_7C00 - 0x2001_8000) - No Access
    let stack_guard_start = 0x2001_8000 - 1024;
    Region::new(3, stack_guard_start as *mut u8, 1024)
        .attr(attr::NO_ACCESS)
        .enable();

    // Enable unprivileged mode (user mode) - MPU enforced for unprivileged code
    cortex_m::register::control::modify(|c| c.set_unprivileged(true));
    
    // Disable write buffer for predictable behavior
    cortex_m::register::scb::set_sysctl_bit(cortex_m::register::scb::SYSCtl::DISDEFWBUF, true);
}

/// Verify MPU is active and regions configured
pub fn verify() -> bool {
    // Try to write to flash - should fault if MPU working
    let flash_ptr = 0x0800_0000 as *mut u32;
    unsafe {
        core::ptr::write_volatile(flash_ptr, 0xDEADBEEF);
        // If we reach here, MPU not working (or we're privileged)
        core::ptr::read_volatile(flash_ptr) != 0xDEADBEEF
    }
}