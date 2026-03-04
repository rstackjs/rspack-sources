//! SIMD-accelerated UTF-16 length calculation from UTF-8 bytes.
//!
//! Formula: `utf16_len = byte_length - continuation_bytes + four_byte_leaders`
//!
//! Where:
//! - continuation bytes: `(byte & 0xC0) == 0x80`
//! - four-byte leaders: `byte >= 0xF0`

#[cfg(target_arch = "x86_64")]
mod x86_64;

#[cfg(target_arch = "aarch64")]
mod aarch64;

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm32;

#[cfg(not(any(
  target_arch = "x86_64",
  target_arch = "aarch64",
  all(target_arch = "wasm32", target_feature = "simd128"),
)))]
mod scalar;

#[cfg(target_arch = "x86_64")]
pub(crate) use x86_64::utf16_length_from_utf8;

#[cfg(target_arch = "aarch64")]
pub(crate) use aarch64::utf16_length_from_utf8;

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub(crate) use wasm32::utf16_length_from_utf8;

#[cfg(not(any(
  target_arch = "x86_64",
  target_arch = "aarch64",
  all(target_arch = "wasm32", target_feature = "simd128"),
)))]
pub(crate) use scalar::utf16_length_from_utf8;
