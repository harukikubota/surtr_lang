#[allow(dead_code)]
mod support;

use scar::{ScarCheckpoint, ScarSession};

const SOURCE: &str = r#"
deftrait FixtureFunctor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait FixtureMonad where Self: FixtureFunctor {}
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(value) => Box::Box(mapper(value)), } } }
impl FixtureMonad for Box<$T> {}
def retain(value: $F<Int>) -> $F<Int> where $F: FixtureFunctor { FixtureFunctor::fmap(value, {|x| x}) }
def stronger(value: FixtureMonad<Int>) -> Int { 1 }
a = retain(Box::Box(1))
stronger(a)
"#;

fn check_across_batches(restore_checkpoint: bool) {
    let ast = spire::parse_with_context(SOURCE, spire::ParserContext::project(0)).unwrap();
    let mut resolved = support::resolve_ast_with_builtin_prelude(ast).unwrap();
    let call = resolved.pop().unwrap();
    let mut session = support::session_from_cached_std_prelude();
    session.typecheck(resolved).unwrap();
    if restore_checkpoint {
        let bytes = bincode::serialize(&session.checkpoint()).unwrap();
        let checkpoint: ScarCheckpoint = bincode::deserialize(&bytes).unwrap();
        session = ScarSession::new();
        session.rollback(checkpoint);
    }
    let error = session.typecheck(vec![call]).expect_err(
        "a retained FixtureFunctor view cannot gain FixtureMonad capability across typecheck batches",
    );
    assert!(error.message.contains("FixtureMonad"), "{error:?}");
}

#[test]
fn constructor_view_survives_typecheck_batches() {
    check_across_batches(false);
}

#[test]
fn constructor_view_survives_serialized_checkpoint() {
    check_across_batches(true);
}
