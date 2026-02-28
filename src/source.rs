use std::{
  any::{Any, TypeId},
  borrow::Cow,
  convert::{TryFrom, TryInto},
  fmt,
  hash::{Hash, Hasher},
  sync::Arc,
};

use dyn_clone::DynClone;
use serde::Deserialize;

use crate::{
  helpers::{decode_mappings, encode_mappings, Stream, ToStream},
  object_pool::ObjectPool,
  to_json::to_json,
  Result,
};

/// An alias for `Box<dyn Source>`.
pub type BoxSource = Arc<dyn Source>;

/// A unified representation for source content that can be either text or binary data.
///
/// `SourceValue` provides a flexible way to handle source content regardless of whether
/// it's originally stored as a string or raw bytes. This is particularly useful for
/// build tools and bundlers that need to process various types of source files.
#[derive(Debug, PartialEq, Eq)]
pub enum SourceValue<'a> {
  /// Text content stored as a UTF-8 string.
  String(Cow<'a, str>),
  /// Binary content stored as raw bytes.
  Buffer(Cow<'a, [u8]>),
}

impl<'a> SourceValue<'a> {
  /// Convert the source value to a string using lossy UTF-8 conversion.
  ///
  /// This method converts both string and buffer variants to `Cow<str>`.
  /// For buffer data that contains invalid UTF-8 sequences, replacement
  /// characters (�) will be used in place of invalid sequences.
  pub fn into_string_lossy(self) -> Cow<'a, str> {
    match self {
      SourceValue::String(cow) => cow,
      SourceValue::Buffer(cow) => match cow {
        Cow::Borrowed(bytes) => String::from_utf8_lossy(bytes),
        Cow::Owned(bytes) => {
          match String::from_utf8_lossy(&bytes) {
            Cow::Borrowed(_) => {
              // SAFETY: When `String::from_utf8_lossy` returns `Cow::Borrowed(_)`,
              // it guarantees that the input slice contains only valid UTF-8 bytes.
              // Since we're operating on the exact same `bytes` that were just
              // validated by `from_utf8_lossy`, we can safely skip the UTF-8
              // validation in `String::from_utf8_unchecked`.
              //
              // This optimization avoids the redundant UTF-8 validation that would
              // occur if we used `String::from_utf8(bytes).unwrap()` or similar.
              #[allow(unsafe_code)]
              Cow::Owned(unsafe { String::from_utf8_unchecked(bytes) })
            }
            Cow::Owned(s) => Cow::Owned(s),
          }
        }
      },
    }
  }

  /// Get a reference to the source content as bytes.
  ///
  /// This method provides access to the raw byte representation of the source
  /// content regardless of whether it was originally stored as a string or buffer.
  pub fn as_bytes(&self) -> &[u8] {
    match self {
      SourceValue::String(cow) => cow.as_bytes(),
      SourceValue::Buffer(cow) => cow.as_ref(),
    }
  }

  /// Convert the source value into bytes.
  ///
  /// This method consumes the `SourceValue` and converts it to `Cow<'a, [u8]>`,
  /// providing the most efficient representation possible while preserving
  /// the original borrowing relationships.
  pub fn into_bytes(self) -> Cow<'a, [u8]> {
    match self {
      SourceValue::String(cow) => match cow {
        Cow::Borrowed(s) => Cow::Borrowed(s.as_bytes()),
        Cow::Owned(s) => Cow::Owned(s.into_bytes()),
      },
      SourceValue::Buffer(cow) => cow,
    }
  }

  /// Check if the source value contains binary data.
  ///
  /// Returns `true` if this `SourceValue` is a `Buffer` variant containing
  /// raw bytes, `false` if it's a `String` variant containing text data.
  pub fn is_buffer(&self) -> bool {
    matches!(self, SourceValue::Buffer(_))
  }

  /// Returns `true` if `self` has a length of zero bytes.
  pub fn is_empty(&self) -> bool {
    match self {
      SourceValue::String(string) => string.is_empty(),
      SourceValue::Buffer(buffer) => buffer.is_empty(),
    }
  }
}

