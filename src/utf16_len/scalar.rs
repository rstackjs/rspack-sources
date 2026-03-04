//! Scalar fallback for platforms without SIMD support.

/// Compute the number of UTF-16 code units for UTF-8 bytes using scalar code.
pub(crate) fn utf16_length_from_utf8(bytes: &[u8]) -> usize {
  let len = bytes.len();
  let mut continuation_count: usize = 0;
  let mut four_byte_count: usize = 0;

  for &b in bytes {
    continuation_count += ((b & 0xC0) == 0x80) as usize;
    four_byte_count += (b >= 0xF0) as usize;
  }

  len - continuation_count + four_byte_count
}
