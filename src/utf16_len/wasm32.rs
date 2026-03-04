//! WASM SIMD128-based UTF-16 length calculation.

use std::arch::wasm32::*;

/// Compute the number of UTF-16 code units for UTF-8 bytes using WASM SIMD128.
#[allow(unsafe_code)]
pub(crate) fn utf16_length_from_utf8(bytes: &[u8]) -> usize {
  let len = bytes.len();
  if len == 0 {
    return 0;
  }

  let mut continuation_count: usize = 0;
  let mut four_byte_count: usize = 0;
  let mut i: usize = 0;

  let cont_mask = u8x16_splat(0xC0);
  let cont_val = u8x16_splat(0x80);
  let four_threshold = u8x16_splat(0xEF);
  let ones = u8x16_splat(1);

  // Process 16 bytes at a time, in batches of up to 255 iterations
  // to avoid u8 overflow in the per-lane accumulators.
  while i + 16 <= len {
    let batch = ((len - i) / 16).min(255);
    let mut cont_acc = u8x16_splat(0);
    let mut four_acc = u8x16_splat(0);

    for _ in 0..batch {
      // SAFETY: i + 16 <= len is guaranteed by the while condition.
      let chunk =
        unsafe { v128_load(bytes.as_ptr().add(i) as *const v128) };

      // Continuation bytes: (byte & 0xC0) == 0x80
      let masked = v128_and(chunk, cont_mask);
      let is_cont = u8x16_eq(masked, cont_val);
      // is_cont lanes are 0xFF (-1) for continuation bytes;
      // subtracting -1 is adding 1.
      cont_acc = u8x16_sub(cont_acc, is_cont);

      // Four-byte leaders (byte >= 0xF0):
      // saturating subtract 0xEF gives non-zero only for bytes >= 0xF0,
      // then clamp to 1 with min.
      let sub = u8x16_sub_sat(chunk, four_threshold);
      let is_four = u8x16_min(sub, ones);
      four_acc = u8x16_add(four_acc, is_four);

      i += 16;
    }

    // Horizontal sum via pairwise widening addition.
    continuation_count += horizontal_sum_u8(cont_acc);
    four_byte_count += horizontal_sum_u8(four_acc);
  }

  // Scalar tail for remaining bytes.
  for j in i..len {
    let b = bytes[j];
    continuation_count += ((b & 0xC0) == 0x80) as usize;
    four_byte_count += (b >= 0xF0) as usize;
  }

  len - continuation_count + four_byte_count
}

/// Horizontal sum of all u8 lanes in a v128 register.
#[inline]
fn horizontal_sum_u8(v: v128) -> usize {
  // u8x16 -> i16x8 (pairwise add adjacent u8 lanes)
  let pairs = i16x8_extadd_pairwise_u8x16(v);
  // i16x8 -> i32x4 (pairwise add adjacent i16 lanes)
  let quads = i32x4_extadd_pairwise_i16x8(pairs);
  // Sum the 4 i32 lanes.
  (i32x4_extract_lane::<0>(quads)
    + i32x4_extract_lane::<1>(quads)
    + i32x4_extract_lane::<2>(quads)
    + i32x4_extract_lane::<3>(quads)) as usize
}
