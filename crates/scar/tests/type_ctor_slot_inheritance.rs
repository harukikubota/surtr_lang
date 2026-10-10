#[allow(dead_code)]
mod support;

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).expect("parse");
    support::typecheck(support::resolve_ast_with_builtin_prelude(ast).expect("resolve"))
}

const DECLARATIONS: &str = r#"
deftrait SlotRoot where Self: Type<$A> {}
deftrait SlotChild where Self: SlotRoot {
  def keep(self: Self<$A>) -> Self<$A> { self }
}
defenum SlotPair<$L, $R> { Pair($L, $R), }
"#;

#[test]
fn child_inherits_mapping_before_method_signatures_in_either_declaration_order() {
    let root = "impl SlotRoot for SlotPair<$L, $R> where $R: SlotRoot.$A {}";
    let child =
        "impl SlotChild for SlotPair<$X, $Y> { def keep(self: Self<$A>) -> Self<$A> { self } }";
    for impls in [format!("{root}\n{child}"), format!("{child}\n{root}")] {
        check(&format!(
            "{DECLARATIONS}\n{impls}\nSlotChild::keep(SlotPair::Pair(1, \"ok\"))"
        ))
        .expect("child must inherit the right slot regardless of declaration order and names");
    }
}

#[test]
fn specialized_child_and_default_method_preserve_captured_argument() {
    check(&format!(
        r#"{DECLARATIONS}
impl SlotChild for SlotPair<String, $T> {{}}
impl SlotRoot for SlotPair<$L, $R> where $R: SlotRoot.$A {{}}
value: SlotPair<String, Int> = SlotChild::keep(SlotPair::Pair("left", 1))
"#
    ))
    .expect("a fixed captured argument must not change the inherited payload slot");
}

#[test]
fn child_explicit_mapping_is_an_optional_consistency_check() {
    for (subject, succeeds) in [("$R", true), ("$L", false)] {
        let result = check(&format!(
            r#"{DECLARATIONS}
impl SlotRoot for SlotPair<$L, $R> where $R: SlotRoot.$A {{}}
impl SlotChild for SlotPair<$L, $R> where {subject}: SlotChild.$A {{}}
"#
        ));
        if succeeds {
            result.expect("matching confirmation");
        } else {
            let error = result.expect_err("child must not override the parent mapping");
            assert!(
                error.message.contains("same constructor slot mapping"),
                "{error}"
            );
        }
    }
}

#[test]
fn root_with_multiple_positions_requires_mapping() {
    let error = check(&format!(
        "{DECLARATIONS}\nimpl SlotRoot for SlotPair<$L, $R> {{}}"
    ))
    .expect_err("root cannot guess a slot");
    assert!(
        error.message.contains("map every constructor slot"),
        "{error}"
    );
}

#[test]
fn child_cannot_replace_missing_or_partial_parent_with_explicit_mapping() {
    for parent in [
        "",
        "impl SlotRoot for SlotPair<Int, $R> where $R: SlotRoot.$A {}",
    ] {
        let error = check(&format!(
            r#"{DECLARATIONS}
{parent}
impl SlotChild for SlotPair<$L, $R> where $R: SlotChild.$A {{}}
"#
        ))
        .expect_err("one parent must cover every child instance");
        assert!(
            error.message.contains("requires parent impl SlotRoot"),
            "{error}"
        );
    }
}

#[test]
fn child_requires_parent_bounds_from_its_own_assumptions() {
    let declarations = r#"
deftrait SlotMarker { def mark(self: Self) -> Unit }
deftrait SlotRoot where Self: Type<$A> { def touch(self: Self<$A>) -> Unit }
deftrait SlotChild where Self: SlotRoot { def keep(self: Self<$A>) -> Self<$A> }
defenum SlotPair<$L, $R> { Pair($L, $R), }
impl SlotRoot for SlotPair<$L, $R>
where
  $L: SlotMarker
  $R: SlotRoot.$A
{
  def touch(self: Self<$A>) -> Unit {
    match self { SlotPair::Pair(left, right) => SlotMarker::mark(left), }
  }
}
"#;
    check(&format!(
        r#"{declarations}
impl SlotChild for SlotPair<$X, $Y> where $X: SlotMarker {{
  def keep(self: Self<$A>) -> Self<$A> {{
    match self {{ SlotPair::Pair(left, right) => SlotMarker::mark(left), }}
    self
  }}
}}
"#
    ))
    .expect("child bound proves substituted parent bound");
    let error = check(&format!(
        r#"{declarations}
impl SlotChild for SlotPair<$X, $Y> {{ def keep(self: Self<$A>) -> Self<$A> {{ self }} }}
"#
    ))
    .expect_err("an unbounded child cannot borrow the parent bound");
    assert!(error.message.contains("parent constraint"), "{error}");
}

