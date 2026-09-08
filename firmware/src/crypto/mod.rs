//! cryptographic primitives and utilities
#[cfg(feature = "aes")]
pub mod aes;
#[cfg(feature = "chacha")]
pub mod chacha;
#[cfg(feature = "sha256")]
pub mod sha256;

#[cfg(feature = "trng")]
pub mod trng;

pub mod side_channel;

use rand_core::{RngCore, CryptoRng};

use subtle::ConstantTimeEq;

pub trait CryptoPrimitive{
    type Input;
    type Output;
    type Key;

    // Perform cryptographic operation on the input using the provided key and return the output.
    fn process(&self, input: &Self::Input, key: &Self::Key) -> Self::Output;
    // Perform operation with fixed key
    fn process_fixed_key(&self, input: &Self::Input) -> Self::Output;
}

/// AES-128 context for side channel
#[cfg(feature = "aes")]
pub struct Aes128Context {
    key: [u8; 16],
    encrypt: bool,
}

#[cfg(feature = "aes")]
impl Aes128Context {
    pub fn new_encrypt(key: [u8; 16]) -> Self {
        Self { key, encrypt: true }
    }
    pub fn new_decrypt(key: [u8; 16]) -> Self {
        Self { key, encrypt: false }
    }
}

#[cfg(feature = "aes")]
impl CryptoPrimitive for Aes128Context {
    type Input = [u8; 16];
    type Output = [u8; 16];
    type Key = [u8; 16];

    fn process(&self, input: &Self::Input, key: &Self::Key) -> Self::Output {
        let mut output = *input;
        if self.encrypt {
            aes::encrypt_block(&mut output, key);
        } else {
            aes::decrypt_block(&mut output, key);
        }
        output  
    }

    fn process_fixed_key(&self, input: &Self::Input) -> Self::Output {
        self.process(input, &self.key)
    }
}

/// ChaCha20 context for side-channel testing
#[cfg(feature = "chacha")]
pub struct ChaCha20Context {
    key: [u8; 32],
    nonce: [u8; 12],
    counter: u32,
}

#[cfg(feature = "chacha")]
impl ChaCha20Context {
    pub fn new(key: [u8; 32], nonce: [u8; 12], counter: u32) -> Self {
        Self { key, nonce, counter }
    }
}

#[cfg(feature = "chacha")]
impl CryptoPrimitive for ChaCha20Context {
    type Input = [u8; 64];
    type Output = [u8; 64];
    type Key = [u8; 32];

    fn process(&self, input: &Self::Input, key: &Self::Key) -> Self::Output {
        let mut output = *input;
        chacha::chacha20_block(key, &self.nonce, self.counter, &mut output);
        output
    }

    fn process_fixed_key(&self, input: &Self::Input) -> Self::Output {
        self.process(input, &self.key)
    }
}

/// SHA-256 context for side-channel testing
#[cfg(feature = "sha256")]
pub struct Sha256Context;

#[cfg(feature = "sha256")]
impl CryptoPrimitive for Sha256Context {
    type Input = heapless::Vec<u8, 256>;
    type Output = [u8; 32];
    type Key = ();

    fn process(&self, input: &Self::Input, _key: &Self::Key) -> Self::Output{
        sha256::hash(input)
    }
    fn process_fixed_key(&self, input: &Self::Input) -> Self::Output {
        self.process(input, &())
    }
}

/// True random number generator context for side-channel testing
#[cfg(feature = "trng")]
pub struct TrngContext{
    rng: embasssy_Stm32::rng::Rng,
}

#[cfg(feature = "trng")]
impl TrngContext {
    pub fn new(rng: embasssy_Stm32::rng::Rng) -> Self {
        Self { rng }
    }

    pub fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.rng.fill_bytes(dest).ok();
    }

    pub fn health_check(&mut self, samples: usize) -> TrngHealthReport{
        let mut data = vec!(0u8; samples);
        self.rng.fill_bytes(&mut data).ok();

        let ones: usize = data.iter().map(|&b| b.count_ones() as usize).sum();
        let total_bits = samples * 8;
        let proportion = ones as f32 / total_bits as f32;

        TrngHealthReport {
            samples,
            ones_count: ones,
            proportion_ones: proportion,
            passes_monobit : (proportion - 0.5).abs() < 0.02,
        }
    }
}

#[cfg(feature = "trng")]
#[derive(Debug, Clone, Copy)]
pub struct TrngHealthReport {
    pub samples: usize,
    pub ones_count: usize,
    pub proportion_ones: f32,
    pub passes_monobit: bool,
}

/// Constant-time comparison for side-channel resistant verification
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

// Secure memory zeroization
pub fn zeroize(data: &mut [u8]) {
    for byte in data.iter_mut() {
        *byte = 0;
    }
    compile_fence();
}

fn compile_fence() {
    #[cfg(target_arch = "arm")]
    unsafe {
        core::arch::asm!("", options(nomem, nostack, preserves_flags));
    }

    #[cfg(not(target_arch = "arm"))]
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "aes")]
    fn aes_encrypt_decrypt() {
        let key = [0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c];
        let plaintext = [0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93, 0x17, 0x2a];
        let expected = [0x3a, 0xd7, 0x7b, 0xb4, 0x0d, 0x7a, 0x36, 0x60, 0xa8, 0x9e, 0xca, 0xf3, 0x24, 0x66, 0xef, 0x97];
        
        let ctx = Aes128Context::new_encrypt(key);
        let ciphertext = ctx.process_fixed_key(&plaintext);
        assert_eq!(ciphertext, expected);

        let ctx_dec = Aes128Context::new_decrypt(key);
        let decrypted = ctx_dec.process_fixed_key(&ciphertext);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ct_eq_works(){
        assert!(ct_eq(b"hello", b"hello"));
        assert!(!ct_eq(b"hello", b"world"));
        assert!(!ct_eq(b"hello", b"hell"));
    }
}

