use std::{
  borrow::Cow,
  hash::{Hash, Hasher},
  sync::{Arc, OnceLock},
};

use rustc_hash::FxHasher;

use crate::{
  helpers::{
    get_generated_source_info, stream_and_get_source_and_map,
    stream_chunks_of_raw_source, stream_chunks_of_source_map, GeneratedInfo,
    Stream, ToStream,
  },
  object_pool::ObjectPool,
  source::{IndexSourceMap, Section, SectionOffset, SourceValue},
  BoxSource, MapOptions, RawBufferSource, Source, SourceExt, SourceMap,
};

#[derive(Default)]
struct CachedData {
  hash: OnceLock<u64>,
  size: OnceLock<usize>,
  chunks: OnceLock<Vec<&'static str>>,
  columns_map: OnceLock<Option<SourceMap>>,
  line_only_map: OnceLock<Option<SourceMap>>,
  columns_index_map: OnceLock<Option<IndexSourceMap>>,
  line_only_index_map: OnceLock<Option<IndexSourceMap>>,
}

/// It tries to reused cached results from other methods to avoid calculations,
/// usually used after modify is finished.
///
/// - [webpack-sources docs](https://github.com/webpack/webpack-sources/#cachedsource).
///
/// ```
/// use rspack_sources::{
///   BoxSource, CachedSource, ConcatSource, MapOptions, OriginalSource,
///   RawStringSource, Source, SourceExt, SourceMap,
/// };
///
/// let mut concat = ConcatSource::new([
///   RawStringSource::from("Hello World\n".to_string()).boxed(),
///   OriginalSource::new(
///     "console.log('test');\nconsole.log('test2');\n",
///     "console.js",
///   )
///   .boxed(),
/// ]);
/// concat.add(OriginalSource::new("Hello2\n", "hello.md"));
///
/// let cached = CachedSource::new(concat);
///
/// assert_eq!(
///   cached.source().into_string_lossy(),
///   "Hello World\nconsole.log('test');\nconsole.log('test2');\nHello2\n"
/// );
/// // second time will be fast.
/// assert_eq!(
///   cached.source().into_string_lossy(),
///   "Hello World\nconsole.log('test');\nconsole.log('test2');\nHello2\n"
/// );
/// ```
pub struct CachedSource {
  inner: BoxSource,
  cache: Arc<CachedData>,
}

impl CachedSource {
  /// Create a [CachedSource] with the original [Source].
  pub fn new<T: SourceExt>(inner: T) -> Self {
    let box_source = inner.boxed();
    // Check if it's already a BoxSource containing a CachedSource
    if let Some(cached_source) =
      box_source.as_ref().as_any().downcast_ref::<CachedSource>()
    {
      return cached_source.clone();
    }

    Self {
      inner: box_source,
      cache: Arc::new(CachedData::default()),
    }
  }

  fn get_or_init_chunks(&self) -> &[&str] {
    self.cache.chunks.get_or_init(|| {
      let mut chunks = Vec::new();
      self.inner.rope(&mut |chunk| {
        chunks.push(chunk);
      });
      #[allow(unsafe_code)]
      // SAFETY: CachedSource guarantees that the underlying source outlives the cache,
      // so transmuting Vec<&str> to Vec<&'static str> is safe in this context.
      // This allows us to store string slices in the cache without additional allocations.
      unsafe {
        std::mem::transmute::<Vec<&str>, Vec<&'static str>>(chunks)
      }
    })
  }
}

impl Source for CachedSource {
  fn source(&self) -> SourceValue<'_> {
    // Check if it's a RawBufferSource containing a CachedSource
    if let Some(buffer_source) = self
      .inner
      .as_ref()
      .as_any()
      .downcast_ref::<RawBufferSource>()
    {
      return buffer_source.source();
    }