/// [Source] abstraction, [webpack-sources docs](https://github.com/webpack/webpack-sources/#source).
pub trait Source:
  ToStream + DynHash + AsAny + DynEq + DynClone + fmt::Debug + Sync + Send
{
  /// Get the source code.
  fn source(&self) -> SourceValue<'_>;

  /// Return a lightweight "rope" view of the source as borrowed string slices.
  fn rope<'a>(&'a self, on_chunk: &mut dyn FnMut(&'a str));

  /// Get the source buffer.
  fn buffer(&self) -> Cow<'_, [u8]>;

  /// Get the size of the source.
  fn size(&self) -> usize;

  /// Get the [SourceMap].
  fn map(
    &self,
    object_pool: &ObjectPool,
    options: &MapOptions,
  ) -> Option<SourceMap>;

  /// Get the [IndexSourceMap].
  ///
  /// Returns an index source map which uses sections to represent the mappings.
  /// This is more efficient for concatenated sources as it avoids the expensive
  /// mapping merge. The default implementation wraps the result of [Source::map]
  /// into a single-section [IndexSourceMap].
  fn index_map(
    &self,
    object_pool: &ObjectPool,
    options: &MapOptions,
  ) -> Option<IndexSourceMap> {
    self.map(object_pool, options).map(|map| {
      IndexSourceMap::new(vec![Section {
        offset: SectionOffset { line: 0, column: 0 },
        map,
      }])
    })
  }

  /// Update hash based on the source.
  fn update_hash(&self, state: &mut dyn Hasher) {
    self.dyn_hash(state);
  }

  /// Writes the source into a writer, preferably a `std::io::BufWriter<std::io::Write>`.
  fn to_writer(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()>;
}

impl Source for BoxSource {
  #[inline]
  fn source(&self) -> SourceValue<'_> {
    self.as_ref().source()
  }

  #[inline]
  fn rope<'a>(&'a self, on_chunk: &mut dyn FnMut(&'a str)) {
    self.as_ref().rope(on_chunk)
  }

  #[inline]
  fn buffer(&self) -> Cow<'_, [u8]> {
    self.as_ref().buffer()
  }

  #[inline]
  fn size(&self) -> usize {
    self.as_ref().size()
  }

  #[inline]
  fn map(
    &self,
    object_pool: &ObjectPool,
    options: &MapOptions,
  ) -> Option<SourceMap> {
    self.as_ref().map(object_pool, options)
  }

  #[inline]
  fn index_map(
    &self,
    object_pool: &ObjectPool,
    options: &MapOptions,
  ) -> Option<IndexSourceMap> {
    self.as_ref().index_map(object_pool, options)
  }

  #[inline]
  fn to_writer(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
    self.as_ref().to_writer(writer)
  }
}

dyn_clone::clone_trait_object!(Source);

impl ToStream for BoxSource {
  fn to_stream<'a>(&'a self) -> Box<dyn Stream + 'a> {
    self.as_ref().to_stream()
  }
}

// for `updateHash`
pub trait DynHash {
  fn dyn_hash(&self, state: &mut dyn Hasher);
}

impl<H: Hash> DynHash for H {
  fn dyn_hash(&self, mut state: &mut dyn Hasher) {
    self.hash(&mut state);
  }
}

impl Hash for dyn Source {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.dyn_hash(state)
  }
}

pub trait AsAny {
  fn as_any(&self) -> &dyn Any;
}

impl<T: Any> AsAny for T {
  fn as_any(&self) -> &dyn Any {
    self
  }
}

pub trait DynEq {
  fn dyn_eq(&self, other: &dyn Any) -> bool;
  fn type_id(&self) -> TypeId;
}

impl<E: Eq + Any> DynEq for E {
  fn dyn_eq(&self, other: &dyn Any) -> bool {
    if let Some(other) = other.downcast_ref::<E>() {
      self == other
    } else {
      false
    }
  }

  fn type_id(&self) -> TypeId {
    TypeId::of::<E>()
  }
}

impl PartialEq for dyn Source {
  fn eq(&self, other: &Self) -> bool {
    if self.as_any().type_id() != other.as_any().type_id() {
      return false;
    }
    self.dyn_eq(other.as_any())
  }
}

impl Eq for dyn Source {}

/// Extension methods for [Source].
pub trait SourceExt {
  /// An alias for [BoxSource::from].
  fn boxed(self) -> BoxSource;
}

impl<T: Source + 'static> SourceExt for T {
  fn boxed(self) -> BoxSource {
    if let Some(source) = self.as_any().downcast_ref::<BoxSource>() {
      return source.clone();
    }
    Arc::new(self)
  }
}

/// Options for [Source::map].
#[derive(Debug, Clone)]
pub struct MapOptions {
  /// Whether have columns info in generated [SourceMap] mappings.
  pub columns: bool,
  /// Whether the source will have changes, internal used for `ReplaceSource`, etc.
  pub(crate) final_source: bool,
}

impl Default for MapOptions {
  fn default() -> Self {
    Self {
      columns: true,
      final_source: false,
    }
  }
}

impl MapOptions {
  /// Create [MapOptions] with columns.
  pub fn new(columns: bool) -> Self {
    Self {
      columns,
      ..Default::default()
    }
  }
}

/// The source map created by [Source::map].
#[derive(Clone, PartialEq, Eq)]
pub struct SourceMap {
  version: u8,
  file: Option<Arc<str>>,
  sources: Arc<[String]>,
  sources_content: Arc<[Arc<str>]>,
  names: Arc<[String]>,
  mappings: Arc<str>,
  source_root: Option<Arc<str>>,
  debug_id: Option<Arc<str>>,
  ignore_list: Option<Arc<Vec<u32>>>,
}

