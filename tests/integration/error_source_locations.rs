use serde_json::Value;
use std::fs;

use crate::common::{repo_root, surtr_command, unique_temp_dir, write_source};

#[test]
fn process_stopped_reports_rejected_request_origin_through_callable_routes() {
    let definitions = r#"
defgenserver Worker {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }
  @init
  def init(seed: Int) -> Result<Int> { Ok(seed) }
  @call
  def stop(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Stop(StopReply::Normal(state)))
  }
  @call
  def value(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}
def invoke(pid: PID<Worker>, message: (PID<Worker> -> Result<Int>)) -> Result<Int> {
  message(pid)
}
"#;
    for (route, origin) in [
        ("Worker::value(pid)", "Worker::value(pid)"),
        ("captured = Worker::value()\ncaptured(pid)", "captured(pid)"),
        ("invoke(pid, Worker::value())", "message(pid)"),
    ] {
        let source =
            format!("{definitions}\npid =? Worker::init(7)\n_ =? Worker::stop(pid)\n{route}\n");
        assert_error_source_location(&source, origin, "ProcessStopped");
    }
    let pool = r#"
defsupervisor Root {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Transient
    allow_adopt: True
  }
}
defgenserver Pool {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Workers<Worker>
  }
  @init
  def init() -> Result<Workers<Worker>> {
    Root::workers(Worker::init(7), WorkerStrategy::fixed(1))
  }
  @call
  def rejected(workers: Workers<Worker>) -> Result<CallResult<Int, Workers<Worker>>> {
    lease =? Workers::reserve(workers)
    _ =? Worker::stop(lease)
    reply =? Worker::value(lease)
    Ok(CallResult::Reply(reply, workers))
  }
}
supervisor_init { Root {} Pool {} }
Pool::rejected()
"#;
    assert_error_source_location(
        &format!("{definitions}{pool}"),
        "Worker::value(lease)",
        "ProcessStopped",
    );
}

#[test]
fn workers_unavailable_reports_selection_request_origin() {
    let source = r#"
defgenserver Worker {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }
  @init
  def init() -> Result<Int> { Ok(0) }
  @call
  def value(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}
defsupervisor Root {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Transient
    allow_adopt: True
  }
}
defgenserver EmptyPool {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Workers<Worker>
  }
  @init
  def init() -> Result<Workers<Worker>> {
    Root::workers(Worker::init(), WorkerStrategy::fixed(0))
  }
  @call
  def reserve(workers: Workers<Worker>) -> Result<CallResult<Unit, Workers<Worker>>> {
    _ =? Workers::reserve(workers)
    Ok(CallResult::Reply((), workers))
  }
}
supervisor_init { Root {} EmptyPool {} }
EmptyPool::reserve()
"#;
    assert_error_source_location(source, "Workers::reserve(workers)", "WorkersUnavailable");
}

/// Compare structured offsets and the human caption without depending on Ariadne's layout.
fn assert_error_source_location(input: &str, origin: &str, kind: &str) -> Value {
    assert_error_source_location_with_bytecode(input, origin, kind, false)
}