    let chunks = self.get_or_init_chunks();
    let mut string = String::with_capacity(self.size());
    for chunk in chunks {
      string.push_str(chunk);
    }
    SourceValue::String(Cow::Owned(string))
  }

  fn rope<'a>(&'a self, on_chunk: &mut dyn FnMut(&'a str)) {
    let chunks = self.get_or_init_chunks();
    chunks.iter().for_each(|chunk| on_chunk(chunk));
  }

  fn buffer(&self) -> Cow<'_, [u8]> {
    self.inner.buffer()
  }

  fn size(&self) -> usize {
    *self.cache.size.get_or_init(|| {
      if let Some(chunks) = self.cache.chunks.get() {
        return chunks.iter().fold(0, |acc, chunk| acc + chunk.len());
      }
      self.inner.size()
    })
  }

  fn map(
    &self,
    object_pool: &ObjectPool,
    options: &MapOptions,
  ) -> Option<SourceMap> {
    if options.columns {
      self
        .cache
        .columns_map
        .get_or_init(|| self.inner.map(object_pool, options))
        .clone()
    } else {
      self
        .cache
        .line_only_map
        .get_or_init(|| self.inner.map(object_pool, options))
        .clone()
    }
  }

  fn index_map(
    &self,
    object_pool: &ObjectPool,
    options: &MapOptions,
  ) -> Option<IndexSourceMap> {
    if options.columns {
      self
        .cache
        .columns_index_map
        .get_or_init(|| self.inner.index_map(object_pool, options))
        .clone()
    } else {
      self
        .cache
        .line_only_index_map
        .get_or_init(|| self.inner.index_map(object_pool, options))
        .clone()
    }
  }

  fn to_writer(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
    self.inner.to_writer(writer)
  }
}

struct CachedSourceStream<'source> {
  stream: Box<dyn Stream + 'source>,
  cache: Arc<CachedData>,
  source: Cow<'source, str>,
}

impl<'a> CachedSourceStream<'a> {
  fn new(cache_source: &'a CachedSource) -> Self {
    let source = cache_source.source().into_string_lossy();

    Self {
      stream: cache_source.inner.to_stream(),
      cache: cache_source.cache.clone(),
      source,
    }
  }
}

impl Stream for CachedSourceStream<'_> {
  fn chunks<'a>(
    &'a self,
    object_pool: &'a ObjectPool,
    options: &MapOptions,
    on_chunk: crate::helpers::OnChunk<'_, 'a>,
    on_source: crate::helpers::OnSource<'_, 'a>,
    on_name: crate::helpers::OnName<'_, 'a>,
  ) -> GeneratedInfo {
    let cell = if options.columns {
      &self.cache.columns_map
    } else {
      &self.cache.line_only_map
    };
    match cell.get() {
      Some(map) => {
        if let Some(map) = map {
          stream_chunks_of_source_map(
            options,
            object_pool,
            self.source.as_ref(),
            map,
            on_chunk,
            on_source,
            on_name,
          )
        } else {
          stream_chunks_of_raw_source(
            self.source.as_ref(),
            options,
            on_chunk,
            on_source,
            on_name,
          )
        }
      }
      None => {
        let (generated_info, map) = stream_and_get_source_and_map(
          options,
          object_pool,
          self.stream.as_ref(),
          on_chunk,
          on_source,
          on_name,
        );
        cell.get_or_init(|| map);
        generated_info
      }
    }
  }

  fn sections_size_hint(&self) -> usize {
    if let Some(index_map) = self.cache.columns_index_map.get() {
      index_map
        .as_ref()
        .map(|index_map| index_map.sections().len())
        .unwrap_or(0)
    } else if let Some(index_map) = self.cache.line_only_index_map.get() {
      index_map
        .as_ref()
        .map(|index_map| index_map.sections().len())
        .unwrap_or(0)
    } else {
      self.stream.sections_size_hint()
    }
  }

  fn sections<'a>(
    &'a self,
    object_pool: &'a ObjectPool,
    columns: bool,
    on_section: crate::helpers::OnSection<'_, 'a>,
  ) -> GeneratedInfo {
    let cell = if columns {
      &self.cache.columns_index_map
    } else {
      &self.cache.line_only_index_map
    };
    match cell.get() {
      Some(index_map) => {
        let generated_info = get_generated_source_info(self.source.as_ref());
        if let Some(index_map) = index_map {
          for section in index_map.sections() {
            on_section(section.offset, Some(section.map.clone()));
          }
        } else {
          on_section(SectionOffset::default(), None);
        }
        generated_info
      }
      None => {
        let mut sections = Vec::new();
        let generated_info =
          self
            .stream
            .sections(object_pool, columns, &mut |offset, map| {
              if let Some(ref map) = map {
                sections.push(Section {
                  offset,
                  map: map.clone(),
                });
              }
              on_section(offset, map);
            });
        let index_map = if sections.is_empty() {
          None
        } else {
          Some(IndexSourceMap::new(sections))
        };
        cell.get_or_init(|| index_map);
        generated_info
      }
    }
  }
}

