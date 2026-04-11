use sindr::viewer::viewer_schema;

fn main() {
    let schema = viewer_schema();
    println!(
        "{}",
        serde_json::to_string_pretty(&schema).expect("viewer schema must serialize")
    );
}