fn assert_error_source_location_with_bytecode(
    input: &str,
    origin: &str,
    kind: &str,
    roundtrip: bool,
) -> Value {
    let temp = unique_temp_dir("error_source_location");
    let source_path = temp.join("origin.srt");
    let dump_path = temp.join("dump.json");
    let (source, definitions) = if input.contains("defmod ") {
        let split = input
            .find("def main()")
            .expect("module case must have main");
        let definitions = input[..split].to_string();
        write_source(&temp.join("definitions.srt"), &definitions);
        (
            format!("include \"./definitions.srt\"\n{}", &input[split..]),
            Some(definitions),
        )
    } else {
        (input.to_string(), None)
    };
    write_source(&source_path, &source);
    let run_path = if roundtrip {
        let bytecode_path = temp.join("origin.eldr");
        let build = surtr_command()
            .arg("build")
            .arg(&source_path)
            .arg(&bytecode_path)
            .output()
            .expect("script must build");
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        bytecode_path
    } else {
        source_path.clone()
    };
    let output = surtr_command()
        .arg("run")
        .arg(&run_path)
        .arg("--vm-dump")
        .arg(&dump_path)
        .output()
        .expect("script must run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected Err result: {source}");
    let dump: Value = serde_json::from_slice(&fs::read(&dump_path).unwrap_or_else(|error| {
        panic!("script must compile and dump a language Error: {error}\n{stderr}\n{source}")
    }))
    .expect("dump must be JSON");
    assert_eq!(dump["result"]["status"], "result_err", "{dump}\n{source}");
    assert_eq!(dump["result"]["error"]["kind"], kind, "{dump}\n{source}");
    let (origin_source, origin_file, origin, unique) = if origin == "stdlib_parse_error" {
        (
            fs::read_to_string(repo_root().join("lib/types/int.srt")).unwrap(),
            "types/int.srt".to_string(),
            "IntParseInvalidDigit(base, ch, index)",
            false,
        )
    } else if definitions
        .as_ref()
        .is_some_and(|definitions| definitions.contains(origin))
    {
        (
            definitions.unwrap(),
            temp.join("definitions.srt").to_string_lossy().into_owned(),
            origin,
            true,
        )
    } else {
        (
            source.clone(),
            source_path.to_string_lossy().into_owned(),
            origin,
            true,
        )
    };
    if unique {
        assert_eq!(
            origin_source.matches(origin).count(),
            1,
            "origin must be unique"
        );
    }
    let byte_start = origin_source.find(origin).expect("origin must exist");
    let prefix = &origin_source[..byte_start];
    let start = prefix.chars().count();
    let end = start + origin.chars().count();
    let line = prefix.chars().filter(|ch| *ch == '\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
    let location = &dump["result"]["error"]["location"];
    assert_eq!(
        location["span"],
        serde_json::json!([start, end]),
        "{dump}\n{source}"
    );
    assert_eq!(location["line"], line, "{dump}\n{source}");
    assert_eq!(location["column"], column, "{dump}\n{source}");
    let actual_file = location["file"]
        .as_str()
        .expect("generation file must exist");
    if origin_file == "types/int.srt" {
        assert_eq!(actual_file, origin_file, "{dump}");
    } else {
        assert_eq!(
            fs::canonicalize(actual_file).expect("generation file must exist"),
            fs::canonicalize(&origin_file).expect("expected source file must exist"),
            "{dump}"
        );
    }
    let origin_name = std::path::Path::new(actual_file)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        stderr.contains(&format!("{origin_name}:{line}:{column}")),
        "human caption must agree with structured location: {stderr}\n{source}"
    );
    fs::remove_dir_all(temp).unwrap();
    dump
}

#[test]
fn error_source_location_roundtrip_retains_included_generation_site_and_call_trace() {
    let source = "deferror Rejected { |message: String| message }\ndefmod E {\n  defextractor checked(value: Int) -> MatchResult<Int> {\n    MatchResult::Err(Rejected(\"roundtrip\"))\n  }\n}\ndef main() -> Result<Int> {\n  E::checked(found) =? Ok(2)\n  Ok(found)\n}\nmain()\n";
    let dump = assert_error_source_location_with_bytecode(
        source,
        "Rejected(\"roundtrip\")",
        "Rejected",
        true,
    );
    let trace = dump["result"]["error"]["stack_trace"]
        .as_array()
        .expect("call trace must exist");
    assert!(
        trace.iter().any(|frame| frame["function"] == "main"
            && frame["location"]["file"]
                .as_str()
                .is_some_and(|file| file.ends_with("origin.srt"))),
        "{dump}"
    );
}

