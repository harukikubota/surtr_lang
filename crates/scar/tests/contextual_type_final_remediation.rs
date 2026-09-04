use scar::typed::TypedNode;
use scar::ScarSession;
use sigil::resolved::{Resolved, ResolvedWhereConstraintRhs};

fn resolve_without_std_prelude(source: &str) -> Vec<Resolved> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse without the std prelude");
    sigil::resolve(ast).expect("source should resolve without the std prelude")
}

fn typecheck_without_std_prelude(source: &str) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    scar::typecheck(resolve_without_std_prelude(source))
}

const FUNCTOR: &str = r#"deftrait Functor
where
  Self: Type<$A>
{
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}
"#;

#[test]
fn scar_defensively_rejects_trait_slot_mapping_in_a_method_where_clause() {
    let mut resolved = resolve_without_std_prelude(&format!(
        r#"{FUNCTOR}
defenum Boxed<$A> {{ Boxed($A) }}
impl Functor for Boxed<$A>
where
  $A: Functor.$A
{{
  def fmap(self: Boxed<$A>, mapper: ($A -> $B)) -> Boxed<$B> {{
    match self {{ Boxed::Boxed(value) => Boxed::Boxed(mapper(value)) }}
  }}
}}"#
    ));
    let moved = resolved.iter_mut().any(|node| {
        let Resolved::TraitImplDef(_, _, _, _, where_clause, methods) = node else {
            return false;
        };
        methods[0].where_clause = where_clause.clone();
        true
    });
    assert!(moved, "test setup must find the resolved trait impl");

    let err = scar::typecheck(resolved)
        .expect_err("Scar must reject malformed resolved Trait.$Slot placement");
    assert!(
        err.message.contains("trait implementation where clause"),
        "{err:?}"
    );
}

#[test]
fn self_impl_capability_is_not_an_arity_zero_candidate_obligation() {
    typecheck_without_std_prelude(
        r#"deftrait Marker<$Tag> {
  def mark::<$Tag>(self: Self) -> $Tag
}

deftrait Use {
  def use(self: Self) -> Int
}

impl Marker<Int> for Int {
  def mark::<Int>(self: Int) -> Int { self }
}

impl Use for Int
where
  Self: Marker
{
  def use(self: Int) -> Int { Marker::mark::<Int>(self) }
}

result = Use::use(1)"#,
    )
    .expect("the full Marker<Int> body call, not a synthetic Marker<> proof, validates the impl");
}

#[test]
fn unused_self_impl_capability_is_reported_at_the_bound() {
    let err = typecheck_without_std_prelude(
        r#"deftrait Marker {
  def mark(self: Self) -> Int
}

deftrait Use {
  def use(self: Self) -> Int
}

impl Use for Int
where
  Self: Marker
{
  def use(self: Int) -> Int { self }
}"#,
    )
    .expect_err("an unused Self capability must not disappear from impl accounting");

    assert!(
        err.message
            .contains("UnusedTraitConstraint: Self: Marker is never consumed by this impl block"),
        "{err:?}"
    );
}

#[test]
fn enum_constructor_application_uses_only_the_declared_mapped_slot() {
    typecheck_without_std_prelude(&format!(
        r#"{FUNCTOR}
defenum Pair<$L, $R> {{ Pair($L, $R) }}

impl Functor for Pair<$L, $R>
where
  $R: Functor.$A
{{
  def fmap(self: Pair<$L, $A>, mapper: ($A -> $B)) -> Pair<$L, $B> {{
    match self {{ Pair::Pair(left, right) => Pair::Pair(left, mapper(right)) }}
  }}
}}

def accept(value: Functor<Int>) -> Int {{ 1 }}
def make() -> Functor<Int> {{ Pair::Pair("left", 1) }}

pair = Pair::Pair("left", 1)
from_parameter: Int = accept(pair)
from_return: Pair<String, Int> = make()"#
    ))
    .expect("Pair's left capture must not be treated as a Functor slot");
}