impl std::fmt::Debug for SourceMap {
  fn fmt(
    &self,
    f: &mut std::fmt::Formatter<'_>,
  ) -> std::result::Result<(), std::fmt::Error> {
    let indent = f.width().unwrap_or(0);
    let indent_str = format!("{:indent$}", "", indent = indent);

    write!(
      f,
      "{indent_str}SourceMap::from_json({:?}).unwrap()",
      self.to_json()
    )?;

    Ok(())
  }
}
impl Hash for SourceMap {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.file.hash(state);
    self.mappings.hash(state);
    self.sources.hash(state);
    self.sources_content.hash(state);
    self.names.hash(state);
    self.source_root.hash(state);
    self.ignore_list.hash(state);
  }
}

impl SourceMap {
  /// Create a [SourceMap].
  pub fn new<Mappings, Sources, SourcesContent, Names>(
    mappings: Mappings,
    sources: Sources,
    sources_content: SourcesContent,
    names: Names,
  ) -> Self
  where
    Mappings: Into<Arc<str>>,
    Sources: Into<Arc<[String]>>,
    SourcesContent: Into<Vec<Arc<str>>>,
    Names: Into<Arc<[String]>>,
  {
    Self {
      version: 3,
      file: None,
      mappings: mappings.into(),
      sources: sources.into(),
      sources_content: Arc::from(sources_content.into()),
      names: names.into(),
      source_root: None,
      debug_id: None,
      ignore_list: None,
    }
  }

  /// Get the file field in [SourceMap].
  pub fn file(&self) -> Option<&str> {
    self.file.as_deref()
  }

  /// Set the file field in [SourceMap].
  pub fn set_file<T: Into<Arc<str>>>(&mut self, file: Option<T>) {
    self.file = file.map(Into::into);
  }

  /// Get the ignoreList field in [SourceMap].
  pub fn ignore_list(&self) -> Option<&[u32]> {
    self.ignore_list.as_deref().map(|v| &**v)
  }

  /// Set the ignoreList field in [SourceMap].
  pub fn set_ignore_list<T: Into<Vec<u32>>>(&mut self, ignore_list: Option<T>) {
    self.ignore_list = ignore_list.map(|v| Arc::new(v.into()));
  }

  /// Get the decoded mappings in [SourceMap].
  pub fn decoded_mappings(&self) -> impl Iterator<Item = Mapping> + '_ {
    decode_mappings(self)
  }

  /// Get the mappings string in [SourceMap].
  pub fn mappings(&self) -> &str {
    &self.mappings
  }

  /// Get the sources field in [SourceMap].
  pub fn sources(&self) -> &[String] {
    &self.sources
  }

  /// Set the sources field in [SourceMap].
  pub fn set_sources<T: Into<Arc<[String]>>>(&mut self, sources: T) {
    self.sources = sources.into();
  }

  /// Get the source by index from sources field in [SourceMap].
  pub fn get_source(&self, index: usize) -> Option<&str> {
    self.sources.get(index).map(|s| s.as_ref())
  }

  /// Get the sourcesContent field in [SourceMap].
  pub fn sources_content(&self) -> &[Arc<str>] {
    &self.sources_content
  }

  /// Set the sourcesContent field in [SourceMap].
  pub fn set_sources_content<T: Into<Vec<Arc<str>>>>(
    &mut self,
    sources_content: T,
  ) {
    self.sources_content = Arc::from(sources_content.into());
  }

  /// Get the source content by index from sourcesContent field in [SourceMap].
  pub fn get_source_content(&self, index: usize) -> Option<&Arc<str>> {
    self.sources_content.get(index)
  }

  /// Get the names field in [SourceMap].
  pub fn names(&self) -> &[String] {
    &self.names
  }

  /// Set the names field in [SourceMap].
  pub fn set_names<T: Into<Arc<[String]>>>(&mut self, names: T) {
    self.names = names.into();
  }

  /// Get the name by index from names field in [SourceMap].
  pub fn get_name(&self, index: usize) -> Option<&str> {
    self.names.get(index).map(|s| s.as_ref())
  }

  /// Get the source_root field in [SourceMap].
  pub fn source_root(&self) -> Option<&str> {
    self.source_root.as_deref()
  }

  /// Set the source_root field in [SourceMap].
  pub fn set_source_root<T: Into<Arc<str>>>(&mut self, source_root: Option<T>) {
    self.source_root = source_root.map(Into::into);
  }

  /// Set the debug_id field in [SourceMap].
  pub fn set_debug_id<T: Into<Arc<str>>>(&mut self, debug_id: Option<T>) {
    self.debug_id = debug_id.map(Into::into);
  }

  /// Get the debug_id field in [SourceMap].
  pub fn get_debug_id(&self) -> Option<&str> {
    self.debug_id.as_deref()
  }
}

