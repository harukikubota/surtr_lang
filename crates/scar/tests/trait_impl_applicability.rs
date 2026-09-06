fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).expect("parse");
    scar::typecheck(sigil::resolve(ast).expect("resolve"))
}

#[test]
fn generic_box_method_instantiates_head_variable() {
    check(
        r#"
defstruct Box<$T> { val: $T }
impl Box { def new(val: $T) -> Box<$T> { Box { val: val } } }
deftrait Read<$V> { def read::<$V>(self: Self) -> $V }
impl Read<$T> for Box<$T> { def read::<$T>(self: Self) -> $T { self.val } }
def run() -> String { Read::read::<String>(Box::new("ok")) }
"#,
    )
    .expect("generic impl head must instantiate method and body");
}

#[test]
fn nested_disjoint_heads_are_order_independent() {
    for reverse in [false, true] {
        let implementations = [
            "impl Read<Int> for Box<Int> { def read::<Int>(self: Self) -> Int { self.val } }",
            "impl Read<String> for Box<String> { def read::<String>(self: Self) -> String { self.val } }",
        ];
        let order = if reverse { [1, 0] } else { [0, 1] };
        check(&format!(
            r#"
defstruct Box<$T> {{ val: $T }}
impl Box {{ def new(val: $T) -> Box<$T> {{ Box {{ val: val }} }} }}
deftrait Read<$V> {{ def read::<$V>(self: Self) -> $V }}
{}
{}
def run() -> String {{ Read::read::<String>(Box::new("ok")) }}
"#,
            implementations[order[0]], implementations[order[1]]
        ))
        .expect("disjoint heads");
    }
}

#[test]
fn generic_overlap_is_rejected() {
    let error = check(
        r#"
deftrait Read<$V> { def read::<$V>(self: Self) -> $V }
impl Read<$T> for List<$T> { def read::<$T>(self: Self) -> $T { Read::read(self) } }
impl Read<Int> for List<Int> { def read::<Int>(self: Self) -> Int { 1 } }
"#,
    )
    .expect_err("overlap");
    assert!(error.message.contains("Overlapping"), "{error}");
}

#[test]
fn phantom_arguments_remain_part_of_impl_pattern() {
    check(
        r#"
defstruct Token<$T> { value: Int }
impl Token { def new(value: Int) -> Token<Int> { Token { value: value } } }
deftrait Read { def read(self: Self) -> Int }
impl Read for Token<Int> { def read(self: Self) -> Int { 1 } }
impl Read for Token<String> { def read(self: Self) -> Int { 2 } }
"#,
    )
    .expect("phantom nominal arguments make these impl heads disjoint");
}

#[test]
fn impl_where_rejects_a_concrete_subject_without_capability() {
    let source = include_str!(
        "../../../tests/fixtures/script/fail/typecheck/trait_impl_where_unsatisfied.srt"
    );
    let source = source.replace(
        "print(Read::read(Box::new(1), 2))",
        "Read::read(Box::new(1), 2)",
    );
    let error = check(&source).expect_err("impl where must be proved");
    assert!(error.message.contains("implementing Read<Int>"), "{error}");
}

#[test]
fn full_head_requires_trait_arguments_and_subject_in_one_mapping() {
    let source = include_str!(
        "../../../tests/fixtures/script/fail/typecheck/trait_impl_no_applicable_full_head.srt"
    );
    let source = source.replace(
        "print(Pick::pick(Box::new(1), 2))",
        "Pick::pick(Box::new(1), 2)",
    );
    let error = check(&source).expect_err("no complete head matches");
    assert!(error.message.contains("implementing Pick<Int>"), "{error}");
}

#[test]
fn impl_where_accepts_a_concrete_subject_with_capability() {
    check(r#"
deftrait Marker { def mark(self: Self) -> String }
impl Marker for Int { def mark(self: Self) -> String { "ok" } }
defstruct Box<$T> { value: $T }
impl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }
deftrait Read<$V> { def read(self: Self, fallback: $V) -> String }
impl Read<$T> for Box<$T> where $T: Marker { def read(self: Self, fallback: $T) -> String { Marker::mark(self.value) } }
Read::read(Box::new(1), 2)
"#).expect("where proof uses concrete Marker impl");
}

#[test]
fn zero_argument_method_uses_return_only_head_substitution() {
    check(
        r#"
deftrait Factory { def make::<Self>() -> Self }
impl Factory for Int { def make::<Self>() -> Self { 1 } }
Factory::make::<Int>()
"#,
    )
    .expect("return-only invocation selects its full head");
}