#[test]
fn error_source_location_safebind_selects_the_failing_pattern_node() {
    for (setup, binding, origin, kind) in [
        (
            "",
            "hash![\"欠落\" => _] =? hash![\"a\" => 1]",
            "\"欠落\"",
            "HashMapKeyMissing",
        ),
        (
            "key = \"absent\"\n  ",
            "hash![^key => _] =? hash![\"a\" => 1]",
            "^key",
            "HashMapKeyMissing",
        ),
        (
            "inner: HashMap<Int> = hash![]\n  ",
            "hash![\"a\" => hash![\"nested\" => _]] =? hash![\"a\" => inner]",
            "\"nested\"",
            "HashMapKeyMissing",
        ),
        (
            "",
            "hash![\"a\" => 11] =? hash![\"a\" => 2]",
            "11",
            "IntLiteralPatternMismatch",
        ),
        ("", "1 =? 2", "1", "IntLiteralPatternMismatch"),
        ("", "(x, 11) =? (3, 2)", "11", "IntLiteralPatternMismatch"),
        (
            "",
            "(\"あ\", 11) =? (\"あ\", 2)",
            "11",
            "IntLiteralPatternMismatch",
        ),
        ("", "[[x]] =? [[]]", "[x]", "ListPatternTooShort"),
        (
            "empty: Option<Int> = Option::None\n  ",
            "Option::Some(x) =? empty",
            "Option::Some(x)",
            "EnumVariantPatternMismatch",
        ),
        (
            "",
            "Option::Some(11) =? Option::Some(2)",
            "11",
            "IntLiteralPatternMismatch",
        ),
        (
            "expected = 11\n  ",
            "^expected =? 2",
            "^expected",
            "PinnedValuePatternMismatch",
        ),
        (
            "",
            "(x, 11) @ whole =? (3, 2)",
            "11",
            "IntLiteralPatternMismatch",
        ),
        (
            "empty: List<Int> = []\n  ",
            "[head, ..tail] =? empty",
            "[head, ..tail]",
            "EmptyHeadTailListPattern",
        ),
        (
            "",
            "[first, ..rest] =? \"\"",
            "[first, ..rest]",
            "UnconsEmptyString",
        ),
        ("", "[x, y] =? [2]", "[x, y]", "ListPatternTooShort"),
        (
            "",
            "11 =? Int::parse(\"2\")",
            "11",
            "IntLiteralPatternMismatch",
        ),
    ] {
        let source =
            format!("def main() -> Result<Int> {{\n  {setup}{binding}\n  Ok(0)\n}}\nmain()\n");
        assert_error_source_location(&source, origin, kind);
    }
}

#[test]
fn error_source_location_keeps_generation_site_across_result_and_extractor_propagation() {
    let cases = [
        ("def main() -> Result<Int> {\n  1 =? Err(NoneError())\n  Ok(0)\n}\nmain()\n", "NoneError()", "NoneError"),
        ("def main() -> Result<Int> {\n  value =? Int::parse(\"a\")\n  Ok(value)\n}\nmain()\n", "stdlib_parse_error", "IntParseInvalidDigit"),
        ("deferror Rejected { |message: String| message }\ndef source() -> Result<Int> {\n  Err(Rejected(\"generated\"))\n}\ndef main() -> Result<Int> {\n  value =? source()\n  Ok(value)\n}\nmain()\n", "Rejected(\"generated\")" , "Rejected"),
        ("deferror Rejected { |message: String| message }\ndef source() -> Result<Int> {\n  Err(Rejected(\"generated\"))\n}\ndef main() -> Result<Int> {\n  source()\n}\nmain()\n", "Rejected(\"generated\")", "Rejected"),
        ("deferror Rejected { |message: String| message }\ndefmod E {\n  defextractor reject(value: Int) -> MatchResult<Int> {\n    MatchResult::Err(Rejected(\"generated\"))\n  }\n  defextractor outer(value: Int) -> MatchResult<Int> {\n    reject(found) =? Ok(value)\n    MatchResult::Ok(found)\n  }\n}\ndef main() -> Result<Int> {\n  E::outer(found) =? Ok(2)\n  Ok(found)\n}\nmain()\n", "Rejected(\"generated\")", "Rejected"),
        ("defmod E {\n  defextractor check(value: Int) -> MatchResult<Int> {\n    11 =? value\n    MatchResult::Ok(value)\n  }\n}\ndef main() -> Result<Int> {\n  E::check(found) =? Ok(2)\n  Ok(found)\n}\nmain()\n", "11", "IntLiteralPatternMismatch"),
        ("defmod E {\n  defextractor identity(value: Int) -> MatchResult<Int> {\n    MatchResult::Ok(value)\n  }\n}\ndef main() -> Result<Int> {\n  E::identity(11) =? Ok(2)\n  Ok(0)\n}\nmain()\n", "11", "IntLiteralPatternMismatch"),
        ("def main() -> Result<Int> {\n  ext: ExtractorClosure<(Int -> MatchResult<Int>)> = *{|value: Int|\n    11 =? value\n    MatchResult::Ok(value)\n  }\n  ext(found) =? Ok(2)\n  Ok(found)\n}\nmain()\n", "11", "IntLiteralPatternMismatch"),
        ("deferror Rejected { |message: String| message }\ndef main() -> Result<Int> {\n  ext: ExtractorClosure<(Int -> MatchResult<Int>)> = *{|value: Int| MatchResult::Err(Rejected(\"closure\"))}\n  ext(found) =? Ok(2)\n  Ok(found)\n}\nmain()\n", "Rejected(\"closure\")", "Rejected"),
        ("def main() -> Result<Int> {\n  _ =? apply_pattern((2, 3), (_, 11))\n  Ok(0)\n}\nmain()\n", "11", "IntLiteralPatternMismatch"),
    ];
    for (source, origin, kind) in cases {
        assert_error_source_location(source, origin, kind);
    }
}