#[derive(Debug, Default, Deserialize)]
struct RawSourceMap {
  pub file: Option<String>,
  pub sources: Option<Vec<Option<String>>>,
  #[serde(rename = "sourceRoot")]
  pub source_root: Option<String>,
  #[serde(rename = "sourcesContent")]
  pub sources_content: Option<Vec<Option<String>>>,
  pub names: Option<Vec<Option<String>>>,
  pub mappings: String,
  #[serde(rename = "debugId")]
  pub debug_id: Option<String>,
  #[serde(rename = "ignoreList")]
  pub ignore_list: Option<Vec<u32>>,
}

impl RawSourceMap {
  pub fn from_reader<R: std::io::Read>(r: R) -> Result<Self> {
    let raw: RawSourceMap = simd_json::serde::from_reader(r)?;
    Ok(raw)
  }

  pub fn from_slice(val: &[u8]) -> Result<Self> {
    let mut v = val.to_vec();
    let raw: RawSourceMap = simd_json::serde::from_slice(&mut v)?;
    Ok(raw)
  }

  pub fn from_json(val: &str) -> Result<Self> {
    let mut v = val.as_bytes().to_vec();
    let raw: RawSourceMap = simd_json::serde::from_slice(&mut v)?;
    Ok(raw)
  }
}

impl SourceMap {
  /// Create a [SourceMap] from json string.
  pub fn from_json(s: &str) -> Result<Self> {
    RawSourceMap::from_json(s)?.try_into()
  }

  /// Create a [SourceMap] from [&[u8]].
  pub fn from_slice(s: &[u8]) -> Result<Self> {
    RawSourceMap::from_slice(s)?.try_into()
  }

  /// Create a [SourceMap] from reader.
  pub fn from_reader<R: std::io::Read>(s: R) -> Result<Self> {
    RawSourceMap::from_reader(s)?.try_into()
  }

  /// Generate source map to a json string.
  pub fn to_json(&self) -> String {
    to_json(self)
  }
}

impl TryFrom<RawSourceMap> for SourceMap {
  type Error = crate::Error;

  fn try_from(raw: RawSourceMap) -> Result<Self> {
    let file = raw.file.map(Into::into);
    let mappings = raw.mappings.into();
    let sources = raw
      .sources
      .unwrap_or_default()
      .into_iter()
      .map(Option::unwrap_or_default)
      .collect::<Vec<_>>()
      .into();
    let sources_content = raw
      .sources_content
      .unwrap_or_default()
      .into_iter()
      .map(|source_content| Arc::from(source_content.unwrap_or_default()))
      .collect::<Vec<_>>()
      .into();
    let names = raw
      .names
      .unwrap_or_default()
      .into_iter()
      .map(Option::unwrap_or_default)
      .collect::<Vec<_>>()
      .into();
    let source_root = raw.source_root.map(Into::into);
    let debug_id = raw.debug_id.map(Into::into);
    let ignore_list = raw.ignore_list.map(Into::into);

    Ok(Self {
      version: 3,
      file,
      mappings,
      sources,
      sources_content,
      names,
      source_root,
      debug_id,
      ignore_list,
    })
  }
}

/// The offset of a section within the generated code.
///
/// Both `line` and `column` are 0-based, as specified by the
/// [Index Source Map](https://tc39.es/ecma426/#sec-index-source-map) format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Copy, Default)]
pub struct SectionOffset {
  /// 0-based line offset in the generated code.
  pub line: u32,
  /// 0-based column offset in the generated code.
  pub column: u32,
}

/// A section within an [IndexSourceMap], pairing an [offset](SectionOffset)
/// with a regular [SourceMap].
///
/// See [Index Source Map § Section](https://tc39.es/ecma426/#sec-index-source-map).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Section {
  /// The offset in the generated code where this section begins.
  pub offset: SectionOffset,
  /// The source map for this section.
  pub map: SourceMap,
}

/// An [Index Source Map](https://tc39.es/ecma426/#sec-index-source-map)
/// that represents concatenated source maps as a list of [Section]s.
///
/// Each section contains a regular [SourceMap] and an offset indicating
/// where that section starts in the generated output. This avoids the
/// need to merge mappings from multiple sources, improving performance
/// for concatenated sources like [ConcatSource](crate::ConcatSource).
///
/// Use [IndexSourceMap::to_source_map] to flatten it into a regular [SourceMap].
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct IndexSourceMap {
  version: u8,
  #[serde(skip_serializing_if = "Option::is_none")]
  file: Option<Arc<str>>,
  sections: Vec<Section>,
}

