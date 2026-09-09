use scar::{ScarCheckpoint, ScarSession};

const SOURCE: &str = r#"
deftrait Functor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait Monad where Self: Functor {}
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(value) => Box::Box(mapper(value)), } } }
impl Monad for Box<$T> {}
def retain(value: $F<Int>) -> $F<Int> where $F: Functor { Functor::fmap(value, {|x| x}) }
def stronger(value: Monad<Int>) -> Int { 1 }
a = retain(Box::Box(1))
stronger(a)
"#;

fn check_across_batches(restore_checkpoint: bool) {
    let ast = spire::parse_with_context(SOURCE, spire::ParserContext::project(0)).unwrap();
    let mut resolved = sigil::resolve(ast).unwrap();
    let call = resolved.pop().unwrap();
    let mut session = ScarSession::new();
    session.typecheck(resolved).unwrap();
    if restore_checkpoint {
        let bytes = bincode::serialize(&session.checkpoint()).unwrap();
        let checkpoint: ScarCheckpoint = bincode::deserialize(&bytes).unwrap();
        session = ScarSession::new();
        session.rollback(checkpoint);
    }
    let error = session.typecheck(vec![call]).expect_err(
        "a retained Functor view cannot gain Monad capability across typecheck batches",
    );
    assert!(error.message.contains("Monad"), "{error:?}");
}

#[test]
fn constructor_view_survives_typecheck_batches() {
    check_across_batches(false);
}

#[test]
fn constructor_view_survives_serialized_checkpoint() {
    check_across_batches(true);
}
