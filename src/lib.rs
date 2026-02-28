//! Rusty [`webpack-sources`](https://github.com/webpack/webpack-sources) port.

mod cached_source;
mod concat_source;
mod decoder;
mod encoder;
mod error;
mod helpers;
mod linear_map;
mod object_pool;
mod original_source;
mod raw_source;
mod replace_source;
mod source;
mod source_content_lines;
mod source_map_source;
mod to_json;
mod with_utf16;

pub use cached_source::CachedSource;
pub use concat_source::ConcatSource;
pub use error::{Error, Result};
pub use original_source::OriginalSource;
pub use raw_source::{RawBufferSource, RawStringSource};
pub use replace_source::{ReplaceSource, ReplacementEnforce};
pub use source::{
  BoxSource, IndexSourceMap, MapOptions, Mapping, OriginalLocation, Section,
  SectionOffset, Source, SourceExt, SourceMap, SourceValue,
};
pub use source_map_source::{
  SourceMapSource, SourceMapSourceOptions, WithoutOriginalOptions,
};

/// Reexport `ToStream` related types.
pub mod stream_chunks {
  pub use super::helpers::{
    stream_chunks_default, GeneratedInfo, OnChunk, OnName, OnSection, OnSource,
    Stream, ToStream,
  };
}

pub use helpers::{
  decode_mappings, encode_mappings, get_generated_source_info,
};

pub use object_pool::ObjectPool;