#[test]
fn struct_constructor_application_uses_only_the_declared_mapped_slot() {
    typecheck_without_std_prelude(&format!(
        r#"{FUNCTOR}
defstruct Pair<$L, $R> {{ left: $L, right: $R }}

impl Pair {{
  def new(left: $L, right: $R) -> Pair<$L, $R> {{ Pair {{ left: left, right: right }} }}
}}

impl Functor for Pair<$L, $R>
where
  $R: Functor.$A
{{
  def fmap(self: Pair<$L, $A>, mapper: ($A -> $B)) -> Pair<$L, $B> {{
    Pair {{ left: self.left, right: mapper(self.right) }}
  }}
}}

def accept(value: Functor<Int>) -> Int {{ 1 }}
pair = Pair("left", 1)
result: Int = accept(pair)"#
    ))
    .expect("struct fields unrelated to the mapped slot must not enter constructor matching");
}

#[test]
fn alias_expansion_cannot_hide_a_forbidden_constructor_application() {
    let err = typecheck_without_std_prelude(&format!(
        r#"{FUNCTOR}
type HiddenContext = (Functor<Int> -> Int)
defstruct Holder {{ callback: HiddenContext }}"#
    ))
    .expect_err("a function-signature alias must not hide a nested constructor application");

    assert!(
        err.message.contains("ConstructorTraitApplicationPosition"),
        "{err:?}"
    );
}

#[test]
fn inherent_method_does_not_count_as_an_ordinary_function_position() {
    let err = typecheck_without_std_prelude(&format!(
        r#"{FUNCTOR}
defenum Boxed<$A> {{ Boxed($A) }}
impl Boxed {{
  def forbidden(value: Functor<Int>) -> Int {{ 1 }}
}}"#
    ))
    .expect_err("an inherent method is not an ordinary function signature");
    assert!(
        err.message.contains("ConstructorTraitApplicationPosition"),
        "{err:?}"
    );
}

#[test]
fn specialized_generic_pin_pattern_has_a_static_dispatch() {
    let typed = typecheck_without_std_prelude(
        r#"deftrait Eq {
  def eq(self: Self, rhs: Self) -> Int
}

impl Eq for Int {
  def eq(self: Int, rhs: Int) -> Int { self }
}

def pinned_equal(value: $A, pinned: $A) -> Int
where
  $A: Eq
{
  match value {
    ^pinned => 1,
    _ => 0,
  }
}

result: Int = pinned_equal(1, 1)"#,
    )
    .expect("the generic pin-pattern function should specialize for Int");

    let debug = format!("{typed:#?}");
    assert!(
        debug.contains("pattern: Pin {"),
        "test must retain a pin pattern: {debug}"
    );
    assert!(
        !debug.contains("Pending"),
        "a specialized pin dispatch must not remain pending: {debug}"
    );
}

#[test]
fn contextual_return_reconstructs_only_the_declared_enum_slot() {
    typecheck_without_std_prelude(&format!(
        r#"{FUNCTOR}
defenum Pair<$L, $R> {{ Pair($L, $R) }}

impl Functor for Pair<$L, $R>
where
  $R: Functor.$A
{{
  def fmap(self: Pair<$L, $A>, mapper: ($A -> $B)) -> Pair<$L, $B> {{
    match self {{ Pair::Pair(left, right) => Pair::Pair(left, mapper(right)) }}
  }}
}}

result: Pair<String, Boolean> = Functor::fmap(
  Pair::Pair("left", 1),
  {{|item| True}}
)"#
    ))
    .expect("changing the Functor slot must preserve Pair's unmapped left argument");
}