impl std::fmt::Debug for IndexSourceMap {
  fn fmt(
    &self,
    f: &mut std::fmt::Formatter<'_>,
  ) -> std::result::Result<(), std::fmt::Error> {
    write!(
      f,
      "IndexSourceMap {{ version: {}, file: {:?}, sections: {:?} }}",
      self.version, self.file, self.sections
    )
  }
}

impl Hash for IndexSourceMap {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.file.hash(state);
    self.sections.hash(state);
  }
}

impl IndexSourceMap {
  /// Create a new [IndexSourceMap] from a list of [Section]s.
  pub fn new(sections: Vec<Section>) -> Self {
    Self {
      version: 3,
      file: None,
      sections,
    }
  }

  /// Get the file field.
  pub fn file(&self) -> Option<&str> {
    self.file.as_deref()
  }

  /// Set the file field.
  pub fn set_file<T: Into<Arc<str>>>(&mut self, file: Option<T>) {
    self.file = file.map(Into::into);
  }

  /// Get the sections.
  pub fn sections(&self) -> &[Section] {
    &self.sections
  }

  /// Flatten this [IndexSourceMap] into a regular [SourceMap] by merging
  /// all sections, offsetting their mappings accordingly.
  pub fn to_source_map(&self) -> Option<SourceMap> {
    if self.sections.is_empty() {
      return None;
    }

    // Single section with zero offset — return its map directly.
    if self.sections.len() == 1 {
      let section = &self.sections[0];
      if section.offset.line == 0 && section.offset.column == 0 {
        let mut map = section.map.clone();
        if self.file.is_some() {
          map.set_file(self.file.clone());
        }
        return Some(map);
      }
    }

    let mut global_sources: Vec<String> = Vec::new();
    let mut global_sources_content: Vec<Arc<str>> = Vec::new();
    let mut global_names: Vec<String> = Vec::new();
    let mut source_mapping: std::collections::HashMap<String, u32> =
      std::collections::HashMap::new();
    let mut name_mapping: std::collections::HashMap<String, u32> =
      std::collections::HashMap::new();

    let mut all_mappings: Vec<Mapping> = Vec::new();

    for section in &self.sections {
      let map = &section.map;

      // Build local-to-global source index mapping
      let local_source_mapping: Vec<u32> = map
        .sources()
        .iter()
        .enumerate()
        .map(|(i, source)| {
          if let Some(&idx) = source_mapping.get(source) {
            // Update source content if we have better content
            if let Some(content) = map.get_source_content(i) {
              if (idx as usize) < global_sources_content.len()
                && global_sources_content[idx as usize].is_empty()
              {
                global_sources_content[idx as usize] = content.clone();
              }
            }
            idx
          } else {
            let idx = global_sources.len() as u32;
            source_mapping.insert(source.clone(), idx);
            global_sources.push(source.clone());
            global_sources_content
              .resize_with(global_sources.len(), || "".into());
            if let Some(content) = map.get_source_content(i) {
              global_sources_content[idx as usize] = content.clone();
            }
            idx
          }
        })
        .collect();

      // Build local-to-global name index mapping
      let local_name_mapping: Vec<u32> = map
        .names()
        .iter()
        .map(|name| {
          if let Some(&idx) = name_mapping.get(name) {
            idx
          } else {
            let idx = global_names.len() as u32;
            name_mapping.insert(name.clone(), idx);
            global_names.push(name.clone());
            idx
          }
        })
        .collect();

      // Decode, offset, and remap mappings
      for mapping in map.decoded_mappings() {
        // Offset the generated position.
        // generated_line is 1-based; section.offset.line is 0-based.
        let generated_line = mapping.generated_line + section.offset.line;
        let generated_column = if mapping.generated_line == 1 {
          mapping.generated_column + section.offset.column
        } else {
          mapping.generated_column
        };

        let original = mapping.original.map(|orig| OriginalLocation {
          source_index: *local_source_mapping
            .get(orig.source_index as usize)
            .unwrap_or(&orig.source_index),
          original_line: orig.original_line,
          original_column: orig.original_column,
          name_index: orig
            .name_index
            .map(|ni| *local_name_mapping.get(ni as usize).unwrap_or(&ni)),
        });

        all_mappings.push(Mapping {
          generated_line,
          generated_column,
          original,
        });
      }
    }

    if all_mappings.is_empty() {
      return None;
    }

    let mappings_str = encode_mappings(all_mappings.into_iter());
    let mut result = SourceMap::new(
      mappings_str,
      global_sources,
      global_sources_content,
      global_names,
    );
    if self.file.is_some() {
      result.set_file(self.file.clone());
    }
    Some(result)
  }

  /// Generate index source map to a JSON string.
  pub fn to_json(&self) -> Result<String> {
    let json = simd_json::serde::to_string(&self)?;
    Ok(json)
  }

