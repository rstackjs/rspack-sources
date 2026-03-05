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
  if sourcemap.file().is_some()  {
    max_segments += 8 /* "file":" */ + 2 /* ", */;
  }

  // Optional "sourceRoot":"...",
  if sourcemap.source_root().is_some() {
    max_segments += 14 /* "sourceRoot":" */ + 2 /* ", */;
  }

  // Calculate string lengths in a single pass for better cache locality
  let names_count = sourcemap.names().len();
  let sources_count = sourcemap.sources().len();

  let has_sources_content = !sourcemap.sources_content().is_empty()
    && !sourcemap.sources_content().iter().all(|s| s.is_empty());
  let sc_count = if has_sources_content {
    sourcemap.sources_content().len()
  } else {
    0
  };

  // Accumulate total string bytes across all collections
  let mut total_string_bytes = 0usize;

  if let Some(source_root) = sourcemap.source_root() {
    total_string_bytes += source_root.len();
  }

  if let Some(file) = sourcemap.file() {
    total_string_bytes += file.len();
  }

  for name in sourcemap.names() {
    total_string_bytes += name.len();
  }

  for source in sourcemap.sources() {
    total_string_bytes += source.len();
  }

  if has_sources_content {
    for content in sourcemap.sources_content() {
      total_string_bytes += content.len();
    }
  }

  // Calculate total capacity needed
  // JSON structure overhead
  max_segments += 9; // "sources":[
  max_segments += 10; // ],"names":[
  max_segments += 14; // ],"mappings":"
  max_segments += 1; // closing "
  if has_sources_content {
    max_segments += 20; // ],"sourcesContent":[
  }

  // Worst-case escaping for strings (control chars -> \\u0000)
  max_segments += 6 * total_string_bytes;

  // Quotes around each string item
  max_segments += 2 * (names_count + sources_count + sc_count);

  // Commas between array items
  max_segments += names_count.saturating_sub(1)
    + sources_count.saturating_sub(1)
    + sc_count.saturating_sub(1);

  // Optional ignoreList field
  if let Some(ignore_list) = sourcemap.ignore_list() {
    max_segments += 15; // ,"ignoreList":[
    max_segments += 1; // ]
    let ig_count = ignore_list.len();
    // Estimate 10 digits per number (max u32: 4,294,967,295)
    max_segments += 10 * ig_count + ig_count.saturating_sub(1);
  }

  // ],"mappings":"
  max_segments += 14;
  max_segments += sourcemap.mappings().len();
  max_segments += 1; // closing "

  // Optional ,"debugId":"..."
  if let Some(debug_id) = sourcemap.get_debug_id() {
    max_segments += 12; // ,"debugId":"
    max_segments += debug_id.len();
    max_segments += 1; // closing "
  }

  // }
  max_segments += 1;
  let mut contents = PreAllocatedString::new(max_segments);

  contents.push("{\"version\":3,");
  if let Some(file) = sourcemap.file() {
    contents.push("\"file\":\"");
    escape_into(file, contents.as_mut_vec());
    contents.push("\",");
  }

  if let Some(source_root) = sourcemap.source_root() {
    contents.push("\"sourceRoot\":\"");
    escape_into(source_root, contents.as_mut_vec());
    contents.push("\",");
  }

  contents.push("\"sources\":[");
  contents.push_list(sourcemap.sources().iter(), escape_into);

  if has_sources_content {
    contents.push("],\"sourcesContent\":[");
    contents.push_list(sourcemap.sources_content().iter(), escape_into);
  }

  contents.push("],\"names\":[");
  contents.push_list(sourcemap.names().iter(), escape_into);

  contents.push("],\"mappings\":\"");
  contents.push_str(sourcemap.mappings());
  contents.push("\"");

  if let Some(ignore_list) = &sourcemap.ignore_list() {
    contents.push(",\"ignoreList\":[");
    contents.push_list(ignore_list.iter(), |s, output| {
      output.extend_from_slice(s.to_string().as_bytes());
    });
    contents.push("]");
  }

  if let Some(debug_id) = sourcemap.get_debug_id() {
    contents.push(",\"debugId\":\"");
    contents.push(debug_id);
    contents.push("\"");
  }

  contents.push("}");

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
  fn push_str(&mut self, s: &str) {
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

    for item in iter {
      self.0.push(',');
      encode(item, self.as_mut_vec());
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
