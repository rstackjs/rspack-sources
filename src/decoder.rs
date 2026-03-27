use crate::{Mapping, OriginalLocation};

const COM: u8 = 0x40; // END_SEGMENT_BIT
const SEM: u8 = COM | 0x01; // NEXT_LINE
const ERR: u8 = COM | 0x02; // INVALID

const CONTINUATION_BIT: u8 = 0x20;
const DATA_MASK: u8 = 0x1f;

#[rustfmt::skip]
const B64: [u8; 256] = [
//  0    1    2    3    4    5    6    7    8    9    A    B    C    D    E    F    //
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // 0
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // 1
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  62, COM, ERR, ERR,  63,  // 2
    52,  53,  54,  55,  56,  57,  58,  59,  60,  61, ERR, SEM, ERR, ERR, ERR, ERR,  // 3
   ERR,   0,   1,   2,   3,   4,   5,   6,   7,   8,   9,  10,  11,  12,  13,  14,  // 4
    15,  16,  17,  18,  19,  20,  21,  22,  23,  24,  25, ERR, ERR, ERR, ERR, ERR,  // 5
   ERR,  26,  27,  28,  29,  30,  31,  32,  33,  34,  35,  36,  37,  38,  39,  40,  // 6
    41,  42,  43,  44,  45,  46,  47,  48,  49,  50,  51, ERR, ERR, ERR, ERR, ERR,  // 7
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // 8
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // 9
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // A
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // B
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // C
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // D
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // E
   ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR,  // F
];

pub(crate) struct MappingsDecoder<'a> {
  mappings: &'a [u8],
  index: usize,

  current_data: [u32; 5],
  current_data_pos: usize,
  // current_value will include a sign bit at bit 0
  current_value: u32,
  current_value_pos: usize,
  generated_line: u32,
}

impl<'a> MappingsDecoder<'a> {
  pub fn new(mappings: &'a str) -> Self {
    Self {
      mappings: mappings.as_bytes(),
      index: 0,
      current_data: [0u32, 0u32, 1u32, 0u32, 0u32],
      current_data_pos: 0,
      // current_value will include a sign bit at bit 0
      current_value: 0,
      current_value_pos: 0,
      generated_line: 1,
    }
  }

  #[inline]
  fn emit_segment(
    &self,
    generated_line: u32,
    generated_column: u32,
    current_data_pos: usize,
  ) -> Option<Mapping> {
    match current_data_pos {
      1 => Some(Mapping {
        generated_line,
        generated_column,
        original: None,
      }),
      4 => Some(Mapping {
        generated_line,
        generated_column,
        original: Some(OriginalLocation {
          source_index: self.current_data[1],
          original_line: self.current_data[2],
          original_column: self.current_data[3],
          name_index: None,
        }),
      }),
      5 => Some(Mapping {
        generated_line,
        generated_column,
        original: Some(OriginalLocation {
          source_index: self.current_data[1],
          original_line: self.current_data[2],
          original_column: self.current_data[3],
          name_index: Some(self.current_data[4]),
        }),
      }),
      _ => None,
    }
  }

  #[allow(unsafe_code)]
  pub fn decode_into(self, mut on_mapping: impl FnMut(Mapping)) {
    let mut this = self;

    while this.index < this.mappings.len() {
      let byte = unsafe { *this.mappings.get_unchecked(this.index) };
      let value = unsafe { *B64.get_unchecked(byte as usize) };
      this.index += 1;

      if value < COM {
        if (value & CONTINUATION_BIT) == 0 {
          this.current_value |= (value as u32) << this.current_value_pos;
          let final_value = if (this.current_value & 1) != 0 {
            -((this.current_value >> 1) as i64)
          } else {
            (this.current_value >> 1) as i64
          };
          if this.current_data_pos < 5 {
            this.current_data[this.current_data_pos] =
              (this.current_data[this.current_data_pos] as i64 + final_value)
                as u32;
          }
          this.current_data_pos += 1;
          this.current_value_pos = 0;
          this.current_value = 0;
        } else {
          this.current_value |=
            ((value & DATA_MASK) as u32) << this.current_value_pos;
          this.current_value_pos += 5;
        }
        continue;
      }

      if value == ERR {
        continue;
      }

      let generated_line = this.generated_line;
      let generated_column = this.current_data[0];
      let current_data_pos = this.current_data_pos;
      this.current_data_pos = 0;
      if value == SEM {
        this.generated_line += 1;
        this.current_data[0] = 0;
      }
      if let Some(mapping) =
        this.emit_segment(generated_line, generated_column, current_data_pos)
      {
        on_mapping(mapping);
      }
    }

    let current_data_pos = this.current_data_pos;
    this.current_data_pos = 0;
    if let Some(mapping) = this.emit_segment(
      this.generated_line,
      this.current_data[0],
      current_data_pos,
    ) {
      on_mapping(mapping);
    }
  }
}
