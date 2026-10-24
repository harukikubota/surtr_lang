#[allow(dead_code)]
mod support;

const PAIR_CARRIER: &str = r#"
defenum Pair<$A, $B> { Pair($A, $B), }

impl Functor for Pair<$T, Int> {
  def fmap(self: Pair<$A, Int>, mapper: ($A -> $B)) -> Pair<$B, Int> {
    match self { Pair::Pair(value, count) => Pair::Pair(mapper(value), count), }
  }
}
impl Applicative for Pair<$T, Int> {
  def pure::<Pair<$T, Int>>(value: $A) -> Pair<$A, Int> { Pair::Pair(value, 0) }
  def ap(mapper: Pair<($A -> $B), Int>, value: Pair<$A, Int>) -> Pair<$B, Int> {
    match mapper { Pair::Pair(fun, _) => Functor::fmap(value, fun), }
  }
}
impl Monad for Pair<$T, Int> {
  def return::<Pair<$T, Int>>(value: $A) -> Pair<$A, Int> { Pair::Pair(value, 0) }
  def bind(self: Pair<$A, Int>, mapper: ($A -> Pair<$B, Int>)) -> Pair<$B, Int> {
    match self { Pair::Pair(value, _) => mapper(value), }
  }
}
impl MonadExtractor for Pair<$T, Int> {
  def extract(self: Pair<$A, Int>) -> Result<$A> {
    match self { Pair::Pair(value, _) => Ok(value), }
  }
}
impl Pair {
  defextractor whole(expected: $T, target: Pair<$T, Int>) -> MatchResult<Pair<$T, Int>> {
    MatchResult::Ok(target)
  }
  defextractor bare(target: Pair<$T, Int>) -> MatchResult<Pair<$T, Int>> {
    MatchResult::Ok(target)
  }
}
"#;

#[test]
fn failed_payload_relation_does_not_constrain_extractor_prearguments() {
    let source = format!(
        "{PAIR_CARRIER}\nPair::whole(Pair::Pair(True, \"label\"), value) =? Pair::Pair(Pair::Pair(True, \"label\"), 1)"
    );
    let error = support::typecheck(support::resolve_with_builtin_prelude(&source))
        .expect_err("the extracted Pair<Boolean, String> is not the original Pair<_, Int>");
    assert_eq!(
        error.reason().map(|reason| reason.as_str()),
        Some("SafeBindWholeInputAfterExtraction"),
        "{error:?}"
    );
}

#[test]
fn failed_payload_relation_does_not_constrain_whole_input_probe() {
    let source =
        format!("{PAIR_CARRIER}\nPair::bare(value) =? Pair::Pair(Pair::Pair(True, \"label\"), 1)");
    let error = support::typecheck(support::resolve_with_builtin_prelude(&source))
        .expect_err("the extracted Pair<Boolean, String> is not the original Pair<_, Int>");
    assert_eq!(
        error.reason().map(|reason| reason.as_str()),
        Some("SafeBindWholeInputAfterExtraction"),
        "{error:?}"
    );
}

#[test]
fn compatible_payload_keeps_shared_input_and_preargument_inference() {
    let source = format!(
        "{PAIR_CARRIER}\nPair::whole(Pair::Pair(True, \"label\"), value) =? Ok(Pair::Pair(Pair::Pair(True, \"label\"), 1))"
    );
    support::typecheck(support::resolve_with_builtin_prelude(&source))
        .expect("compatible input must retain the substitution shared with the preargument");

    let source =
        format!("{PAIR_CARRIER}\nPair::whole(\"wrong\", value) =? Ok(Pair::Pair(True, 1))");
    let error = support::typecheck(support::resolve_with_builtin_prelude(&source))
        .expect_err("a preargument inconsistent with a compatible input must still fail");
    assert_eq!(
        error.reason().map(|reason| reason.as_str()),
        Some("ArgumentTypeMismatch"),
        "{error:?}"
    );
}
