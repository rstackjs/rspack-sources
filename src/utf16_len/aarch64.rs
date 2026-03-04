//! NEON-based UTF-16 length calculation (always available on aarch64).

use std::arch::aarch64::*;

/// Compute the number of UTF-16 code units for UTF-8 bytes using NEON.
#[allow(unsafe_code)]
pub(crate) fn utf16_length_from_utf8(bytes: &[u8]) -> usize {
  let len = bytes.len();
  if len == 0 {
    return 0;
  }

  let mut continuation_count: usize = 0;
  let mut four_byte_count: usize = 0;
  let mut i: usize = 0;

  // SAFETY: NEON is always available on aarch64.
  unsafe {
    let cont_mask = vdupq_n_u8(0xC0);
    let cont_val = vdupq_n_u8(0x80);
    let four_threshold = vdupq_n_u8(0xEF);
    let one = vdupq_n_u8(1);

    // Process 16 bytes at a time, in batches of up to 255 iterations
    // to avoid u8 overflow in the per-lane accumulators.
    while i + 16 <= len {
      let batch = ((len - i) / 16).min(255);
      let mut cont_acc = vdupq_n_u8(0);
      let mut four_acc = vdupq_n_u8(0);

      for _ in 0..batch {
        let chunk = vld1q_u8(bytes.as_ptr().add(i));

        // Continuation bytes: (byte & 0xC0) == 0x80
        let masked = vandq_u8(chunk, cont_mask);
        let is_cont = vceqq_u8(masked, cont_val);
        // is_cont lanes are 0xFF (-1) for continuation bytes;
        // subtracting -1 is adding 1.
        cont_acc = vsubq_u8(cont_acc, is_cont);

        // Four-byte leaders (byte >= 0xF0):
        // saturating subtract 0xEF gives non-zero only for bytes >= 0xF0,
        // then clamp to 1 with min.
        let sub = vqsubq_u8(chunk, four_threshold);
        let is_four = vminq_u8(sub, one);
        four_acc = vaddq_u8(four_acc, is_four);

        i += 16;
      }

      // Horizontal sum across all lanes.
      continuation_count += vaddlvq_u8(cont_acc) as usize;
      four_byte_count += vaddlvq_u8(four_acc) as usize;
    }
  }

  // Scalar tail for remaining bytes.
  for j in i..len {
    let b = bytes[j];
    continuation_count += ((b & 0xC0) == 0x80) as usize;
    four_byte_count += (b >= 0xF0) as usize;
  }

  len - continuation_count + four_byte_count
}
