use std::ops::{Deref, DerefMut};

use crate::SourceMap;
use json_escape_simd::escape_into;

pub fn to_json(sourcemap: &SourceMap) -> String {
  // Worst-case capacity accounting:
  // - escape_into may write up to (len * 2 + 2) for each string
  // - include commas between items and constant JSON punctuation/keys
  let mut max_segments = 0usize;

  // {"version":3,
  max_segments += 13;

  // Optional "file":"...",
  if let Some(file) = sourcemap.file() {
    max_segments += 8 /* "file":" */ + file.len() + 2 /* ", */;
  }

  // Optional "sourceRoot":"...",
  if let Some(source_root) = sourcemap.source_root() {
    max_segments += 14 /* "sourceRoot":" */ + source_root.len() + 2 /* ", */;
  }

  // Calculate string lengths in a single pass for better cache locality
  let names_count = sourcemap.names().len();
  let sources_count = sourcemap.sources().len();

  let should_skip_sources_content = sourcemap.sources_content().is_empty()
    || sourcemap.sources_content().iter().all(|s| s.is_empty());
  let sc_count = if should_skip_sources_content {
    0
  } else {
    sourcemap.sources_content().len()
  };

  // Accumulate total string bytes across all collections
  let mut total_string_bytes = 0usize;

  for name in sourcemap.names() {
    total_string_bytes += name.len();
  }

  for source in sourcemap.sources() {
    total_string_bytes += source.len();
  }

  if !should_skip_sources_content {
    for content in sourcemap.sources_content() {
      total_string_bytes += content.len();
    }
  }

  // Calculate total capacity needed
  max_segments += 9 + 13 + 20; // "names":[ + ],"sources":[ + ],"sourcesContent":[
  max_segments += 6 * total_string_bytes; // worst-case escaping (* 6), \0 -> \\u0000
  max_segments += 2 * (names_count + sources_count + sc_count); // quotes around each item

  // Commas between array items
  let comma_count = names_count.saturating_sub(1)
    + sources_count.saturating_sub(1)
    + sc_count.saturating_sub(1);
  max_segments += comma_count;

  // Optional ],"ignoreList":[
  if let Some(ignore_list) = sourcemap.ignore_list() {
    max_segments += 16; // ],"ignoreList":[

    let ig_count = ignore_list.len();
    // guess 10 digits per item, 100_000_000 maximum per element
    max_segments += 10 * ig_count;
  }

  // ],"mappings":"
  max_segments += 14;
  max_segments += sourcemap.mappings().len();

  // Optional ,"debugId":"..."
  if let Some(debug_id) = sourcemap.get_debug_id() {
    max_segments += 13 /* ,"debugId":" */ + debug_id.len();
  }

  // "}
  max_segments += 2;
  let mut contents = PreAllocatedString::new(max_segments);

  contents.push("{\"version\":3,");
  if let Some(file) = sourcemap.file() {
    contents.push("\"file\":\"");
    contents.push(file.as_ref());
    contents.push("\",");
  }

  if let Some(source_root) = sourcemap.source_root() {
    contents.push("\"sourceRoot\":\"");
    contents.push(source_root);
    contents.push("\",");
  }

  contents.push("\"sources\":[");
  contents.push_list(sourcemap.sources().iter(), escape_into);

  if !should_skip_sources_content {
    contents.push("],\"sourcesContent\":[");
    contents.push_list(sourcemap.sources_content().iter(), escape_into);
  }

  if let Some(ignore_list) = &sourcemap.ignore_list() {
    contents.push("],\"ignoreList\":[");
    contents.push_list(ignore_list.iter(), |s, output| {
      output.extend_from_slice(s.to_string().as_bytes());
    });
  }

  contents.push("],\"names\":[");
  contents.push_list(sourcemap.names().iter(), escape_into);

  contents.push("],\"mappings\":\"");
  contents.push_str(sourcemap.mappings());

  if let Some(debug_id) = sourcemap.get_debug_id() {
    contents.push("\",\"debugId\":\"");
    contents.push(debug_id);
  }

  contents.push("\"}");

  // Check we calculated number of segments required correctly
  debug_assert!(contents.len() <= max_segments);

  contents.consume()
}

/// A helper for pre-allocate string buffer.
///
/// Pre-allocate a Cow<'a, str> buffer, and push the segment into it.
/// Finally, convert it to a pre-allocated length String.
#[repr(transparent)]
struct PreAllocatedString(String);

impl Deref for PreAllocatedString {
  type Target = String;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for PreAllocatedString {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

impl PreAllocatedString {
  fn new(max_segments: usize) -> Self {
    Self(String::with_capacity(max_segments))
  }

  #[inline]
  fn push(&mut self, s: &str) {
    self.0.push_str(s);
  }

  #[inline]
  fn push_list<S, I>(&mut self, mut iter: I, encode: impl Fn(S, &mut Vec<u8>))
  where
    I: Iterator<Item = S>,
  {
    let Some(first) = iter.next() else {
      return;
    };
    encode(first, self.as_mut_vec());

    for other in iter {
      self.0.push(',');
      encode(other, self.as_mut_vec());
    }
  }

  #[allow(unsafe_code)]
  fn as_mut_vec(&mut self) -> &mut Vec<u8> {
    // SAFETY: we are sure that the string is not shared
    unsafe { self.0.as_mut_vec() }
  }

  #[inline]
  fn consume(self) -> String {
    self.0
  }
}
