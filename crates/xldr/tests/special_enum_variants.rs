use xldr::repl::logic::ReplOutput;
use xldr::ReplEngine;

fn query_text(output: ReplOutput) -> String {
    match output {
        ReplOutput::StyledDoc { lines } | ReplOutput::PlainText { lines } => lines.join("\n"),
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet,
            details,
        } => [
            symbol,
            signature.unwrap_or_default(),
            summary.unwrap_or_default(),
            source_snippet.unwrap_or_default(),
            details.join("\n"),
        ]
        .join("\n"),
        ReplOutput::Diagnostic {
            rendered,
            summary_tail,
        } => [rendered.join("\n"), summary_tail.join("\n")].join("\n"),
        ReplOutput::EvalError { rendered, .. } => rendered.join("\n"),
        _ => panic!("expected query output"),
    }
}

#[test]
fn canonical_variant_aliases_share_repl_queries_and_definition_targets() {
    let mut engine = ReplEngine::new().expect("standard declarations must bootstrap");
    for (alias, qualified) in [
        ("Ok", "Result::Ok"),
        ("Err", "Result::Err"),
        ("True", "Boolean::True"),
        ("False", "Boolean::False"),
    ] {
        let index = engine.semantic_index();
        let bare = index.find_symbol(alias).expect("alias must be visible");
        let canonical = index
            .find_symbol(qualified)
            .expect("variant must be visible");
        assert_eq!(bare.definition, canonical.definition, "{alias}");
        let bare_info = index
            .symbol_semantic_infos()
            .iter()
            .find(|info| info.surface_name == alias)
            .unwrap();
        let canonical_info = index
            .symbol_semantic_infos()
            .iter()
            .find(|info| info.surface_name == qualified)
            .unwrap();
        assert_eq!(
            bare_info.canonical_name, canonical_info.canonical_name,
            "{alias} declaration target"
        );
        assert_eq!(bare.detail, canonical.detail, "{alias}");
        let sig = engine.handle_line(&format!(":sig {alias}"));
        let qualified_sig = engine.handle_line(&format!(":sig {qualified}"));
        assert_eq!(
            query_text(sig.output),
            query_text(qualified_sig.output),
            "{alias} signatures"
        );
        let doc = engine.handle_line(&format!(":doc {alias}"));
        let qualified_doc = engine.handle_line(&format!(":doc {qualified}"));
        assert_eq!(
            query_text(doc.output),
            query_text(qualified_doc.output),
            "{alias} documentation"
        );
    }
}

#[test]
fn special_constructor_captures_survive_chunks_and_rollback() {
    let mut engine = ReplEngine::new().expect("standard declarations must bootstrap");
    for source in [
        "yes: (-> Boolean) = &True",
        "no: (-> Boolean) = &Boolean::False",
        "wrap: (Int -> Result<Int>) = &Result<_>::Ok",
    ] {
        let result = engine.handle_line(source);
        assert!(
            matches!(result.output, ReplOutput::EvalSuccess { .. }),
            "{source} must construct a callable"
        );
    }
    let failed = engine.handle_line("bad: Result<Int> = Result<Int>::Ok(\"text\")");
    assert!(
        matches!(
            failed.output,
            ReplOutput::Diagnostic { .. } | ReplOutput::EvalError { .. }
        ),
        "explicit payload mismatch must fail"
    );
    let result = engine.handle_line("(yes(), no(), wrap(7))");
    match result.output {
        ReplOutput::EvalSuccess { rendered, .. } => {
            assert!(
                rendered.join("\n").contains("(True, False, Ok(7))"),
                "{rendered:?}"
            );
        }
        _ => panic!("captures must remain callable after rollback"),
    }
}

#[test]
fn special_constructor_captures_append_after_bytecode_restore() {
    let mut engine = ReplEngine::new().expect("standard declarations must bootstrap");
    let input = engine.handle_line("yes: (-> Boolean) = &True");
    assert!(matches!(input.output, ReplOutput::EvalSuccess { .. }));
    let path = std::env::temp_dir().join(format!("surtr-oi037-{}.eldr", std::process::id()));
    let save = engine.handle_line(&format!(":save {}", path.display()));
    assert!(query_text(save.output).contains("saved to"));
    let bytes = std::fs::read(&path).expect("saved bytecode");
    std::fs::remove_file(&path).expect("remove temporary bytecode");
    let bytecode = sindr::ir::Bytecode::decode(&bytes).expect("decode bytecode");
    let next_index = bytecode.functions.len() as u32;
    let mut restored = ReplEngine::from_eldr(&bytes).expect("restore bytecode");
    for source in [
        "wrap: (Int -> Result<Int>) = &Result<_>::Ok",
        "no: (-> Boolean) = &Boolean::False",
    ] {
        let result = restored.handle_line(source);
        assert!(
            matches!(result.output, ReplOutput::EvalSuccess { .. }),
            "{source}"
        );
    }
    let result = restored.handle_line("(wrap(8), no())");
    match result.output {
        ReplOutput::EvalSuccess { rendered, .. } => assert!(
            rendered.join("\n").contains("(Ok(8), False)"),
            "{rendered:?}"
        ),
        _ => panic!("new captures must call after restore"),
    }
    let save = restored.handle_line(&format!(":save {}", path.display()));
    assert!(query_text(save.output).contains("saved to"));
    let bytes = std::fs::read(&path).expect("saved restored bytecode");
    std::fs::remove_file(&path).expect("remove temporary bytecode");
    let updated = sindr::ir::Bytecode::decode(&bytes).expect("decode restored bytecode");
    assert!(updated.functions.len() > next_index as usize);
    for (index, entry) in updated.functions.iter().enumerate() {
        assert_eq!(entry.fun_idx, index as u32);
    }
}