  /// Generate index source map to writer.
  pub fn to_writer<W: std::io::Write>(self, w: W) -> Result<()> {
    simd_json::serde::to_writer(w, &self)?;
    Ok(())
  }
}

/// Represent a [Mapping] information of source map.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Mapping {
  /// Generated line.
  pub generated_line: u32,
  /// Generated column.
  pub generated_column: u32,
  /// Original position information.
  pub original: Option<OriginalLocation>,
}

/// Represent original position information of a [Mapping].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OriginalLocation {
  /// Source index.
  pub source_index: u32,
  /// Original line.
  pub original_line: u32,
  /// Original column.
  pub original_column: u32,
  /// Name index.
  pub name_index: Option<u32>,
}

/// An convenient way to create a [Mapping].
#[macro_export]
macro_rules! m {
  ($gl:expr, $gc:expr, $si:expr, $ol:expr, $oc:expr, $ni:expr) => {{
    let gl: i64 = $gl;
    let gc: i64 = $gc;
    let si: i64 = $si;
    let ol: i64 = $ol;
    let oc: i64 = $oc;
    let ni: i64 = $ni;
    $crate::Mapping {
      generated_line: gl as u32,
      generated_column: gc as u32,
      original: (si >= 0).then(|| $crate::OriginalLocation {
        source_index: si as u32,
        original_line: ol as u32,
        original_column: oc as u32,
        name_index: (ni >= 0).then(|| ni as u32),
      }),
    }
  }};
}

/// An convenient way to create [Mapping]s.
#[macro_export]
macro_rules! mappings {
  ($($mapping:expr),* $(,)?) => {
    ::std::vec![$({
      let mapping = $mapping;
      $crate::m![mapping[0], mapping[1], mapping[2], mapping[3], mapping[4], mapping[5]]
    }),*]
  };
}

