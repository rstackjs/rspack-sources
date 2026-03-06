#![allow(missing_docs)]

#[cfg(not(codspeed))]
pub use criterion::*;

#[cfg(codspeed)]
pub use codspeed_criterion_compat::*;

use rspack_sources::SourceMap;

const ANTD_MIN_JS_MAP: &str = include_str!(concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/benches/fixtures/antd-mini/antd.min.js.map"
));

pub fn benchmark_parse_source_map_from_json(b: &mut Bencher) {
  b.iter(|| black_box(SourceMap::from_json(ANTD_MIN_JS_MAP).unwrap()))
}

pub fn benchmark_source_map_to_json(b: &mut Bencher) {
  let source_map = SourceMap::from_json(ANTD_MIN_JS_MAP).unwrap();
  b.iter(|| source_map.to_json())
}

fn main() {}
