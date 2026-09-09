// Stack Canary - Detects stack overflow/underflow
// Purpose: Runtime stack integrity check - essential for safety certification

/// Canary value placed at end of RAM (0x2001_7FFC)
const CANARY_VALUE: u32 = 0xDEAD_BEEF;
const CANARY_ADDR: *mut u32 = 0x2001_7FFC as *mut u32;

/// Initialize stack canary at boot
pub fn init() {
    unsafe {
        core::ptr::write_volatile(CANARY_ADDR, CANARY_VALUE);
    }
}

/// Check if canary is intact
pub fn check() -> bool {
    unsafe {
        core::ptr::read_volatile(CANARY_ADDR) == CANARY_VALUE
    }
}

/// Get canary address for debugging
pub fn address() -> u32 {
    CANARY_ADDR as u32
}

/// Read raw canary value
pub fn read_raw() -> u32 {
    unsafe { core::ptr::read_volatile(CANARY_ADDR) }
}

/// Demo: intentionally corrupt stack to show detection
pub fn demo_corrupt() {
    unsafe {
        core::ptr::write_volatile(CANARY_ADDR, 0x0);
    }
}