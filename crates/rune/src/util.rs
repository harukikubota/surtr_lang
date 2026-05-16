use std::path::Path;

use serde_json::Value as JsonValue;

pub(crate) fn default_output_path(input_srt: &str) -> String {
    let path = Path::new(input_srt);
    path.with_extension("eldr").to_string_lossy().into_owned()
}

pub(crate) fn surface_strip_global_prefixes(value: &mut JsonValue) {
    match value {
        JsonValue::String(text) => {
            if let Some(stripped) = text.strip_prefix("Global::") {
                *text = stripped.to_string();
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                surface_strip_global_prefixes(item);
            }
        }
        JsonValue::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let Some(mut item) = map.remove(&key) else {
                    continue;
                };
                surface_strip_global_prefixes(&mut item);
                let surface_key = key
                    .strip_prefix("Global::")
                    .unwrap_or(key.as_str())
                    .to_string();
                map.insert(surface_key, item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn surface_strip_global_prefixes_rewrites_json_strings_and_keys() {
        let mut value = json!({
            "Global::User": {
                "type": "Global::Profile",
                "items": ["Global::One", "Plain"]
            }
        });

        surface_strip_global_prefixes(&mut value);

        assert_eq!(
            value,
            json!({
                "User": {
                    "type": "Profile",
                    "items": ["One", "Plain"]
                }
            })
        );
    }
}
