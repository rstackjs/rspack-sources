#![allow(missing_docs)]

#[cfg(not(codspeed))]
pub use criterion::*;

#[cfg(codspeed)]
pub use codspeed_criterion_compat::*;

use rspack_sources::utf16_len;

const ASCII: &str = "The quick brown fox jumps over the lazy dog. This is a longer sentence to provide more data for benchmarking purposes, with various words and punctuation marks included.";

const CJK: &str = "这是一段中文测试文本，用于测试UTF-8编码中多字节字符的处理性能。日本語のテキストも含まれています。한국어 텍스트도 포함되어 있습니다。";

const EMOJI: &str =
  "Hello 🌍🌎🌏! Flags: 🇺🇸🇬🇧🇯🇵🇨🇳 Family: 👨‍👩‍👧‍👦 Skin: 👋🏻👋🏼👋🏽👋🏾👋🏿 Fun: 🎉🎊🎈🎁🎄🎃";

const MIXED: &str = "Hello, 世界! 🌍 Привет мир! こんにちは世界！Héllo wörld! 你好世界！안녕하세요 세계! مرحبا بالعالم";

pub fn bench_simd_utf16_len(b: &mut Bencher) {
  let input = [ASCII, CJK, EMOJI, MIXED].join("\n");
  b.iter(|| black_box(utf16_len(&input)));
}

pub fn bench_std_utf16_len(b: &mut Bencher) {
  let input = [ASCII, CJK, EMOJI, MIXED].join("\n");
  b.iter(|| black_box(input.encode_utf16().count()));
}

fn main() {}