#[test]
fn fresh_contextual_witness_preserves_its_trait_among_same_arity_traits() {
    typecheck_without_std_prelude(
        r#"deftrait LeftMap
where
  Self: Type<$A>
{
  def map_left(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}

deftrait RightMap
where
  Self: Type<$A>
{
  def map_right(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}

defenum Pair<$L, $R> { Pair($L, $R) }

impl LeftMap for Pair<$L, $R>
where
  $L: LeftMap.$A
{
  def map_left(self: Pair<$A, $R>, mapper: ($A -> $B)) -> Pair<$B, $R> {
    match self { Pair::Pair(left, right) => Pair::Pair(mapper(left), right) }
  }
}

impl RightMap for Pair<$L, $R>
where
  $R: RightMap.$A
{
  def map_right(self: Pair<$L, $A>, mapper: ($A -> $B)) -> Pair<$L, $B> {
    match self { Pair::Pair(left, right) => Pair::Pair(left, mapper(right)) }
  }
}

def transform(value: RightMap<Int>) -> Int {
  mapped = RightMap::map_right(value, {|item| True})
  1
}

result: Int = transform(Pair::Pair("left", 1))"#,
    )
    .expect("fresh RightMap witnesses must not fall back ambiguously to LeftMap by arity");
}

#[test]
fn builtin_declaration_is_not_an_ordinary_function_position() {
    let mut resolved = resolve_without_std_prelude(&format!(
        r#"{FUNCTOR}
def forbidden(value: Functor<Int>) -> Int {{ 1 }}"#
    ));
    let converted = resolved.iter_mut().any(|node| {
        let Resolved::Def(span, id, _, params, ret, where_clause, _, attrs) = node.clone() else {
            return false;
        };
        if id.name != "forbidden" {
            return false;
        }
        *node = Resolved::BuiltinDecl(span, id, Vec::new(), params, ret, where_clause, attrs);
        true
    });
    assert!(
        converted,
        "test setup must create a resolved builtin declaration"
    );

    let err = scar::typecheck(resolved)
        .expect_err("builtin declarations cannot use contextual constructor applications");
    assert!(
        err.message.contains("ConstructorTraitApplicationPosition"),
        "{err:?}"
    );
}

#[test]
fn inherent_owner_from_an_earlier_batch_is_not_an_ordinary_function_position() {
    let mut resolved = resolve_without_std_prelude(&format!(
        r#"{FUNCTOR}
defenum Boxed<$A> {{ Boxed($A) }}
impl Boxed {{
  def forbidden(value: Functor<Int>) -> Int {{ 1 }}
}}"#
    ));
    let inherent_impl = resolved
        .pop()
        .expect("test setup must retain the inherent impl");
    let mut session = ScarSession::new();
    session
        .typecheck(resolved)
        .expect("the first batch should register the trait and nominal owner");

    let err = session
        .typecheck(vec![inherent_impl])
        .expect_err("a persisted inherent owner must not evade position validation");
    assert!(
        err.message.contains("ConstructorTraitApplicationPosition"),
        "{err:?}"
    );
}

#[test]
fn same_head_receiver_does_not_consume_a_distinct_self_capability() {
    let err = typecheck_without_std_prelude(
        r#"deftrait Marker {
  def mark(self: Self) -> String
}

deftrait Use {
  def use(self: Self) -> String
}

defenum Pair<$A> { Pair($A) }

impl Marker for Pair<String> {
  def mark(self: Pair<String>) -> String {
    match self { Pair::Pair(value) => value }
  }
}

impl Use for Pair<$A>
where
  Self: Marker
{
  def use(self: Pair<$A>) -> String {
    Marker::mark(Pair::Pair("not self"))
  }
}"#,
    )
    .expect_err("Pair<String> must not consume a capability for generic Self Pair<$A>");
    assert!(
        err.message
            .contains("requires a receiver type implementing Marker")
            || err.message.contains("UnusedTraitConstraint"),
        "{err:?}"
    );
}

#[test]
fn scar_rejects_slot_map_owned_by_a_different_constructor_trait() {
    let err = typecheck_without_std_prelude(&format!(
        r#"{FUNCTOR}
deftrait Other
where
  Self: Type<$A>
{{}}
defenum Boxed<$A> {{ Boxed($A) }}

impl Other for Boxed<$A>
where
  $A: Functor.$A
{{}}"#
    ))
    .expect_err("a slot map must belong to the enclosing constructor trait impl");
    assert!(err.message.contains("same trait"), "{err:?}");
}

#[test]
fn scar_rejects_slot_map_on_a_non_constructor_trait_impl() {
    let mut resolved = resolve_without_std_prelude(&format!(
        r#"{FUNCTOR}
deftrait Plain {{}}
defenum Boxed<$A> {{ Boxed($A) }}

impl Plain for Boxed<$A>
where
  $A: Functor.$A
{{}}"#
    ));
    let mutated = resolved.iter_mut().any(|node| {
        let Resolved::TraitImplDef(_, impl_trait_id, _, _, Some(clause), _) = node else {
            return false;
        };
        let Some(ResolvedWhereConstraintRhs::TraitSlot { trait_id, .. }) =
            clause.constraints[0].bounds.first_mut()
        else {
            return false;
        };
        *trait_id = impl_trait_id.clone();
        true
    });
    assert!(mutated, "test setup must retarget the resolved slot owner");

    let err = scar::typecheck(resolved)
        .expect_err("a non-constructor enclosing trait cannot own slot mappings");
    assert!(err.message.contains("TypeConstructor trait"), "{err:?}");
}
