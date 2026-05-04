use std::path::Path;

pub(crate) fn default_output_path(input_srt: &str) -> String {
    let path = Path::new(input_srt);
    path.with_extension("eldr").to_string_lossy().into_owned()
}