impl ToStream for CachedSource {
  fn to_stream<'a>(&'a self) -> Box<dyn Stream + 'a> {
    Box::new(CachedSourceStream::new(self))
  }
}

impl Clone for CachedSource {
  fn clone(&self) -> Self {
    Self {
      inner: self.inner.clone(),
      cache: self.cache.clone(),
    }
  }
}

impl Hash for CachedSource {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    (self.cache.hash.get_or_init(|| {
      let mut hasher = FxHasher::default();
      self.inner.hash(&mut hasher);
      hasher.finish()
    }))
    .hash(state);
  }
}

impl PartialEq for CachedSource {
  fn eq(&self, other: &Self) -> bool {
    self.inner.as_ref() == other.inner.as_ref()
  }
}

impl Eq for CachedSource {}

impl std::fmt::Debug for CachedSource {
  fn fmt(
    &self,
    f: &mut std::fmt::Formatter<'_>,
  ) -> Result<(), std::fmt::Error> {
    let indent = f.width().unwrap_or(0);
    let indent_str = format!("{:indent$}", "", indent = indent);

    writeln!(f, "{indent_str}CachedSource::new(")?;
    writeln!(
      f,
      "{indent_str}{:indent$?}",
      self.inner,
      indent = indent + 2
    )?;
    write!(f, "{indent_str}).boxed()")
  }
}

#[cfg(test)]
mod tests {
  use crate::{
    ConcatSource, OriginalSource, RawBufferSource, RawStringSource,
    ReplaceSource, SourceExt, SourceMapSource, WithoutOriginalOptions,
  };

  use super::*;

  #[test]
  fn line_number_should_not_add_one() {
    let source = ConcatSource::new([
      CachedSource::new(RawStringSource::from("\n")).boxed(),
      SourceMapSource::new(WithoutOriginalOptions {
        value: "\nconsole.log(1);\n".to_string(),
        name: "index.js".to_string(),
        source_map: SourceMap::new(
          ";AACA",
          vec!["index.js".into()],
          vec!["// DELETE IT\nconsole.log(1)".into()],
          vec![],
        ),
      })
      .boxed(),
    ]);
    let map = source
      .map(&ObjectPool::default(), &Default::default())
      .unwrap();
    assert_eq!(map.mappings(), ";;AACA");
  }

  #[test]
  fn should_allow_to_store_and_share_cached_data() {
    let original = OriginalSource::new("Hello World", "test.txt");
    let source = CachedSource::new(original);
    let clone = source.clone();

    // fill up cache
    let map_options = MapOptions::default();
    source.source();
    source.buffer();
    source.size();
    source.map(&ObjectPool::default(), &map_options);

    assert_eq!(
      *clone.cache.columns_map.get().unwrap(),
      source.map(&ObjectPool::default(), &map_options)
    );
  }

  #[test]
  fn should_return_the_correct_size_for_binary_files() {
    let source = OriginalSource::new(
      String::from_utf8(vec![0; 256]).unwrap(),
      "file.wasm",
    );
    let cached_source = CachedSource::new(source);

    assert_eq!(cached_source.size(), 256);
    assert_eq!(cached_source.size(), 256);
  }

  #[test]
  fn should_return_the_correct_size_for_cached_binary_files() {
    let source = OriginalSource::new(
      String::from_utf8(vec![0; 256]).unwrap(),
      "file.wasm",
    );
    let cached_source = CachedSource::new(source);

    cached_source.source();
    assert_eq!(cached_source.size(), 256);
    assert_eq!(cached_source.size(), 256);
  }

  #[test]
  fn should_return_the_correct_size_for_text_files() {
    let source = OriginalSource::new("TestTestTest", "file.js");
    let cached_source = CachedSource::new(source);

    assert_eq!(cached_source.size(), 12);
    assert_eq!(cached_source.size(), 12);
  }

  #[test]
  fn should_return_the_correct_size_for_cached_text_files() {
    let source = OriginalSource::new("TestTestTest", "file.js");
    let cached_source = CachedSource::new(source);

    cached_source.source();
    assert_eq!(cached_source.size(), 12);
    assert_eq!(cached_source.size(), 12);
  }

