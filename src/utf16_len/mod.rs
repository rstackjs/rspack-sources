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

#[cfg(test)]
mod tests {
  use super::utf16_length_from_utf8;

  /// Reference implementation for verification.
  fn utf16_len_ref(s: &str) -> usize {
    s.encode_utf16().count()
  }

  #[test]
  fn empty() {
    assert_eq!(utf16_length_from_utf8(b""), 0);
  }

  #[test]
  fn ascii() {
    let s = "Hello, world!";
    assert_eq!(utf16_length_from_utf8(s.as_bytes()), utf16_len_ref(s));
  }

  #[test]
  fn two_byte_utf8() {
    // Latin-extended, Cyrillic (2-byte UTF-8 → 1 UTF-16 code unit each)
    let s = "àáâãäåæçèé Привет";
    assert_eq!(utf16_length_from_utf8(s.as_bytes()), utf16_len_ref(s));
  }

  #[test]
  fn three_byte_utf8() {
    // CJK characters (3-byte UTF-8 → 1 UTF-16 code unit each)
    let s = "你好世界こんにちは안녕하세요";
    assert_eq!(utf16_length_from_utf8(s.as_bytes()), utf16_len_ref(s));
  }

  #[test]
  fn four_byte_utf8() {
    // Emoji (4-byte UTF-8 → 2 UTF-16 code units / surrogate pair)
    let s = "🌍🌎🌏🎉🎊🎈";
    assert_eq!(utf16_length_from_utf8(s.as_bytes()), utf16_len_ref(s));
  }

  #[test]
  fn mixed() {
    let s = "Hello, 世界! 🌍 Привет мир! こんにちは世界！";
    assert_eq!(utf16_length_from_utf8(s.as_bytes()), utf16_len_ref(s));
  }

  #[test]
  fn single_characters() {
    // 1-byte
    assert_eq!(utf16_length_from_utf8("a".as_bytes()), 1);
    // 2-byte
    assert_eq!(utf16_length_from_utf8("é".as_bytes()), 1);
    // 3-byte
    assert_eq!(utf16_length_from_utf8("中".as_bytes()), 1);
    // 4-byte → surrogate pair
    assert_eq!(utf16_length_from_utf8("🎉".as_bytes()), 2);
  }

  #[test]
  fn longer_than_simd_register() {
    // >32 bytes to exercise the SIMD loop (not just the scalar tail).
    let s = "The quick brown fox jumps over the lazy dog. 你好世界！🎉🎊";
    assert_eq!(utf16_length_from_utf8(s.as_bytes()), utf16_len_ref(s));
  }

  #[test]
  fn large_input() {
    // >255×32 bytes to exercise the batch overflow guard.
    let base = "Hello, 世界! 🌍 こんにちは！";
    let s = base.repeat(500);
    assert_eq!(utf16_length_from_utf8(s.as_bytes()), utf16_len_ref(&s));
  }

  #[test]
  fn boundary_lengths() {
    // Test lengths around SIMD boundaries (15, 16, 17, 31, 32, 33).
    let base = "abcdefghijklmnopqrstuvwxyz0123456789";
    for len in [1, 15, 16, 17, 31, 32, 33, 48, 64, 255, 256] {
      let s = &base.repeat(10)[..len];
      assert_eq!(
        utf16_length_from_utf8(s.as_bytes()),
        utf16_len_ref(s),
        "failed for len={len}"
      );
    }
  }
}