#[test]
fn error_source_location_partial_bind_selects_the_failing_child_in_result_context() {
    for body in [
        "do::<Result> {\n    (x, 11) <- Ok((3, 2))\n    Ok(x)\n  }",
        "value =? Identity::run(ResultT::run(do::<ResultT<Identity, _>> {\n    (x, 11) <- ResultT::ok::<Identity>((3, 2))\n    ResultT::ok::<Identity>(x)\n  }))\n  Ok(value)",
    ] {
        let source = format!("def main() -> Result<Int> {{\n  {body}\n}}\nmain()\n");
        assert_error_source_location(&source, "11", "IntLiteralPatternMismatch");
    }
}

#[test]
fn error_source_location_statement_question_preserves_error_and_cause() {
    let definitions = "deferror Inner { \"inner\" }\ndeferror Outer { |message: String| message }\ndefmod E {\n  def source() -> Result<()> {\n    Result::cause(Err(Inner()), Outer(\"wrapped\"))\n  }\n}\n";
    for body in [
        "E::source()?\n  Ok(0)",
        "match True { True => { E::source()?\n    () }, False => () }\n  Ok(0)",
        "do::<Result> {\n    E::source()?\n    Ok(0)\n  }",
        "inner: (-> Result<Int>) = {|| E::source()?\n    Ok(0) }\n  inner()",
    ] {
        let source = format!("{definitions}def main() -> Result<Int> {{\n  {body}\n}}\nmain()\n");
        let dump = assert_error_source_location(&source, "Outer(\"wrapped\")", "Outer");
        assert_eq!(dump["result"]["error"]["message"], "wrapped");
        assert_eq!(
            dump["result"]["last_value"],
            "Err(Outer(\"wrapped\"))\n|_ Inner(\"inner\")"
        );
    }
}

#[test]
fn error_source_location_partial_bind_preserves_monad_fail_errors_and_causes() {
    let definitions = "deferror Inner { \"inner\" }\ndeferror Outer { |message: String| message }\ndefmod E {\n  def source() -> Result<Int> {\n    Result::cause(Err(Inner()), Outer(\"wrapped\"))\n  }\n  defextractor checked(value: Int) -> MatchResult<Int> {\n    found =? source()\n    MatchResult::Ok(found)\n  }\n}\n";
    for body in [
        "do::<Result> {\n    E::checked(found) <- Ok(2)\n    Ok(found)\n  }",
        "found =? Identity::run(ResultT::run(do::<ResultT<Identity, _>> {\n    E::checked(found) <- ResultT::ok::<Identity>(2)\n    ResultT::ok::<Identity>(found)\n  }))\n  Ok(found)",
    ] {
        let source = format!("{definitions}def main() -> Result<Int> {{\n  {body}\n}}\nmain()\n");
        let dump = assert_error_source_location(&source, "Outer(\"wrapped\")", "Outer");
        assert_eq!(dump["result"]["error"]["message"], "wrapped");
        assert_eq!(dump["result"]["last_value"], "Err(Outer(\"wrapped\"))\n|_ Inner(\"inner\")");
    }
}