  #[test]
  fn should_produce_correct_output_for_cached_raw_source() {
    let map_options = MapOptions::new(true);

    let source = RawStringSource::from("Test\nTest\nTest\n");
    let mut on_chunk_count = 0;
    let mut on_source_count = 0;
    let mut on_name_count = 0;
    let generated_info = {
      let object_pool = ObjectPool::default();
      let stream = source.to_stream();
      stream.chunks(
        &object_pool,
        &map_options,
        &mut |_chunk, _mapping| {
          on_chunk_count += 1;
        },
        &mut |_source_index, _source, _source_content| {
          on_source_count += 1;
        },
        &mut |_name_index, _name| {
          on_name_count += 1;
        },
      )
    };

    let cached_source = CachedSource::new(source);
    cached_source.to_stream().chunks(
      &ObjectPool::default(),
      &map_options,
      &mut |_chunk, _mapping| {},
      &mut |_source_index, _source, _source_content| {},
      &mut |_name_index, _name| {},
    );

    let mut cached_on_chunk_count = 0;
    let mut cached_on_source_count = 0;
    let mut cached_on_name_count = 0;
    let cached_generated_info = cached_source.to_stream().chunks(
      &ObjectPool::default(),
      &map_options,
      &mut |_chunk, _mapping| {
        cached_on_chunk_count += 1;
      },
      &mut |_source_index, _source, _source_content| {
        cached_on_source_count += 1;
      },
      &mut |_name_index, _name| {
        cached_on_name_count += 1;
      },
    );

    assert_eq!(on_chunk_count, cached_on_chunk_count);
    assert_eq!(on_source_count, cached_on_source_count);
    assert_eq!(on_name_count, cached_on_name_count);
    assert_eq!(generated_info, cached_generated_info);
  }

  #[test]
  fn should_have_correct_buffer_if_cache_buffer_from_cache_source() {
    let buf = vec![128u8];
    let source = CachedSource::new(RawBufferSource::from(buf.clone()));

    source.source();
    assert_eq!(source.buffer(), buf.as_slice());
  }

  #[test]
  fn hash_should_different_when_map_are_different() {
    let hash1 = {
      let mut source =
        ReplaceSource::new(OriginalSource::new("Hello", "hello.txt").boxed());
      source.insert(5, " world", None);
      let cache = CachedSource::new(source);
      let mut hasher = FxHasher::default();
      cache.hash(&mut hasher);
      hasher.finish()
    };

    let hash2 = {
      let source = OriginalSource::new("Hello world", "hello.txt").boxed();
      let cache = CachedSource::new(source);
      let mut hasher = FxHasher::default();
      cache.hash(&mut hasher);
      hasher.finish()
    };

    assert!(hash1 != hash2);
  }

  #[test]
  fn size_over_a_raw_buffer_source() {
    // buffer from PNG
    let raw =
      RawBufferSource::from(vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13]);
    let raw_size = raw.size();
    let cached = CachedSource::new(raw.boxed());
    let cached_size = cached.size();
    assert_eq!(raw_size, cached_size);
  }

  #[test]
  fn index_map_should_be_cached() {
    let original = OriginalSource::new("hello\nworld\n", "test.txt");
    let cached = CachedSource::new(original);
    let pool = ObjectPool::default();
    let options = MapOptions::default();

    let index_map1 = cached.index_map(&pool, &options);
    let index_map2 = cached.index_map(&pool, &options);
    assert!(index_map1.is_some());
    assert_eq!(index_map1, index_map2);
  }

  #[test]
  fn index_map_cached_matches_inner() {
    let inner = ConcatSource::new([
      OriginalSource::new("a\n", "a.js").boxed(),
      OriginalSource::new("b\n", "b.js").boxed(),
    ]);
    let pool = ObjectPool::default();
    let options = MapOptions::default();
    let expected = inner.index_map(&pool, &options);
    let cached = CachedSource::new(inner);
    let result = cached.index_map(&pool, &options);
    assert_eq!(result, expected);
  }

  #[test]
  fn index_map_returns_none_for_raw_cached() {
    let cached = CachedSource::new(RawStringSource::from("no map"));
    let pool = ObjectPool::default();
    let options = MapOptions::default();
    assert!(cached.index_map(&pool, &options).is_none());
    // Second call also returns None (from cache)
    assert!(cached.index_map(&pool, &options).is_none());
  }
}