#[test]
fn inherited_slot_cannot_be_fixed_to_concrete_type() {
    let error = check(&format!(
        r#"{DECLARATIONS}
impl SlotRoot for SlotPair<$L, $R> where $R: SlotRoot.$A {{}}
impl SlotChild for SlotPair<$T, Int> {{}}
"#
    ))
    .expect_err("the inherited right slot is not the child's remaining variable");
    assert!(
        error.message.contains("top-level type parameter"),
        "{error}"
    );
}

#[test]
fn ordinary_parent_does_not_supply_or_conflict_with_constructor_slots() {
    check(
        r#"
deftrait SlotMarker {}
deftrait RootWithMarker where Self: Type<$A> + SlotMarker {}
deftrait ChildWithMarker where Self: RootWithMarker + SlotMarker {}
defenum SlotBox<$T> { Box($T), }
impl ChildWithMarker for SlotBox<$T> {}
impl RootWithMarker for SlotBox<$T> {}
impl SlotMarker for SlotBox<$T> {}
"#,
    )
    .expect("ordinary parents prove capability without supplying slots");
}

#[test]
fn diamond_and_independent_roots_require_agreement_only_when_inherited() {
    for right_slot in ["$R", "$L"] {
        let source = format!(
            r#"{DECLARATIONS}
deftrait OtherSlotRoot where Self: Type<$B> {{}}
deftrait SlotDiamond where Self: SlotChild + OtherSlotRoot {{}}
impl SlotDiamond for SlotPair<$L, $R> {{}}
impl SlotChild for SlotPair<$L, $R> {{}}
impl SlotRoot for SlotPair<$L, $R> where $R: SlotRoot.$A {{}}
impl OtherSlotRoot for SlotPair<$L, $R> where {right_slot}: OtherSlotRoot.$B {{}}
"#
        );
        if right_slot == "$R" {
            check(&source).expect("multiple parents agree");
        } else {
            let error =
                check(&source).expect_err("independent roots disagree at their shared child");
            assert!(
                error.message.contains("constructor slot mapping"),
                "{error}"
            );
            check(&source.replace("impl SlotDiamond for SlotPair<$L, $R> {}", ""))
                .expect("independent roots may map different positions");
        }
    }
    check(&format!(
        r#"{DECLARATIONS}
deftrait SlotSibling where Self: SlotRoot {{}}
deftrait SlotDiamond where Self: SlotChild + SlotSibling {{}}
impl SlotDiamond for SlotPair<$L, $R> {{}}
impl SlotChild for SlotPair<$L, $R> {{}}
impl SlotSibling for SlotPair<$L, $R> {{}}
impl SlotRoot for SlotPair<$L, $R> where $R: SlotRoot.$A {{}}
"#
    ))
    .expect("diamond preserves a common root mapping");
}

#[test]
fn child_cannot_add_slots_or_merge_inherited_slots() {
    let error = check(
        r#"
deftrait TwoSlots where Self: Type<$A, $B> {}
deftrait ExtraSlot where Self: TwoSlots + Type<$A, $B, $C> {}
"#,
    )
    .expect_err("child cannot extend inherited shape");
    assert!(error.message.contains("constructor slot"), "{error}");
    let error = check(
        r#"
deftrait TwoSlots where Self: Type<$A, $B> {}
deftrait TwoSlotsChild where Self: TwoSlots {}
defenum SlotPair<$L, $R> { Pair($L, $R), }
impl TwoSlots for SlotPair<$L, $R> where $L: TwoSlots.$A
$R: TwoSlots.$B {}
impl TwoSlotsChild for SlotPair<$T, $T> {}
"#,
    )
    .expect_err("inherited slots must stay distinct variables");
    assert!(
        error.message.contains("more than one constructor slot"),
        "{error}"
    );
}