#[test]
fn error_source_location_accepts_source_within_the_span_encoding_range() {
    let temp = unique_temp_dir("error_source_location_stride_valid");
    let valid_path = temp.join("within_range.srt");
    let tail = "print(\"ok\")\n";
    let valid_padding = sindr::ir::MODULE_SPAN_STRIDE - 1 - tail.chars().count();
    // Exercise the source-length boundary without parsing a newline token for
    // every four characters. Include the single comment terminator in padding.
    let comment_padding = valid_padding - 1;
    let valid_source = format!(
        "{}{}\n{tail}",
        "# あ ".repeat(comment_padding / 4),
        "#".repeat(comment_padding % 4)
    );
    assert_eq!(
        valid_source.chars().count(),
        sindr::ir::MODULE_SPAN_STRIDE - 1
    );
    write_source(&valid_path, &valid_source);
    let valid = surtr_command()
        .arg("run")
        .arg(&valid_path)
        .output()
        .expect("CLI must run");
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    assert_eq!(valid.stdout, b"ok\n");
    assert!(valid.stderr.is_empty(), "{valid:?}");
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn error_source_location_rejects_main_source_exceeding_the_span_encoding_range() {
    let temp = unique_temp_dir("error_source_location_stride_main");
    let padding = "# あ\n".repeat(sindr::ir::MODULE_SPAN_STRIDE / 4 + 1);
    let main_path = temp.join("large_main.srt");
    write_source(
        &main_path,
        &format!("{padding}def main() -> Result<Int> {{ Err(NoneError()) }}\nmain()\n"),
    );
    assert_oversized_source_rejected(&main_path, &main_path);
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn error_source_location_rejects_included_source_exceeding_the_span_encoding_range() {
    let temp = unique_temp_dir("error_source_location_stride_included");
    let padding = "# あ\n".repeat(sindr::ir::MODULE_SPAN_STRIDE / 4 + 1);
    let module_path = temp.join("large_module.srt");
    write_source(
        &module_path,
        &format!(
            "{padding}defmod Oversized {{\n  def fail() -> Result<Int> {{ Err(NoneError()) }}\n}}\n"
        ),
    );
    let include_path = temp.join("include_main.srt");
    write_source(
        &include_path,
        "include \"./large_module.srt\"\nOversized::fail()\n",
    );
    assert_oversized_source_rejected(&include_path, &module_path);
    fs::remove_dir_all(temp).unwrap();
}

fn assert_oversized_source_rejected(entry: &std::path::Path, rejected_source: &std::path::Path) {
    let rejected_file = rejected_source.file_name().unwrap().to_str().unwrap();
    let output = surtr_command()
        .arg("run")
        .arg(entry)
        .output()
        .expect("CLI must run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "oversized source must be rejected"
    );
    assert!(stderr.contains("LoadError"), "{rejected_file}: {stderr}");
    assert!(
        stderr.contains(rejected_file),
        "rejected source must be identified: {stderr}"
    );
    assert!(
        !stderr.contains("bootstrap.srt"),
        "oversized source must not be mapped to Bootstrap: {stderr}"
    );
    assert!(output.stdout.is_empty(), "oversized input must not execute");
}

#[test]
fn error_source_location_hash_map_consumers_keep_key_origin() {
    for source in [
        r#"apply_pattern(hash!["a" => 1], hash!["apply-missing" => _])"#,
        r#"do::<Result> { hash!["do-missing" => _] <- Ok(hash!["a" => 1]); Ok(0) }"#,
    ] {
        let origin = if source.starts_with("apply_pattern") {
            "\"apply-missing\""
        } else {
            "\"do-missing\""
        };
        assert_error_source_location(source, origin, "HashMapKeyMissing");
    }
}
