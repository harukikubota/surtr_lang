use std::path::Path;

pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn char_to_byte_index(source: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    source
        .char_indices()
        .nth(char_index)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(source.len())
}

pub(crate) fn slice_by_char_range(source: &str, start: usize, end: usize) -> &str {
    let byte_start = char_to_byte_index(source, start);
    let byte_end = char_to_byte_index(source, end);
    &source[byte_start..byte_end]
}

pub(crate) fn line_column_for_char_offset(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;
    for (idx, ch) in source.chars().enumerate() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

pub(crate) fn default_output_path(input_srt: &str) -> String {
    let path = Path::new(input_srt);
    path.with_extension("eldr").to_string_lossy().into_owned()
}