#[test]
fn parent_proof_resolves_constructor_metadata_needed_by_a_specialized_base() {
    let declarations = r#"
deftrait DepRoot where Self: Type<$A> {
  def pure::<Self>(value: $A) -> Self<$A>
}
deftrait DepChild where Self: DepRoot {}
defenum DepLeaf<$A> { Leaf($A), }
defenum DepWrap<$M, $A> where $M: DepChild { Wrap($M<$A>), }
deftrait WrapChild where Self: DepRoot {}
impl WrapChild for DepWrap<DepLeaf, $T> {}
impl DepRoot for DepWrap<$M, $T>
where
  $M: DepChild
  $T: DepRoot.$A
{
  def pure::<DepWrap<$M, $T>>(value: $A) -> DepWrap<$M, $A> {
    DepWrap::Wrap(DepRoot::pure(value))
  }
}
impl DepChild for DepLeaf<$T> {}
impl DepRoot for DepLeaf<$T> {
  def pure::<DepLeaf<$T>>(value: $A) -> DepLeaf<$A> { DepLeaf::Leaf(value) }
}
"#;
    check(declarations)
        .expect("parent bound needs the later DepChild mapping for its concrete base");
    check(&format!(
        r#"{declarations}
defenum OtherLeaf<$A> {{ Leaf($A), }}
impl DepChild for OtherLeaf<$T> {{}}
impl DepRoot for OtherLeaf<$T> {{
  def pure::<OtherLeaf<$T>>(value: $A) -> OtherLeaf<$A> {{ OtherLeaf::Leaf(value) }}
}}
"#
    ))
    .expect("an unrelated pending target must not become a dependency of the concrete base proof");
}

#[test]
fn parameterized_parent_family_selects_by_coverage_before_comparing_mappings() {
    let declarations = r#"
deftrait LeftMark { def mark(self: Self) -> Unit }
deftrait RightMark { def mark(self: Self) -> Unit }
deftrait ParamRoot<$Tag> where Self: Type<$A> { def touch(self: Self<$A>) -> Unit }
deftrait ParamChild where Self: ParamRoot { def touch_child(self: Self<$A>) -> Unit }
defenum SlotPair<$L, $R> { Pair($L, $R), }
impl ParamRoot<Int> for SlotPair<$L, $R>
where
  $L: LeftMark
  $R: ParamRoot.$A
{
  def touch(self: Self<$A>) -> Unit {
    match self { SlotPair::Pair(left, right) => LeftMark::mark(left), }
  }
}
impl ParamRoot<String> for SlotPair<$L, $R>
where
  $R: RightMark
  $L: ParamRoot.$A
{
  def touch(self: Self<$A>) -> Unit {
    match self { SlotPair::Pair(left, right) => RightMark::mark(right), }
  }
}
"#;
    check(&format!(
        r#"{declarations}
impl ParamChild for SlotPair<$L, $R> where $L: LeftMark {{
  def touch_child(self: Self<$A>) -> Unit {{
    match self {{ SlotPair::Pair(left, right) => LeftMark::mark(left), }}
  }}
}}
"#
    ))
    .expect("only the Int family instance covers the child's declared bounds");
    let error = check(&format!(
        r#"{declarations}
impl ParamChild for SlotPair<$L, $R>
where
  $L: LeftMark
  $R: RightMark
{{ def touch_child(self: Self<$A>) -> Unit {{ () }} }}
"#
    ))
    .expect_err("two suitable family instances cannot supply different mappings");
    assert!(
        error
            .message
            .contains("conflicting inherited constructor slot mappings"),
        "{error}"
    );
}

#[test]
fn child_shape_and_partial_confirmation_preserve_inherited_slot_ordinals() {
    check(
        r#"
deftrait TwoSlots where Self: Type<$A, $B> {}
deftrait SameShape where Self: TwoSlots + Type<$X, $Y> {}
defenum SlotPair<$L, $R> { Pair($L, $R), }
impl SameShape for SlotPair<$L, $R> where $R: SameShape.$X {}
impl TwoSlots for SlotPair<$L, $R>
where
  $R: TwoSlots.$A
  $L: TwoSlots.$B
{}
"#,
    )
    .expect("child shape keeps slot order and only confirms the explicitly supplied slot");
}

#[test]
fn impl_trait_argument_preserves_self_substitution() {
    // Source syntax rejects Self in an impl head; preserve the resolved-IR
    // substitution contract independently of that parser boundary.
    let ast = spire::parse_with_context(
        "deftrait Echo<$T> {}\nimpl Echo<Int> for Int {}",
        spire::ParserContext::project(0),
    )
    .expect("parse");
    let mut resolved = support::resolve_ast_with_builtin_prelude(ast).expect("resolve");
    for node in &mut resolved {
        if let sigil::resolved::Resolved::TraitImplDef(_, _, id, args, _, _, _) = node {
            if id.name == "Echo" {
                args[0] = spire::ast::AstTy::Named(
                    spire::ast::Span { start: 31, end: 34 },
                    "Self".into(),
                );
            }
        }
    }
    support::typecheck(resolved).expect("resolved impl argument substitutes Self");
}