#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use crate::{
    CachedSource, ConcatSource, ObjectPool, OriginalSource, RawBufferSource,
    RawStringSource, ReplaceSource, SourceMapSource, WithoutOriginalOptions,
  };

  use super::*;

  #[test]
  fn should_not_have_sources_content_field_when_it_is_empty() {
    let map = SourceMap::new(
      ";;",
      vec!["a.js".into()],
      vec!["".into(), "".into(), "".into()],
      vec!["".into(), "".into()],
    )
    .to_json();
    assert!(!map.contains("sourcesContent"));
  }

  #[test]
  fn hash_available() {
    let mut state = twox_hash::XxHash64::default();
    RawStringSource::from("a").hash(&mut state);
    OriginalSource::new("b", "").hash(&mut state);
    SourceMapSource::new(WithoutOriginalOptions {
      value: "c",
      name: "",
      source_map: SourceMap::from_json("{\"mappings\": \";\"}").unwrap(),
    })
    .hash(&mut state);
    ConcatSource::new([RawStringSource::from("d")]).hash(&mut state);
    CachedSource::new(RawStringSource::from("e")).hash(&mut state);
    ReplaceSource::new(RawStringSource::from("f")).hash(&mut state);
    RawStringSource::from("g").boxed().hash(&mut state);
    RawStringSource::from_static("a").hash(&mut state);
    RawBufferSource::from("a".as_bytes()).hash(&mut state);
    (&RawStringSource::from("h") as &dyn Source).hash(&mut state);
    ReplaceSource::new(RawStringSource::from("i").boxed()).hash(&mut state);
    assert_eq!(format!("{:x}", state.finish()), "eca744ab8681f278");
  }

  #[test]
  fn eq_available() {
    assert_eq!(RawStringSource::from("a"), RawStringSource::from("a"));
    assert_eq!(
      RawStringSource::from_static("a"),
      RawStringSource::from_static("a")
    );
    assert_eq!(
      RawBufferSource::from("a".as_bytes()),
      RawBufferSource::from("a".as_bytes())
    );
    assert_eq!(OriginalSource::new("b", ""), OriginalSource::new("b", ""));
    assert_eq!(
      SourceMapSource::new(WithoutOriginalOptions {
        value: "c",
        name: "",
        source_map: SourceMap::from_json("{\"mappings\": \";\"}").unwrap(),
      }),
      SourceMapSource::new(WithoutOriginalOptions {
        value: "c",
        name: "",
        source_map: SourceMap::from_json("{\"mappings\": \";\"}").unwrap(),
      })
    );
    assert_eq!(
      ConcatSource::new([RawStringSource::from("d")]),
      ConcatSource::new([RawStringSource::from("d")])
    );
    assert_eq!(
      CachedSource::new(RawStringSource::from("e")),
      CachedSource::new(RawStringSource::from("e"))
    );
    assert_eq!(
      ReplaceSource::new(RawStringSource::from("f")),
      ReplaceSource::new(RawStringSource::from("f"))
    );
    assert_eq!(
      &RawStringSource::from("g").boxed(),
      &RawStringSource::from("g").boxed()
    );
    assert_eq!(
      (&RawStringSource::from("h") as &dyn Source),
      (&RawStringSource::from("h") as &dyn Source)
    );
    assert_eq!(
      ReplaceSource::new(RawStringSource::from("i").boxed()),
      ReplaceSource::new(RawStringSource::from("i").boxed())
    );
    assert_eq!(
      CachedSource::new(RawStringSource::from("j").boxed()),
      CachedSource::new(RawStringSource::from("j").boxed())
    );
  }

  #[test]
  #[allow(suspicious_double_ref_op)]
  fn clone_available() {
    let a = RawStringSource::from("a");
    assert_eq!(a, a.clone());
    let b = OriginalSource::new("b", "");
    assert_eq!(b, b.clone());
    let c = SourceMapSource::new(WithoutOriginalOptions {
      value: "c",
      name: "",
      source_map: SourceMap::from_json("{\"mappings\": \";\"}").unwrap(),
    });
    assert_eq!(c, c.clone());
    let d = ConcatSource::new([RawStringSource::from("d")]);
    assert_eq!(d, d.clone());
    let e = CachedSource::new(RawStringSource::from("e"));
    assert_eq!(e, e.clone());
    let f = ReplaceSource::new(RawStringSource::from("f"));
    assert_eq!(f, f.clone());
    let g = RawStringSource::from("g").boxed();
    assert_eq!(&g, &g.clone());
    let h = &RawStringSource::from("h") as &dyn Source;
    assert_eq!(h, h);
    let i = ReplaceSource::new(RawStringSource::from("i").boxed());
    assert_eq!(i, i.clone());
    let j = CachedSource::new(RawStringSource::from("j").boxed());
    assert_eq!(j, j.clone());
    let k = RawStringSource::from_static("k");
    assert_eq!(k, k.clone());
    let l = RawBufferSource::from("l".as_bytes());
    assert_eq!(l, l.clone());
  }

  #[test]
  fn box_dyn_source_use_hashmap_available() {
    let mut map = HashMap::new();
    let a = RawStringSource::from("a").boxed();
    map.insert(a.clone(), a.clone());
    assert_eq!(map.get(&a).unwrap(), &a);
  }

  #[test]
  #[allow(suspicious_double_ref_op)]
  fn ref_dyn_source_use_hashmap_available() {
    let mut map = HashMap::new();
    let a = &RawStringSource::from("a") as &dyn Source;
    map.insert(a, a);
    assert_eq!(map.get(&a).unwrap(), &a);
  }

  #[test]
  fn to_writer() {
    let sources = ConcatSource::new([
      RawStringSource::from("a"),
      RawStringSource::from("b"),
    ]);
    let mut writer = std::io::BufWriter::new(Vec::new());
    let result = sources.to_writer(&mut writer);
    assert!(result.is_ok());
    assert_eq!(
      String::from_utf8(writer.into_inner().unwrap()).unwrap(),
      "ab"
    );
  }

  #[test]
  fn index_source_map_serialization() {
    let map = SourceMap::new(
      "AAAA;AACA",
      vec!["file.js".into()],
      vec!["line1\nline2\n".into()],
      vec![],
    );
    let index_map = IndexSourceMap::new(vec![Section {
      offset: SectionOffset { line: 0, column: 0 },
      map,
    }]);
    let json = index_map.to_json().unwrap();
    assert!(json.contains("\"version\":3"));
    assert!(json.contains("\"sections\""));
    assert!(json.contains("\"offset\""));
    assert!(json.contains("\"map\""));
  }

  #[test]
  fn index_source_map_to_source_map_single_section() {
    let map = SourceMap::new(
      "AAAA;AACA",
      vec!["file.js".into()],
      vec!["line1\nline2\n".into()],
      vec![],
    );
    let index_map = IndexSourceMap::new(vec![Section {
      offset: SectionOffset { line: 0, column: 0 },
      map: map.clone(),
    }]);
    let result = index_map.to_source_map().unwrap();
    assert_eq!(result, map);
  }

  #[test]
  fn index_source_map_to_source_map_empty_sections() {
    let index_map = IndexSourceMap::new(vec![]);
    assert!(index_map.to_source_map().is_none());
  }

  #[test]
  fn index_source_map_to_source_map_with_offset() {
    // First section at line 0, col 0
    let map1 = SourceMap::new(
      "AAAA",
      vec!["a.js".into()],
      vec!["hello\n".into()],
      vec![],
    );
    // Second section at line 1, col 0 (after the first line)
    let map2 = SourceMap::new(
      "AAAA",
      vec!["b.js".into()],
      vec!["world\n".into()],
      vec![],
    );
    let index_map = IndexSourceMap::new(vec![
      Section {
        offset: SectionOffset { line: 0, column: 0 },
        map: map1,
      },
      Section {
        offset: SectionOffset { line: 1, column: 0 },
        map: map2,
      },
    ]);
    let result = index_map.to_source_map().unwrap();
    assert_eq!(result.sources(), &["a.js".to_string(), "b.js".to_string()]);
    assert_eq!(
      result.sources_content(),
      &[Arc::from("hello\n"), Arc::from("world\n")]
    );
    // Verify mappings: first mapping at line 1 (1-based), second at line 2
    let mappings: Vec<Mapping> = result.decoded_mappings().collect();
    assert_eq!(mappings.len(), 2);
    assert_eq!(mappings[0].generated_line, 1);
    assert_eq!(mappings[0].generated_column, 0);
    assert_eq!(mappings[0].original.as_ref().unwrap().source_index, 0);
    assert_eq!(mappings[1].generated_line, 2);
    assert_eq!(mappings[1].generated_column, 0);
    assert_eq!(mappings[1].original.as_ref().unwrap().source_index, 1);
  }

  #[test]
  fn index_source_map_to_source_map_with_column_offset() {
    // First section at line 0, col 0
    let map1 =
      SourceMap::new("AAAA", vec!["a.js".into()], vec!["hello".into()], vec![]);
    // Second section at line 0, col 5 (same line, after "hello")
    let map2 =
      SourceMap::new("AAAA", vec!["b.js".into()], vec!["world".into()], vec![]);
    let index_map = IndexSourceMap::new(vec![
      Section {
        offset: SectionOffset { line: 0, column: 0 },
        map: map1,
      },
      Section {
        offset: SectionOffset { line: 0, column: 5 },
        map: map2,
      },
    ]);
    let result = index_map.to_source_map().unwrap();
    let mappings: Vec<Mapping> = result.decoded_mappings().collect();
    assert_eq!(mappings.len(), 2);
    assert_eq!(mappings[0].generated_line, 1);
    assert_eq!(mappings[0].generated_column, 0);
    assert_eq!(mappings[1].generated_line, 1);
    assert_eq!(mappings[1].generated_column, 5);
  }

  #[test]
  fn index_map_default_impl_wraps_map() {
    let source = OriginalSource::new("hello\nworld\n", "test.txt");
    let pool = ObjectPool::default();
    let options = MapOptions::default();
    let map = source.map(&pool, &options).unwrap();
    let index_map = source.index_map(&pool, &options).unwrap();

    assert_eq!(index_map.sections().len(), 1);
    assert_eq!(index_map.sections()[0].offset.line, 0);
    assert_eq!(index_map.sections()[0].offset.column, 0);
    assert_eq!(index_map.sections()[0].map, map);
  }

  #[test]
  fn index_map_returns_none_for_raw_source() {
    let source = RawStringSource::from("hello world");
    let pool = ObjectPool::default();
    let options = MapOptions::default();
    assert!(source.index_map(&pool, &options).is_none());
  }

  #[test]
  fn index_map_file_field_propagated() {
    let map =
      SourceMap::new("AAAA", vec!["a.js".into()], vec!["hello".into()], vec![]);
    let mut index_map = IndexSourceMap::new(vec![Section {
      offset: SectionOffset { line: 0, column: 0 },
      map,
    }]);
    index_map.set_file(Some("bundle.js"));
    assert_eq!(index_map.file(), Some("bundle.js"));

    let result = index_map.to_source_map().unwrap();
    assert_eq!(result.file(), Some("bundle.js"));
  }

  #[test]
  fn index_source_map_shared_sources_across_sections() {
    // Both sections reference the same source file
    let map1 = SourceMap::new(
      "AAAA",
      vec!["shared.js".into()],
      vec!["content".into()],
      vec![],
    );
    let map2 = SourceMap::new(
      "AAAA",
      vec!["shared.js".into()],
      vec!["content".into()],
      vec![],
    );
    let index_map = IndexSourceMap::new(vec![
      Section {
        offset: SectionOffset { line: 0, column: 0 },
        map: map1,
      },
      Section {
        offset: SectionOffset { line: 1, column: 0 },
        map: map2,
      },
    ]);
    let result = index_map.to_source_map().unwrap();
    // Should deduplicate sources
    assert_eq!(result.sources().len(), 1);
    assert_eq!(result.sources()[0], "shared.js");
    // Both mappings should reference source index 0
    let mappings: Vec<Mapping> = result.decoded_mappings().collect();
    assert_eq!(mappings[0].original.as_ref().unwrap().source_index, 0);
    assert_eq!(mappings[1].original.as_ref().unwrap().source_index, 0);
  }
}
