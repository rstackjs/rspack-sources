//! x86_64 SIMD UTF-16 length calculation.
//!
//! Uses AVX2 (32 bytes at a time) when available at runtime,
//! falls back to SSE2 (16 bytes at a time, always available on x86_64).

use std::arch::x86_64::*;

/// Compute the number of UTF-16 code units for UTF-8 bytes.
#[allow(unsafe_code)]
pub(crate) fn utf16_length_from_utf8(bytes: &[u8]) -> usize {
  let len = bytes.len();
  if len == 0 {
    return 0;
  }

  // SAFETY: Feature detection ensures the correct SIMD path is used.
  unsafe {
    if std::is_x86_feature_detected!("avx2") {
      utf16_length_avx2(bytes)
    } else {
      utf16_length_sse2(bytes)
    }
  }
}

/// AVX2 implementation: processes 32 bytes per iteration.
#[target_feature(enable = "avx2")]
#[allow(unsafe_code)]
unsafe fn utf16_length_avx2(bytes: &[u8]) -> usize {
  let len = bytes.len();
  let mut continuation_count: usize = 0;
  let mut four_byte_count: usize = 0;
  let mut i: usize = 0;

  let cont_mask = _mm256_set1_epi8(0xC0_u8 as i8);
  let cont_val = _mm256_set1_epi8(0x80_u8 as i8);
  let four_threshold = _mm256_set1_epi8(0xEF_u8 as i8);
  let ones = _mm256_set1_epi8(1);
  let zero = _mm256_setzero_si256();

  // Process 32 bytes at a time, in batches of up to 255 iterations
  // to avoid u8 overflow in the per-lane accumulators.
  while i + 32 <= len {
    let batch = ((len - i) / 32).min(255);
    let mut cont_acc = zero;
    let mut four_acc = zero;

    for _ in 0..batch {
      let chunk = _mm256_loadu_si256(bytes.as_ptr().add(i) as *const __m256i);

      let masked = _mm256_and_si256(chunk, cont_mask);
      let is_cont = _mm256_cmpeq_epi8(masked, cont_val);
      cont_acc = _mm256_sub_epi8(cont_acc, is_cont);

      let sub = _mm256_subs_epu8(chunk, four_threshold);
      let is_four = _mm256_min_epu8(sub, ones);
      four_acc = _mm256_add_epi8(four_acc, is_four);

      i += 32;
    }

    // Horizontal sum: SAD produces 4 u64 partial sums across two
    // 128-bit lanes. Extract high/low halves, add, then reduce.
    continuation_count += hsum_u8_avx2(cont_acc, zero);
    four_byte_count += hsum_u8_avx2(four_acc, zero);
  }

  // Scalar tail for remaining bytes (up to 31).
  for &b in &bytes[i..] {
    continuation_count += ((b & 0xC0) == 0x80) as usize;
    four_byte_count += (b >= 0xF0) as usize;
  }

  len - continuation_count + four_byte_count
}

/// Horizontal sum of all u8 lanes in a __m256i register.
#[target_feature(enable = "avx2")]
#[inline]
#[allow(unsafe_code)]
unsafe fn hsum_u8_avx2(v: __m256i, zero: __m256i) -> usize {
  let sad = _mm256_sad_epu8(v, zero);
  let hi = _mm256_extracti128_si256::<1>(sad);
  let lo = _mm256_castsi256_si128(sad);
  let sum128 = _mm_add_epi64(lo, hi);
  let shift = _mm_srli_si128::<8>(sum128);
  _mm_cvtsi128_si64(_mm_add_epi64(sum128, shift)) as usize
}

/// SSE2 implementation: processes 16 bytes per iteration.
#[allow(unsafe_code)]
unsafe fn utf16_length_sse2(bytes: &[u8]) -> usize {
  let len = bytes.len();
  let mut continuation_count: usize = 0;
  let mut four_byte_count: usize = 0;
  let mut i: usize = 0;

  let cont_mask = _mm_set1_epi8(0xC0_u8 as i8);
  let cont_val = _mm_set1_epi8(0x80_u8 as i8);
  let four_threshold = _mm_set1_epi8(0xEF_u8 as i8);
  let ones = _mm_set1_epi8(1);
  let zero = _mm_setzero_si128();

  // Process 16 bytes at a time, in batches of up to 255 iterations
  // to avoid u8 overflow in the per-lane accumulators.
  while i + 16 <= len {
    let batch = ((len - i) / 16).min(255);
    let mut cont_acc = zero;
    let mut four_acc = zero;

    for _ in 0..batch {
      let chunk = _mm_loadu_si128(bytes.as_ptr().add(i) as *const __m128i);

      let masked = _mm_and_si128(chunk, cont_mask);
      let is_cont = _mm_cmpeq_epi8(masked, cont_val);
      cont_acc = _mm_sub_epi8(cont_acc, is_cont);

      let sub = _mm_subs_epu8(chunk, four_threshold);
      let is_four = _mm_min_epu8(sub, ones);
      four_acc = _mm_add_epi8(four_acc, is_four);

      i += 16;
    }

    // Horizontal sum via SAD (Sum of Absolute Differences) against zero.
    let cont_sad = _mm_sad_epu8(cont_acc, zero);
    let high = _mm_srli_si128::<8>(cont_sad);
    let sum = _mm_add_epi64(cont_sad, high);
    continuation_count += _mm_cvtsi128_si64(sum) as usize;

    let four_sad = _mm_sad_epu8(four_acc, zero);
    let high = _mm_srli_si128::<8>(four_sad);
    let sum = _mm_add_epi64(four_sad, high);
    four_byte_count += _mm_cvtsi128_si64(sum) as usize;
  }

  // Scalar tail for remaining bytes.
  for &b in &bytes[i..] {
    continuation_count += ((b & 0xC0) == 0x80) as usize;
    four_byte_count += (b >= 0xF0) as usize;
  }

  len - continuation_count + four_byte_count
}
