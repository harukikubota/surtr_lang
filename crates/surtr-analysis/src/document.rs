#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utf16Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone)]
pub struct LineIndex {
    source: String,
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(idx + ch.len_utf8());
            }
        }

        Self {
            source: source.to_string(),
            line_starts,
        }
    }

    pub fn byte_to_text_position(&self, byte_offset: usize) -> TextPosition {
        let byte_offset = self.clamp_to_char_boundary(byte_offset);
        let line_idx = self.line_index_for_byte(byte_offset);
        let line_start = self.line_starts[line_idx];
        let character = self.source[line_start..byte_offset].chars().count() as u32;

        TextPosition {
            line: line_idx as u32,
            character,
        }
    }

    pub fn byte_to_utf16_position(&self, byte_offset: usize) -> Utf16Position {
        let byte_offset = self.clamp_to_char_boundary(byte_offset);
        let line_idx = self.line_index_for_byte(byte_offset);
        let line_start = self.line_starts[line_idx];
        let character = self.source[line_start..byte_offset]
            .chars()
            .map(char::len_utf16)
            .sum::<usize>() as u32;

        Utf16Position {
            line: line_idx as u32,
            character,
        }
    }

    pub fn utf16_position_to_byte(&self, position: Utf16Position) -> Option<usize> {
        let line_idx = position.line as usize;
        let line_start = *self.line_starts.get(line_idx)?;
        let line_end = self
            .line_starts
            .get(line_idx + 1)
            .copied()
            .unwrap_or(self.source.len());
        let mut utf16_units = 0u32;

        if position.character == 0 {
            return Some(line_start);
        }

        for (idx, ch) in self.source[line_start..line_end].char_indices() {
            if ch == '\n' {
                break;
            }
            utf16_units += ch.len_utf16() as u32;
            if utf16_units == position.character {
                return Some(line_start + idx + ch.len_utf8());
            }
            if utf16_units > position.character {
                return None;
            }
        }

        None
    }

    fn clamp_to_char_boundary(&self, byte_offset: usize) -> usize {
        let mut byte_offset = byte_offset.min(self.source.len());
        while !self.source.is_char_boundary(byte_offset) {
            byte_offset -= 1;
        }
        byte_offset
    }

    fn line_index_for_byte(&self, byte_offset: usize) -> usize {
        match self.line_starts.binary_search(&byte_offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        }
    }
}
