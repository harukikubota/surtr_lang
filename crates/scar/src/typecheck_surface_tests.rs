use scar::typed::{
    OperatorTraitOp, TraitCallOrigin, TypedInner, TypedLensSegment, TypedNode, TypedProgram,
};
use scar::types::Ty;
use sindr::policy::{EntryPoint, ExitCodePolicy, RuntimeSourcePolicy};

use crate::test_support::*;

#[test]
fn field_access_is_resolved_to_numeric_index() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age }
  }
}

user: User = User("alice", 30)
age = user.age"#,
    );

    let typed = typecheck(resolved).expect("typecheck should succeed");
    let lens_view = typed.iter().find_map(|node| {
        if let TypedInner::Bind(_, rhs) = &node.node {
            if let TypedInner::LensView {
                path,
                source_is_result,
                ..
            } = &rhs.node
            {
                return Some((path.clone(), *source_is_result));
            }
        }
        None
    });

    let (path, source_is_result) = lens_view.expect("expected bind rhs to be LensView");
    assert!(!source_is_result);
    assert!(!path.may_fail);
    assert_eq!(path.segments.len(), 1);
    match &path.segments[0] {
        TypedLensSegment::Field { field_index, .. } => assert_eq!(*field_index, 1),
        other => panic!("expected field segment, got {other:?}"),
    }
}

#[test]
fn match_bool_requires_exhaustive_arms() {
    let resolved = resolve_with_builtin_prelude(
        r#"flag = True
print(match flag {
  True => "yes",
})"#,
    );

    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err.message.contains("Non-exhaustive match. Missing: False"));
}

#[test]
fn safebind_total_pattern_accepts_plain_rhs() {
    let resolved = resolve_with_builtin_prelude("num =? 10");
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

#[test]
fn dbg_special_form_typechecks_to_unit() {
    let resolved = resolve_with_builtin_prelude("x = dbg!(1, \"ok\")");
    let typed = typecheck(resolved).expect("typecheck should succeed");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");

    assert_eq!(rhs.ty, Ty::Unit);
    assert!(matches!(rhs.node, TypedInner::Dbg(_)));
}

#[test]
fn safebind_function_requires_result_return_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"def bad() -> Int {
  num =? Ok(1)
  num
}"#,
    );

    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err
        .message
        .contains("can only be used in functions returning Result"));
}

#[test]
fn safebind_top_ok_pattern_requires_nested_result_rhs() {
    let resolved = resolve_with_builtin_prelude(
        r#"value: Result<Int> = Ok(1)
Ok(num) =? value"#,
    );
    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err.message.contains("`Ok(...)` pattern requires Result"));
}

#[test]
fn safebind_top_ok_pattern_accepts_nested_result_rhs() {
    let resolved = resolve_with_builtin_prelude(
        r#"value: Result<Result<Int>> = Ok(Ok(1))
Ok(num) =? value"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

#[test]
fn safebind_list_pattern_accepts_plain_list_rhs() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = [1, 2, 3]
[head, ..tail] =? value"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

#[test]
fn safebind_string_pattern_accepts_plain_string_rhs() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = "source"
[head, ..tail] =? value"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

#[test]
fn int_range_literal_typechecks_to_list_int() {
    let resolved = resolve_with_builtin_prelude("nums = [1..3]");
    let typed = typecheck(resolved).expect("typecheck should succeed");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");
    assert_eq!(rhs.ty, Ty::List(Box::new(Ty::Int)));
}

#[test]
fn string_range_literal_typechecks_to_result_list_string() {
    let resolved = resolve_with_builtin_prelude(r#"chars = ["a".."c"]"#);
    let typed = typecheck(resolved).expect("typecheck should succeed");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");
    assert_eq!(
        rhs.ty,
        Ty::Result(Box::new(Ty::List(Box::new(Ty::Str))), Box::new(Ty::Error))
    );
}

#[test]
fn match_string_requires_empty_and_uncons_arms_for_exhaustiveness() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = "x"
print(match value {
  [head, ..tail] => head,
})"#,
    );

    let err = typecheck(resolved).expect_err("typecheck should fail");
    assert!(err.message.contains("Non-exhaustive match. Missing: []"));
}

#[test]
fn match_string_accepts_empty_and_uncons_arms() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = "x"
print(match value {
  [] => "empty",
  [head, ..tail] => tail,
})"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(!typed.is_empty());
}

#[test]
fn safebind_list_pattern_accepts_nested_constructor_literals() {
    let resolved = resolve_with_builtin_prelude(
        r#"lr = [Ok(1), Ok(2), Ok(3)]
[Ok(1), Ok(2), _] =? lr"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::SafeBind(_, _))
    ));
}

#[test]
fn tuple_literal_and_field_access_typecheck() {
    let resolved = resolve_with_builtin_prelude(
        r#"pair = (1, "two")
first = pair._0
second = pair._1"#,
    );
    let typed = typecheck(resolved).expect("tuple access should typecheck");
    assert!(
        typed
            .iter()
            .filter(|node| matches!(node.node, TypedInner::Bind(_, _)))
            .count()
            >= 3
    );
}

#[test]
fn tuple_bind_pattern_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"pair = (1, "two")
(left, right) = pair"#,
    );
    let typed = typecheck(resolved).expect("tuple bind should typecheck");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

#[test]
fn lens_view_on_plain_value_returns_plain_focus() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
user.name"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
    assert!(matches!(last.node, TypedInner::LensView { .. }));
}

#[test]
fn lens_view_on_result_value_returns_result_focus() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
result_user: Result<User> = Ok(User("alice"))
result_user.name"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Str)
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));
    assert!(matches!(last.node, TypedInner::LensView { .. }));
}

#[test]
fn lens_variant_selector_returns_result_and_requires_pascal_case() {
    let typed = typecheck_with_builtin_prelude(
        r#"defenum Expr {
  Add(Int, Int),
  Halt,
}
expr = Expr::Add(1, 2)
expr.Add"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Tuple(items) if items.len() == 2)
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));

    let err = typecheck_with_rules(
        r#"defenum Expr {
  Add(Int, Int),
  Halt,
}
expr = Expr::Add(1, 2)
expr.add"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("lowercase variant selector should fail");
    assert!(err.message.contains("No variant selector 'add'"));
}

#[test]
fn lens_compose_typecheck_success_and_mismatch() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
user = User(Profile("alice"))
Lens::view(Lens::compose(User.profile, Profile.name), user)"#,
    );
    assert!(matches!(
        typed.last().map(|node| &node.ty),
        Some(scar::types::Ty::Str)
    ));

    let err = typecheck_with_rules(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
Lens::compose(Profile.name, User.profile)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("mismatched compose should fail");
    assert!(err.message.contains("Lens::compose source/focus mismatch"));
}

#[test]
fn lens_slash_compose_typecheck_success_and_mismatch() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
user = User(Profile("alice"))
Lens::view(User.profile / Profile.name, user)"#,
    );
    assert!(matches!(
        typed.last().map(|node| &node.ty),
        Some(scar::types::Ty::Str)
    ));

    let err = typecheck_with_rules(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
Profile.name / User.profile"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("mismatched slash compose should fail");
    assert!(err.message.contains("source/focus mismatch"));
}

#[test]
fn lens_set_returns_result_source() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
Lens::set(User.name, user, "bob")"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Record(name, _) if name == "User")
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));
    assert!(matches!(last.node, TypedInner::LensSet { .. }));
}

#[test]
fn lens_over_requires_unary_result_callable() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
Lens::over(User.name, user, {|name| Ok(name)})"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Record(name, _) if name == "User")
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));
    assert!(matches!(last.node, TypedInner::LensOver { .. }));

    let err = typecheck_with_rules(
        r#"defrecord User(name: String)
user = User("alice")
Lens::over(User.name, user, {|name| name})"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("non-Result update function should fail");
    assert!(err
        .message
        .contains("Lens::over update function must return Result"));
}

#[test]
fn optional_type_annotation_matches_result_none_error() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Boxed(
  value: Int?,
)
boxed = Boxed(Ok(1))
same: Result<Int, NoneError> = boxed.value"#,
    );
    assert!(!typed.is_empty());
}

#[test]
fn lens_set_accepts_plain_value_for_result_focus() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(score: Result<Int>)
user = User(Err(NoneError))
Lens::set(User.score, user, 3)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(
        &last.ty,
        scar::types::Ty::Result(ok, err)
            if matches!(ok.as_ref(), scar::types::Ty::Record(name, _) if name == "User")
                && matches!(err.as_ref(), scar::types::Ty::Error)
    ));
}

#[test]
fn lens_over_accepts_success_updater_for_result_focus() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(score: Result<Int>)
user = User(Ok(1))
Lens::over(User.score, user, {|score| Ok(score + 1)})"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.node, TypedInner::LensOver { .. }));
}

#[test]
fn lens_over_rejects_result_container_updater_for_result_focus() {
    let err = typecheck_with_rules(
        r#"defrecord User(score: Result<Int>)
user = User(Ok(1))
Lens::over(User.score, user, {|score| Ok(Ok(score))})"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("Result container updater should fail for Lens::over");
    assert!(err
        .message
        .contains("Lens::over update function output mismatch"));
}

#[test]
fn lens_over_result_requires_result_container_updater() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(score: Result<Int>)
user = User(Ok(1))
Lens::over_result(User.score, user, {|score| Ok(score)})"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.node, TypedInner::LensOver { .. }));

    let err = typecheck_with_rules(
        r#"defrecord User(score: Result<Int>)
user = User(Ok(1))
Lens::over_result(User.score, user, {|score| Ok(1)})"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("plain success updater should fail for Lens::over_result");
    assert!(err
        .message
        .contains("Lens::over_result update function output mismatch"));
}

#[test]
fn lens_standalone_tuple_root_is_rejected() {
    let err = resolve_with_builtin_prelude_result(
        r#"pair = (1, "one")
Lens::view(_0, pair)"#,
    )
    .expect_err("standalone tuple root should fail during resolve");
    assert!(err.message.contains("Undefined variable: _0"));
}

#[test]
fn lens_bindings_can_be_reused_by_lens_intrinsics() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
lens = User.name
Lens::view(lens, user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
    assert!(matches!(last.node, TypedInner::LensView { .. }));
}

#[test]
fn lens_tuple_type_root_view_works_with_expected_context() {
    let typed = typecheck_with_builtin_prelude(
        r#"pair = ("alice", 42)
Lens::view(Tuple._0, pair)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
    assert!(matches!(last.node, TypedInner::LensView { .. }));
}

#[test]
fn deferred_tuple_lens_binding_can_be_reused_by_lens_intrinsics() {
    let typed = typecheck_with_builtin_prelude(
        r#"pair = ("alice", 42)
lens = Tuple._1
Lens::view(lens, pair)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Int));
    assert!(matches!(last.node, TypedInner::LensView { .. }));
}

#[test]
fn deferred_tuple_lens_binding_can_compose_before_consumption() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Profile(name: String)
pair = (Profile("alice"), 42)
outer = Tuple._0
path = outer / Profile.name
Lens::view(path, pair)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
    assert!(matches!(last.node, TypedInner::LensView { .. }));
}

#[test]
fn lens_tuple_type_root_compose_works_as_inner_path() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(pair: (String, Int))
user = User(("alice", 42))
Lens::view(Lens::compose(User.pair, Tuple._0), user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

#[test]
fn lens_tuple_type_root_slash_compose_works_as_inner_path() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(pair: (String, Int))
user = User(("alice", 42))
Lens::view(User.pair / Tuple._0, user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

#[test]
fn lens_const_slash_compose_allows_lens_consts() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord Profile(name: String)
defrecord User(profile: Profile)
const USER_PROFILE: Lens<User, Profile> = User.profile
const PROFILE_NAME: Lens<Profile, String> = Profile.name
const FULL_NAME: Lens<User, String> = USER_PROFILE / PROFILE_NAME
user = User(Profile("alice"))
Lens::view(FULL_NAME, user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

#[test]
fn lens_const_slash_compose_rejects_non_lens_const_refs() {
    let err = typecheck_with_rules(
        r#"const VALUE = 1
const BAD = VALUE / VALUE"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("non-lens const refs should fail");
    assert!(err
        .message
        .contains("const value must be a primitive literal or a lens path"));
}

#[test]
fn slash_operator_rejects_numeric_division_and_points_to_safe_div() {
    let err = typecheck_with_rules(r#"print(to_string(10 / 3))"#, RuntimeSourcePolicy::script())
        .expect_err("numeric infix slash should fail");
    assert!(err.message.contains("`/` requires Compose implementation"));
    assert!(err
        .hint
        .as_deref()
        .is_some_and(|hint| hint.contains("safe_div")));
}

#[test]
fn lens_tuple_type_root_without_context_can_bind_as_deferred_path() {
    let typed = typecheck_with_builtin_prelude("lens = Tuple._0");
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Unit));
}

#[test]
fn lens_capture_is_rejected_for_scope_local_model() {
    let err = typecheck_with_rules(
        r#"defrecord User(name: String)
lens = User.name
getter = {|user| Lens::view(lens, user)}
getter(User("alice"))"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("capturing Lens value should fail");
    assert!(err.message.contains("cannot be captured by closures"));
}

#[test]
fn lens_values_cannot_be_embedded_in_runtime_containers() {
    let tuple_err = typecheck_with_rules(
        r#"defrecord User(name: String)
(User.name, 1)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("tuple literal should reject lens");
    assert!(tuple_err
        .message
        .contains("Tuple literal cannot contain Lens values"));

    let list_err = typecheck_with_rules(
        r#"defrecord User(name: String)
[User.name, User.name]"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("list literal should reject lens");
    assert!(list_err
        .message
        .contains("List literal cannot contain Lens values"));

    let ok_err = typecheck_with_rules(
        r#"defrecord User(name: String)
Ok(User.name)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("result constructors should reject lens");
    assert!(ok_err
        .message
        .contains("Result constructors cannot contain Lens values"));
}

#[test]
fn nested_lens_types_are_rejected_in_function_signatures() {
    let param_err = typecheck_with_rules(
        r#"defrecord User(name: String)
def bad(values: List<Lens<User, String>>) -> Unit { () }"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("nested lens in parameter type should fail");
    assert!(param_err
        .message
        .contains("cannot appear in function parameter types"));

    let ret_err = typecheck_with_rules(
        r#"defrecord User(name: String)
def bad() -> List<Lens<User, String>> { [] }"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("nested lens in return type should fail");
    assert!(ret_err
        .message
        .contains("cannot appear in function return types"));
}

#[test]
fn private_value_access_is_allowed_but_capability_root_is_rejected() {
    let typed = typecheck_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
user = User("alice", "s3cr3t")
user.password"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
    assert!(matches!(last.node, TypedInner::LensView { .. }));

    let err = typecheck_with_rules(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
User.password"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("private capability root should fail");
    assert!(err.message.contains("Field 'User.password' is private"));
}

#[test]
fn private_value_access_inside_closure_is_rejected_outside_impl() {
    let err = typecheck_with_rules(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
user = User("alice", "s3cr3t")
{|| user.password}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("private value access inside closure should fail");
    assert!(err
        .message
        .contains("cannot be accessed from closures outside impl"));
}

#[test]
fn private_value_can_be_returned_as_plain_value() {
    let typed = typecheck_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
def read_password(user: User) -> String {
  user.password
}
user = User("alice", "s3cr3t")
read_password(user)"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

#[test]
fn private_value_capture_after_scope_local_read_is_allowed() {
    let typed = typecheck_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
user = User("alice", "s3cr3t")
password = user.password
reader = {|| password}
reader()"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

#[test]
fn private_value_access_inside_param_closure_is_rejected_outside_impl() {
    let err = typecheck_with_rules(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
reader = {|user: User| user.password}
user = User("alice", "s3cr3t")
reader(user)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("private value access inside closure parameter should fail");
    assert!(
        err.message
            .contains("cannot be accessed from closures outside impl")
            || err.message.contains("Field 'User.password' is private")
    );
}

#[test]
fn private_capability_root_is_rejected_in_lens_view_call() {
    let err = typecheck_with_rules(
        r#"defstruct User {
  name: String,
  private password: String,
}
impl User {
  def new(name: String, password: String) -> Self {
User { name: name, password: password }
  }
}
user = User("alice", "s3cr3t")
Lens::view(User.password, user)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("private capability root in Lens::view should fail");
    assert!(err.message.contains("Field 'User.password' is private"));
}

#[test]
fn lens_scope_local_value_can_flow_to_closure_after_view() {
    let typed = typecheck_with_builtin_prelude(
        r#"defrecord User(name: String)
user = User("alice")
lens = User.name
name = Lens::view(lens, user)
reader = {|| name}
reader()"#,
    );
    let last = typed.last().expect("typed program should not be empty");
    assert!(matches!(last.ty, scar::types::Ty::Str));
}

#[test]
fn lens_runtime_transport_restrictions_remain() {
    let arg_err = typecheck_with_rules(
        r#"defrecord User(name: String)
print(to_string(User.name))"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("passing Lens value as argument should fail");
    assert!(arg_err.message.contains("cannot accept Lens values"));

    let return_err = typecheck_with_rules(
        r#"defrecord User(name: String)
def bad() -> Lens<User, String> {
  User.name
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("returning Lens value should fail");
    assert!(return_err
        .message
        .contains("cannot appear in function return types"));

    let arg_var_err = typecheck_with_rules(
        r#"defrecord User(name: String)
def consume(value: String) -> String {
  value
}
lens = User.name
consume(lens)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("passing Lens binding as runtime function argument should fail");
    assert!(
        arg_var_err.message.contains("cannot accept Lens values")
            || arg_var_err.message.contains("Argument type mismatch")
            || arg_var_err.message.contains("compile-time only")
    );
}

#[test]
fn extractor_single_value_match_result_contract_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Single {
  value: Int,
}
impl Single {
  def new(value: Int) -> Self {
Single { value: value }
  }

  defextractor deconstruct(self: Self) -> MatchResult<Int, Error> {
MatchResult::Success(self.value)
  }
}

value = Single(1)
print(match value {
  Single(inner) => to_string(inner),
  _ => "bad",
})"#,
    );
    let typed = typecheck(resolved).expect("single-value extractor should typecheck");
    assert!(!typed.is_empty());
}

#[test]
fn struct_matchblock_head_uses_attached_deconstruct_method() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age }
  }
  defextractor deconstruct(self: Self) -> MatchResult<(String, Int), Error> {
MatchResult::NoMatch
  }
}
user = User("alice", 30)
print(match user {
  User(name, age) => "bad",
  _ => "fallback",
})"#,
    );
    let typed = typecheck(resolved).expect("typecheck should succeed");
    assert!(!typed.is_empty());
}

#[test]
fn struct_matchblock_head_requires_attached_deconstruct_method() {
    let err = resolve_with_builtin_prelude_result(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
User { name: name }
  }
}
user = User("alice")
print(match user {
  User(name) => name,
  _ => "fallback",
})"#,
    )
    .expect_err("resolve should fail");
    assert!(err.message.contains(
        "MatchBlock head `User` requires attached extractor `User::deconstruct`, but it is not defined"
    ));
}

#[test]
fn enum_impl_extractor_can_be_used_in_matchblock() {
    let resolved = resolve_with_builtin_prelude(
        r#"defenum Light {
  Red,
  Green,
}
impl Light {
  defextractor stop_code(self: Self) -> MatchResult<Int, Error> {
match self {
  Light::Red => MatchResult::Success(1),
  _ => MatchResult::NoMatch,
}
  }
}
light = Light::Red
print(match light {
  Light::stop_code(code) => to_string(code),
  _ => "fallback",
})"#,
    );
    let typed = typecheck(resolved).expect("enum impl extractor should typecheck");
    assert!(!typed.is_empty());
}

#[test]
fn forward_struct_type_annotation_and_literal_are_allowed() {
    let resolved = resolve_with_builtin_prelude(
        r#"user: User = User("alice", 30)
defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age }
  }
}"#,
    );
    let typed = typecheck(resolved).expect("forward struct reference should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::StructDef(_, _, _, _))));
}

#[test]
fn forward_deferror_value_can_flow_into_err() {
    let resolved = resolve_with_builtin_prelude(
        r#"ret: Result<Int> = Err(NotFound)
deferror NotFound {
  "not found"
}"#,
    );
    let typed = typecheck(resolved).expect("forward deferror constructor should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::DeferrorDef(_, _, _, _, _))));
}

#[test]
fn zero_arg_deferror_value_can_flow_into_error_parameter() {
    let resolved = resolve_with_builtin_prelude(
        r#"wrapped = Result::cause(Err(NoneError), NotFound)
deferror NotFound {
  "not found"
}"#,
    );
    let typed = typecheck(resolved).expect("zero-arg deferror should satisfy Error parameters");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Bind(_, _))));
}

#[test]
fn recover_kind_constructor_marker_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"value = Result::recover_kind(Err(NotFound("runtime")), NotFound("marker"), {|err| Ok(1)})
deferror NotFound(detail: String) {
  detail
}"#,
    );
    let typed = typecheck(resolved).expect("recover_kind constructor marker should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Bind(_, _))));
}

#[test]
fn forward_reference_type_tags_are_deterministic_across_runs() {
    let source = r#"user: User = User("alice", 30)
pair = Pair(first: 1, second: "two")
ret: Result<Int> = Err(NotFound("404"))

defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age }
  }
}

defrecord Pair(first: Int, second: String)

deferror NotFound(code: String) {
  "missing #{code}"
}"#;

    let first = typecheck_with_builtin_prelude(source);
    let second = typecheck_with_builtin_prelude(source);

    fn collect_type_tags(nodes: &[TypedNode]) -> Vec<(String, u32)> {
        nodes
            .iter()
            .filter_map(|node| match &node.node {
                TypedInner::StructDef(tag, name, _, _) | TypedInner::RecordDef(tag, name, _, _) => {
                    Some((name.clone(), *tag))
                }
                TypedInner::DeferrorDef(tag, _, id, _, _) => Some((id.name.clone(), *tag)),
                _ => None,
            })
            .collect()
    }

    assert_eq!(collect_type_tags(&first), collect_type_tags(&second));
}

#[test]
fn user_function_calls_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        "def add1(x: Int) -> Int { x + 1 }\nprint(to_string(add1(41)))",
    );
    assert!(
        typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::Def(..))),
        "expected user function definition to survive typechecking"
    );
}

#[test]
fn namespaced_type_and_trait_impl_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"namespace Auth {
  defrecord User(name: String)
}

impl Show for Auth::User {
  def to_string(self: Self) -> String { "user" }
}

value: Auth::User = Auth::User("alice")
print(to_string(value))"#,
    );
    assert!(
        typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::RecordDef(_, _, _, _))),
        "expected namespaced record definition to survive typechecking"
    );
    assert!(
        typed
            .iter()
            .any(|node| matches!(node.node, TypedInner::TraitImplDef(_, _))),
        "expected namespaced trait impl to survive typechecking"
    );
}

#[test]
fn generic_user_function_calls_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"def id(x: $A) -> $A { x }

print(to_string(id(1)))
print(id("ok"))"#,
    );
    assert!(
        typed
            .iter()
            .filter(|node| matches!(node.node, TypedInner::App(_, _)))
            .count()
            >= 2,
        "expected both generic function call sites to typecheck"
    );
}

#[test]
fn named_args_user_function_calls_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"def add(x: Int, y: Int) -> Int { x + y }
def add3(x: Int, y: Int, z: Int) -> Int { x + y + z }

print(to_string(add(y: 2, x: 1)))
print(to_string(add3(z: 3, y: 2, x: 1)))"#,
    );
    assert!(
        typed
            .iter()
            .filter(|node| matches!(node.node, TypedInner::Def(..)))
            .count()
            >= 2,
        "expected named-argument user functions to typecheck"
    );
}

#[test]
fn canonical_builtin_type_name_hole_is_reserved_for_structs() {
    let err = typecheck_module_source_result(
        r#"defstruct Hole {
  value: Int,
}"#,
    )
    .expect_err("Hole should be reserved");
    assert!(
        err.contains("Type name `Hole` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

#[test]
fn canonical_builtin_type_name_hole_is_reserved_for_enums() {
    let err = typecheck_module_source_result(
        r#"defenum Hole {
  Filled,
}"#,
    )
    .expect_err("Hole should be reserved");
    assert!(
        err.contains("Type name `Hole` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

#[test]
fn canonical_builtin_type_name_hole_is_reserved_for_errors() {
    let err = typecheck_module_source_result(
        r#"deferror Hole {
  "reserved"
}"#,
    )
    .expect_err("Hole should be reserved");
    assert!(
        err.contains("Type name `Hole` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

#[test]
fn canonical_builtin_type_name_closure_is_reserved_for_structs() {
    let err = typecheck_module_source_result(
        r#"defstruct Closure {
  value: Int,
}"#,
    )
    .expect_err("Closure should be reserved");
    assert!(
        err.contains("Type name `Closure` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

#[test]
fn canonical_builtin_type_name_match_arms_is_reserved_for_structs() {
    let err = typecheck_module_source_result(
        r#"defstruct MatchArms {
  value: Int,
}"#,
    )
    .expect_err("MatchArms should be reserved");
    assert!(
        err.contains("Type name `MatchArms` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

#[test]
fn canonical_builtin_type_name_cond_clauses_is_reserved_for_enums() {
    let err = typecheck_module_source_result(
        r#"defenum CondClauses {
  Clause,
}"#,
    )
    .expect_err("CondClauses should be reserved");
    assert!(
        err.contains("Type name `CondClauses` is reserved by a canonical builtin type declaration"),
        "unexpected error: {err}"
    );
}

#[test]
fn match_arms_type_is_forbidden_in_ordinary_user_signatures() {
    let err = typecheck_with_rules(
        r#"def bad(arms: MatchArms<Int, String>) -> String {
  "nope"
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("MatchArms should be restricted to special-form signatures");
    assert!(
        err.message
            .contains("MatchArms<$Scrutinee, $Result> is reserved for the `match` special form"),
        "unexpected error: {err}"
    );
}

#[test]
fn match_arms_type_is_forbidden_in_return_types() {
    let err = typecheck_with_rules(
        r#"def bad() -> MatchArms<Int, String> {
  "nope"
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("MatchArms should be restricted to special-form return types");
    assert!(
        err.message
            .contains("MatchArms<$Scrutinee, $Result> is reserved for the `match` special form"),
        "unexpected error: {err}"
    );
}

#[test]
fn cond_clauses_type_is_forbidden_in_ordinary_user_signatures() {
    let err = typecheck_with_rules(
        r#"def bad(clauses: CondClauses<String>) -> String {
  "nope"
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("CondClauses should be restricted to special-form signatures");
    assert!(
        err.message
            .contains("CondClauses<$Result> is reserved for the `cond` special form"),
        "unexpected error: {err}"
    );
}

#[test]
fn cond_clauses_type_is_forbidden_in_return_types() {
    let err = typecheck_with_rules(
        r#"def bad() -> CondClauses<String> {
  "nope"
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("CondClauses should be restricted to special-form return types");
    assert!(
        err.message
            .contains("CondClauses<$Result> is reserved for the `cond` special form"),
        "unexpected error: {err}"
    );
}

#[test]
fn trailing_block_calls_typecheck_inside_script_module_scope() {
    let typed = typecheck_with_builtin_prelude_in_script_module(
        r#"def take(flag: Boolean, value: (-> Int)) -> Int {
  if(flag, value(), 0)
}

print(to_string(take(True) { num = 10; num }))

v = if_then(True) { print("x") }
print(to_string(v))"#,
    );
    assert!(
        typed
            .iter()
            .filter(|node| matches!(node.node, TypedInner::App(_, _)))
            .count()
            >= 2,
        "expected trailing-block call sites to typecheck"
    );
}

#[test]
fn set_exit_code_is_allowed_in_script_rules() {
    let typed =
        typecheck_with_rules("set_exit_code(9)", RuntimeSourcePolicy::script()).expect("must pass");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::App(_, _))
    ));
}

#[test]
fn set_exit_code_is_forbidden_in_repl_chunk_rules() {
    let err = typecheck_with_rules("set_exit_code(9)", RuntimeSourcePolicy::repl_chunk())
        .expect_err("must fail");
    assert!(err.message.contains("forbidden by source policy"));
}

#[test]
fn set_exit_code_entry_only_policy_allows_only_entrypoint_function() {
    let entrypoint = EntryPoint::qualified("main");
    let rules = RuntimeSourcePolicy::module()
        .with_exit_code_policy(ExitCodePolicy::EntryOnly, Some(&entrypoint));

    let ok = typecheck_with_rules(
        r#"def main() -> Result<()> {
  set_exit_code(7)
  Ok(())
}"#,
        rules.clone(),
    )
    .expect("entrypoint body should allow set_exit_code");
    assert!(ok
        .iter()
        .find(|node| matches!(node.node, TypedInner::Def(..)))
        .is_some());

    let err = typecheck_with_rules(
        r#"def helper() -> Result<()> {
  set_exit_code(7)
  Ok(())
}"#,
        rules,
    )
    .expect_err("non-entrypoint function must fail");
    assert!(err.message.contains("only allowed inside entrypoint"));
}

#[test]
fn assert_special_form_typechecks_to_result_unit() {
    let typed = typecheck_with_builtin_prelude("guard = assert(True, NoneError())");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(rhs.node, TypedInner::Assert(_, _)));
            assert!(matches!(
                rhs.ty,
                scar::types::Ty::Result(ref ok, ref err)
                    if matches!(ok.as_ref(), scar::types::Ty::Unit)
                        && matches!(err.as_ref(), scar::types::Ty::Error)
            ));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn bitwidth_zero_arg_variant_reference_reuses_std_enum_constructor_uid() {
    let resolved = resolve_program_with_builtin_prelude("width = BitWidth::W8");

    let use_uid = match resolved
        .last()
        .expect("user bind should be present after std modules")
    {
        sigil::resolved::Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
            sigil::resolved::Resolved::ConstructorCall(_, id, args) => {
                assert!(args.is_empty(), "W8 should be zero-arg");
                id.unique_id
            }
            other => panic!("expected zero-arg constructor call, got {other:?}"),
        },
        other => panic!("expected user bind, got {other:?}"),
    };

    let variant_uid = resolved
        .iter()
        .find_map(|node| match node {
            sigil::resolved::Resolved::EnumDef(_, id, _, variants, _) if id.name == "BitWidth" => {
                variants
                    .iter()
                    .find(|variant| variant.id.name == "BitWidth::W8")
                    .map(|variant| variant.id.unique_id)
            }
            _ => None,
        })
        .expect("BitWidth::W8 variant should exist");

    assert_eq!(use_uid, variant_uid);

    let colliding_defs = resolved
        .iter()
        .filter_map(|node| match node {
            sigil::resolved::Resolved::BuiltinDecl(_, id, _, _, _) if id.unique_id == use_uid => {
                Some(format!("builtin {}", id.name))
            }
            sigil::resolved::Resolved::Def(_, id, _, _, _, _, _) if id.unique_id == use_uid => {
                Some(format!("def {}", id.name))
            }
            sigil::resolved::Resolved::ExtractorDef(_, id, _, _, _, _, _)
                if id.unique_id == use_uid =>
            {
                Some(format!("extractor {}", id.name))
            }
            sigil::resolved::Resolved::StructDef(_, id, _, _) if id.unique_id == use_uid => {
                Some(format!("struct {}", id.name))
            }
            sigil::resolved::Resolved::RecordDef(_, id, _) if id.unique_id == use_uid => {
                Some(format!("record {}", id.name))
            }
            sigil::resolved::Resolved::DeferrorDef(_, id, _, _) if id.unique_id == use_uid => {
                Some(format!("deferror {}", id.name))
            }
            sigil::resolved::Resolved::EnumDef(_, _, _, variants, _) => variants
                .iter()
                .find(|variant| variant.id.unique_id == use_uid)
                .map(|variant| format!("enum variant {}", variant.id.name)),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        colliding_defs,
        vec!["enum variant BitWidth::W8".to_string()],
        "unexpected declarations sharing uid {use_uid}: {colliding_defs:?}"
    );
}

#[test]
fn bitwidth_zero_arg_variant_typechecks_with_builtin_prelude() {
    let typed = typecheck_with_builtin_prelude("width = BitWidth::W8");
    assert!(matches!(
        typed.last().expect("user bind should be present").node,
        TypedInner::Bind(_, _)
    ));
}

#[test]
fn ensure_special_form_typechecks_to_result_value() {
    let typed = typecheck_with_builtin_prelude(
        r#"def is_even(n: Int) -> Boolean { Int::is_even(n) }
guard = ensure(4, &is_even, NoneError())"#,
    );
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(rhs.node, TypedInner::Ensure(_, _, _)));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn and_special_form_typechecks_to_boolean_if() {
    let typed = typecheck_with_builtin_prelude("flag = and(True, False)");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(rhs.node, TypedInner::If(_, _, Some(_))));
            assert!(matches!(rhs.ty, scar::types::Ty::Bool));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn eq_helper_typechecks_as_trait_call() {
    let typed = typecheck_with_builtin_prelude("flag = eq(1, 1)");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(
                rhs.node,
                TypedInner::TraitCall { ref method_name, .. } if method_name == "eq"
            ));
            assert!(matches!(rhs.ty, scar::types::Ty::Bool));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn lt_helper_typechecks_as_trait_call() {
    let typed = typecheck_with_builtin_prelude("flag = lt(1, 2)");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(
                rhs.node,
                TypedInner::TraitCall { ref method_name, .. } if method_name == "lt"
            ));
            assert!(matches!(rhs.ty, scar::types::Ty::Bool));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn concat_helper_typechecks_as_trait_call() {
    let typed = typecheck_with_builtin_prelude(r#"value = concat("a", "b")"#);
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(
                rhs.node,
                TypedInner::TraitCall { ref method_name, .. } if method_name == "concat"
            ));
            assert!(matches!(rhs.ty, scar::types::Ty::Str));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn to_string_helper_typechecks_as_trait_call() {
    let typed = typecheck_with_builtin_prelude("text = to_string(42)");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => {
            assert!(matches!(
                rhs.node,
                TypedInner::TraitCall { ref method_name, .. } if method_name == "to_string"
            ));
            assert!(matches!(rhs.ty, scar::types::Ty::Str));
        }
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn ensure_rejects_call_expression_predicate() {
    let err = typecheck_with_rules(
        r#"def is_even() -> (Int -> Boolean) { {|n| Int::is_even(n) } }
guard = ensure(4, is_even(), NoneError)"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("call expression predicate must fail");
    assert!(err.message.contains("ensure requires a closure or capture"));
}

#[test]
fn assert_rejects_non_concrete_error_expression() {
    let err = typecheck_with_rules(
        r#"def bad_code() -> Int { 1 }
guard = assert(False, bad_code())"#,
        RuntimeSourcePolicy::script(),
    )
    .expect_err("non-Error expression must fail");
    assert!(err
        .message
        .contains("assert error branch must evaluate to Error, got Int"));
}

#[test]
fn kernel_and_contract_rejects_eager_signature() {
    let err = typecheck_std_modules_with_overrides(&[(
        "Kernel",
        r#"defmod Kernel {
  @builtin def and(left: Boolean, right: Boolean) -> Boolean
}"#,
    )])
    .expect_err("eager signature should violate canonical contract");
    assert!(err
        .message
        .contains("@builtin def and(left: Boolean, right: Lazy<Boolean>) -> Boolean"));
}

#[test]
fn special_form_builtin_decl_must_live_under_kernel() {
    let err = typecheck_std_modules_with_overrides(&[(
        "Boolean",
        r#"@builtin type Boolean

impl Boolean {
  def not(value: Boolean) -> Boolean {
if(value, False, True)
  }

  @builtin def and(left: Boolean, right: Boolean) -> Boolean
}"#,
    )])
    .expect_err("special-form declaration outside Kernel must fail");
    assert!(err
        .message
        .contains("Special-form declaration `and` is only allowed in std module `Kernel`."));
}

#[test]
fn kernel_does_not_allow_removed_concat_builtin() {
    let module_stages = std_module_stages_with_overrides(&[(
        "Kernel",
        r#"defmod Kernel {
  @builtin def concat(left: $A, right: $A) -> String
}"#,
    )]);
    let declaration_index =
        sigil::precollect_declaration_index(&module_stages).expect("std modules should precollect");
    let err = sigil::resolve_staged_program(&module_stages, Vec::new(), &declaration_index, None)
        .expect_err("concat is no longer a declared runtime builtin");
    assert!(err.message.contains("Unknown builtin declaration: concat"));
}

#[test]
fn if_auto_forces_zero_arg_closure_once_for_branch_type() {
    let typed = typecheck_with_builtin_prelude("value = if(True, {|| 1}, 2)");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.ty, scar::types::Ty::Int)),
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn if_nested_closure_is_not_deep_forced() {
    let err = typecheck_with_rules(
        "value = if(True, {|| {|| 1}}, 2)",
        RuntimeSourcePolicy::script(),
    )
    .expect_err("nested lazy branch should not be deep forced");
    assert!(err
        .message
        .contains("if branches have different types: (-> Int) and Int"));
}

#[test]
fn user_lazy_annotation_is_rejected() {
    let err = typecheck_with_rules("x: Lazy<Int> = 1", RuntimeSourcePolicy::script())
        .expect_err("user lazy annotations must fail");
    assert!(err
        .message
        .contains("Lazy<T> is reserved for std-module special-form declarations"));
}

#[test]
fn assert_accepts_lazy_error_branch() {
    let typed = typecheck_with_rules(
        r#"deferror SomeError(detail: String) { detail }
guard = assert(False, {|| SomeError("boom") })"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("lazy error branch should typecheck");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.node, TypedInner::Assert(_, _))),
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn ensure_accepts_lazy_error_branch() {
    let typed = typecheck_with_rules(
        r#"deferror SomeError(detail: String) { detail }
def is_positive(value: Int) -> Boolean { value > 0 }
guard = ensure(-1, &is_positive, {|| SomeError("boom") })"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("lazy ensure error branch should typecheck");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.node, TypedInner::Ensure(_, _, _))),
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn assert_accepts_existing_error_value() {
    let typed = typecheck_with_rules(
        r#"guard = match Err(NoneError) {
  Ok(_) => assert(False, NoneError),
  Err(e) => assert(False, e),
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("existing Error value should typecheck");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.node, TypedInner::Match(_, _))),
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn ensure_accepts_existing_error_value() {
    let typed = typecheck_with_rules(
        r#"def is_positive(value: Int) -> Boolean { value > 0 }
guard = match Err(NoneError) {
  Ok(_) => ensure(-1, &is_positive, NoneError),
  Err(e) => ensure(-1, &is_positive, e),
}"#,
        RuntimeSourcePolicy::script(),
    )
    .expect("existing Error value should typecheck");
    let bind = typed.last().expect("binding should exist");
    match &bind.node {
        TypedInner::Bind(_, rhs) => assert!(matches!(rhs.node, TypedInner::Match(_, _))),
        other => panic!("expected bind, got {:?}", other),
    }
}

#[test]
fn generic_annotation_list_int_is_accepted() {
    let typed = typecheck_with_builtin_prelude("nums: List<Int> = [1, 2, 3]");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

#[test]
fn generic_def_signature_instantiates_per_call_site() {
    let typed = typecheck_with_builtin_prelude(
        r#"def id(x: $A) -> $A { x }
left: Int = id(1)
right: String = id("ok")"#,
    );
    assert!(typed.len() >= 3);
    assert!(typed
        .iter()
        .rev()
        .take(3)
        .all(|node| matches!(node.node, TypedInner::Bind(_, _) | TypedInner::Def(..))));
}

#[test]
fn generic_defenum_constructor_and_match_typecheck() {
    let typed = typecheck_with_builtin_prelude(
        r#"defenum StepSignal<$A> {
  Resume($A),
  Stop($A),
}

step: StepSignal<Int> = StepSignal::Resume(1)
value = match step {
  StepSignal::Resume(v) => v,
  StepSignal::Stop(v) => v,
}"#,
    );
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::EnumDef(_, _))));
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

#[test]
fn closure_param_annotation_without_expected_type_constrains_calls() {
    let resolved = resolve_with_builtin_prelude(
        r#"id = {|value: Int| value}
answer = id("oops")"#,
    );
    let err = typecheck(resolved).expect_err("annotation should reject String call");
    assert!(err.message.contains("expected Int, got String"));
}

#[test]
fn closure_application_mismatch_reports_callable_type_signature() {
    let resolved = resolve_with_builtin_prelude(
        r#"inc = {|n: Int| n + 1}
answer = inc("oops")"#,
    );
    let err = typecheck(resolved).expect_err("closure application should fail");
    assert!(err.message.contains("expected Int, got String"));
    let hint = err.hint.as_deref().expect("callable signature hint");
    assert!(hint.contains("Callable type signature: (Int -> Int)"));
}

#[test]
fn builtin_function_arity_reports_call_target_signature() {
    let resolved = resolve_with_builtin_prelude("value = print()");
    let err = typecheck(resolved).expect_err("builtin arity mismatch should fail");
    let hint = err.hint.as_deref().expect("builtin signature hint");
    assert_eq!(
        hint,
        "Call target signature: Kernel::print(arg1: String) -> Unit"
    );
}

#[test]
fn builtin_function_mismatch_reports_call_target_signature() {
    let resolved = resolve_with_builtin_prelude("value = print(1)");
    let err = typecheck(resolved).expect_err("builtin type mismatch should fail");
    let hint = err.hint.as_deref().expect("builtin signature hint");
    assert_eq!(
        hint,
        "Call target signature: Kernel::print(arg1: String) -> Unit"
    );
}

#[test]
fn capture_application_mismatch_reports_callable_type_signature() {
    let resolved = resolve_with_builtin_prelude_in_script_module(
        r#"def add(x: Int, y: Int) -> Int {
  x + y
}
bad = &add(&1, "oops")"#,
    )
    .expect("source should resolve");
    let err = typecheck(resolved).expect_err("capture application should fail");
    assert!(err.message.contains("expected Int, got String"));
    let hint = err
        .hint
        .as_deref()
        .expect("callable definition signature hint");
    assert!(hint.contains("Callable definition signature: add(x: Int, y: Int) -> Int"));
}

#[test]
fn script_callable_signature_omits_file_path_segments() {
    let resolved = resolve_with_builtin_prelude_in_module(
        r#"def add_one(x: Int) -> Int {
  x + 1
}
result = add_one("oops")"#,
        "__Script::Users::haruca::work::rust::surtr::surtr_compile_error_cases::type_call_arg_mismatch",
    )
    .expect("source should resolve");
    let err = typecheck(resolved).expect_err("function call should fail");
    let hint = err
        .hint
        .as_deref()
        .expect("callable definition signature hint");
    assert!(hint.contains("Callable definition signature: add_one(x: Int) -> Int"));
    assert!(!hint.contains("__Script::Users::haruca"));
    assert!(hint.contains("Callable definition span: 0.."));
}

#[test]
fn compose_mismatch_reports_left_and_right_callable_types() {
    let resolved = resolve_with_builtin_prelude(
        r#"def text(x: Int) -> String {
  to_string(x)
}

def inc(x: Int) -> Int {
  x + 1
}

bad = &text >> &inc"#,
    );
    let err = typecheck(resolved).expect_err("compose mismatch should fail");
    assert!(err.message.contains("left output type"));
    let hint = err.hint.as_deref().expect("compose mismatch hint");
    assert!(hint.contains("Left output is String; right input is Int"));
    assert!(hint.contains("LHS: (Int -> String)"));
    assert!(hint.contains("RHS: (Int -> Int)"));
}

#[test]
fn compose_accepts_calls_returning_function_values() {
    let resolved = resolve_with_builtin_prelude(
        r#"def make_inc() -> (Int -> Int) {
  {|x| x + 1}
}

def make_double() -> (Int -> Int) {
  {|x| x * 2}
}

plain = make_inc() >> make_double()"#,
    );
    typecheck(resolved).expect("compose should accept function-returning calls");
}

#[test]
fn compose_rejects_non_function_call_results_after_typechecking_call() {
    let resolved = resolve_with_builtin_prelude(
        r#"def inc(x: Int) -> Int {
  x + 1
}

plain = inc(1) >> inc(1)"#,
    );
    let err = typecheck(resolved).expect_err("compose should reject Int call results");
    assert_eq!(err.message, "`>>` requires a function value");
    let hint = err.hint.as_deref().expect("compose function-value hint");
    assert!(hint.contains("Call target signature:"));
    assert!(hint.contains("result type Int is not a function value"));
}

#[test]
fn pipe_plain_apply_over_result_reports_whole_lhs_mismatch() {
    let resolved = resolve_with_builtin_prelude(
        r#"def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def inc(x: Int) -> Int {
  x + 1
}

bad = parse(1) |> &inc"#,
    );
    let err = typecheck(resolved).expect_err("plain pipe over Result should fail");
    assert!(err.message.contains("expected Int, got Result<Int>"));
    let hint = err.hint.as_deref().expect("operator rule hint");
    assert!(hint.contains("`|>` signature rule"));
    assert!(hint.contains("LHS: Result<Int>"));
    assert!(hint.contains("RHS: (Int -> Int)"));
    assert!(!hint.contains("`|*>`"));
}

#[test]
fn context_bind_rejects_plain_rhs_return() {
    let resolved = resolve_with_builtin_prelude(
        r#"def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def inc(x: Int) -> Int {
  x + 1
}

bad = parse(1) |>= &inc"#,
    );
    let err = typecheck(resolved).expect_err("bind with plain RHS should fail");
    assert!(err
        .message
        .contains("requires the right-hand side to return Result, got Int"));
    let hint = err.hint.as_deref().expect("operator rule hint");
    assert!(hint.contains("`|>=` signature rule"));
    assert!(hint.contains("RHS: (Int -> Int)"));
    assert!(hint.contains("Use `|*>`"));
}

#[test]
fn context_map_keeps_result_for_later_bind() {
    let typed = typecheck_with_builtin_prelude(
        r#"def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def inc(x: Int) -> Int {
  x + 1
}

def stringify(x: Int) -> Result<String> {
  Ok(to_string(x))
}

ok = parse(1) |*> &inc |>= &stringify"#,
    );
    assert_eq!(typed.last().map(|node| &node.ty), Some(&Ty::Unit));
}

#[test]
fn context_map_and_bind_lower_to_operator_trait_calls() {
    let typed = typecheck_with_builtin_prelude(
        r#"def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def inc(x: Int) -> Int {
  x + 1
}

def stringify(x: Int) -> Result<String> {
  Ok(to_string(x))
}

mapped = parse(1) |*> &inc
bound = parse(1) |>= &stringify"#,
    );
    let trait_calls = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => match &rhs.node {
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    dispatch,
                    origin,
                    args,
                    ..
                } => Some((trait_name, method_name, dispatch, origin, args, &rhs.ty)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(trait_calls.iter().any(
        |(trait_name, method_name, dispatch, origin, args, result_ty)| {
            trait_name.starts_with("Functor<")
                && *method_name == "map"
                && matches!(
                    dispatch,
                    scar::typed::TraitDispatch::Static(
                        scar::typed::TraitDispatchTarget::UserFunction { name, .. }
                    ) if name.ends_with("::map") || name == "map"
                )
                && matches!(
                    origin,
                    TraitCallOrigin::Operator {
                        op: OperatorTraitOp::PipeMap,
                        lhs_ty: Ty::Result(_, _),
                        rhs_ty: Ty::Func(_, _) | Ty::UserFunc { .. } | Ty::BuiltinFunc { .. },
                    }
                )
                && args.len() == 2
                && matches!(result_ty, Ty::Result(ok, _) if matches!(ok.as_ref(), Ty::Int))
        }
    ));
    assert!(trait_calls.iter().any(
        |(trait_name, method_name, dispatch, origin, args, result_ty)| {
            trait_name.starts_with("Chainable<")
                && *method_name == "chain"
                && matches!(
                    dispatch,
                    scar::typed::TraitDispatch::Static(
                        scar::typed::TraitDispatchTarget::UserFunction { name, .. }
                    ) if name.ends_with("::chain") || name == "chain"
                )
                && matches!(
                    origin,
                    TraitCallOrigin::Operator {
                        op: OperatorTraitOp::PipeBind,
                        lhs_ty: Ty::Result(_, _),
                        rhs_ty: Ty::Func(_, _) | Ty::UserFunc { .. } | Ty::BuiltinFunc { .. },
                    }
                )
                && args.len() == 2
                && matches!(result_ty, Ty::Result(ok, _) if matches!(ok.as_ref(), Ty::Str))
        }
    ));
}

#[test]
fn explicit_functor_call_has_explicit_origin() {
    let typed = typecheck_with_builtin_prelude(
        r#"def inc(x: Int) -> Int {
  x + 1
}

mapped = Functor::map(Ok(1), &inc)"#,
    );
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    match &rhs.node {
        TypedInner::TraitCall {
            method_name,
            origin,
            ..
        } => {
            assert_eq!(method_name, "map");
            assert_eq!(origin, &TraitCallOrigin::Explicit);
            assert!(matches!(rhs.ty, Ty::Result(_, _)));
        }
        other => panic!("expected trait call, got {:?}", other),
    }
}

#[test]
fn flow_apply_and_compose_operators_lower_to_trait_calls() {
    let typed = typecheck_with_builtin_prelude(
        r#"def inc(x: Int) -> Int {
  x + 1
}

def show_int(x: Int) -> String {
  to_string(x)
}

def parse(x: Int) -> Result<Int> {
  Ok(x)
}

def parse_list(x: Int) -> List<Int> {
  [x]
}

def maybe_parse(x: Int) -> Option<Int> {
  Option::Some(x)
}

def maybe_show(x: Int) -> Option<String> {
  Option::Some(to_string(x))
}

applied = 1 |> &inc
plain = &inc >> &show_int
lifted = &parse >* &show_int
kleisli = &parse_list >=> {|x| [x, x + 1]}
lifted_option = &maybe_parse >* &show_int
kleisli_option = &maybe_parse >=> &maybe_show"#,
    );
    let calls = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => match &rhs.node {
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    origin,
                    ..
                } => Some((trait_name.as_str(), method_name.as_str(), origin, &rhs.ty)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(calls
        .iter()
        .any(|(trait_name, method_name, origin, result_ty)| {
            trait_name.starts_with("PipeApply<")
                && *method_name == "pipe_apply"
                && matches!(
                    origin,
                    TraitCallOrigin::Operator {
                        op: OperatorTraitOp::PipeApply,
                        lhs_ty: Ty::Int,
                        rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    }
                )
                && matches!(result_ty, Ty::Int)
        }));
    assert!(calls
        .iter()
        .any(|(trait_name, method_name, origin, result_ty)| {
            trait_name.starts_with("Composable<")
                && *method_name == "compose"
                && matches!(
                    origin,
                    TraitCallOrigin::Operator {
                        op: OperatorTraitOp::Compose,
                        lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                        rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    }
                )
                && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::Str))
        }));
    assert!(calls.iter().any(|(trait_name, method_name, origin, result_ty)| {
        trait_name.starts_with("LiftComposable<")
            && *method_name == "lift_compose"
            && matches!(
                origin,
                TraitCallOrigin::Operator {
                    op: OperatorTraitOp::LiftCompose,
                    lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                }
            )
            && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::Result(ok, _) if matches!(ok.as_ref(), Ty::Str)))
    }));
    assert!(calls
        .iter()
        .any(|(trait_name, method_name, origin, result_ty)| {
            trait_name.starts_with("KleisliComposable<")
                && *method_name == "kleisli_compose"
                && matches!(
                    origin,
                    TraitCallOrigin::Operator {
                        op: OperatorTraitOp::KleisliCompose,
                        lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                        rhs_ty: Ty::Func(_, _),
                    }
                )
                && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::List(_)))
        }));
    assert!(calls.iter().any(|(trait_name, method_name, origin, result_ty)| {
        trait_name.starts_with("LiftComposable<")
            && *method_name == "lift_compose"
            && matches!(
                origin,
                TraitCallOrigin::Operator {
                    op: OperatorTraitOp::LiftCompose,
                    lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                }
            )
            && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::Enum(name, args) if name == "Option" && matches!(args.as_slice(), [Ty::Str])))
    }));
    assert!(calls.iter().any(|(trait_name, method_name, origin, result_ty)| {
        trait_name.starts_with("KleisliComposable<")
            && *method_name == "kleisli_compose"
            && matches!(
                origin,
                TraitCallOrigin::Operator {
                    op: OperatorTraitOp::KleisliCompose,
                    lhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                    rhs_ty: Ty::UserFunc { .. } | Ty::Func(_, _) | Ty::BuiltinFunc { .. },
                }
            )
            && matches!(result_ty, Ty::Func(_, ret) if matches!(ret.as_ref(), Ty::Enum(name, args) if name == "Option" && matches!(args.as_slice(), [Ty::Str])))
    }));
}

#[test]
fn user_defined_container_can_use_context_operators_via_traits() {
    let typed = typecheck_with_builtin_prelude(
        r#"defenum Boxed<$T> {
  Box($T),
}

impl Functor<$A, $B, Boxed<$B>> for Boxed<$A> {
  def map(self: Self, f: ($A -> $B)) -> Boxed<$B> {
    match self {
      Boxed::Box(value) => Boxed::Box(f(value)),
    }
  }
}

impl Chainable<$A, Boxed<$B>> for Boxed<$A> {
  def chain(self: Self, f: ($A -> Boxed<$B>)) -> Boxed<$B> {
    match self {
      Boxed::Box(value) => f(value),
    }
  }
}

def inc(x: Int) -> Int {
  x + 1
}

def stringify(x: Int) -> Boxed<String> {
  Boxed::Box(to_string(x))
}

mapped = Boxed::Box(1) |*> &inc
bound = Boxed::Box(1) |>= &stringify"#,
    );

    let boxed_results = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(&rhs.ty),
            _ => None,
        })
        .filter(|ty| matches!(ty, Ty::Enum(name, _) if name == "Boxed"))
        .count();
    assert_eq!(boxed_results, 2);
}

#[test]
fn result_match_wildcard_self_after_ok_can_change_ok_payload_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"def remap(value: Result<Int>) -> Result<String> {
  match value {
    Ok(inner) => Ok(to_string(inner)),
    _ => value,
  }
}"#,
    );

    let typed = typecheck(resolved).expect("Err-proven wildcard arm should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Def(_, _, _, _, _, _, _))));
}

#[test]
fn result_match_wildcard_self_after_ok_can_keep_err_for_bind_shape() {
    let resolved = resolve_with_builtin_prelude(
        r#"def bind_like(value: Result<Int>) -> Result<String> {
  match value {
    Ok(inner) => Ok(to_string(inner)),
    _ => value,
  }
}"#,
    );

    let typed = typecheck(resolved).expect("Err-proven bind-style wildcard arm should typecheck");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::Def(_, _, _, _, _, _, _))));
}

#[test]
fn result_match_wildcard_self_requires_err_proven_branch() {
    let resolved = resolve_with_builtin_prelude(
        r#"def bad(value: Result<Int>) -> Result<String> {
  match value {
    _ => value,
    Ok(inner) => Ok(to_string(inner)),
  }
}"#,
    );

    let err = typecheck(resolved).expect_err("wildcard arm without prior Ok coverage must fail");
    assert!(
        err.message.contains("Match arm type mismatch")
            || err
                .message
                .contains("expected Result<String>, got Result<Int>")
            || err
                .message
                .contains("expected Result<Int>, got Result<String>")
    );
}

#[test]
fn closure_param_annotation_must_match_expected_signature() {
    let resolved = resolve_with_builtin_prelude(r#"id: (String -> String) = {|value: Int| value}"#);
    let err = typecheck(resolved).expect_err("mismatched expected signature must fail");
    assert!(err
        .message
        .contains("closure parameter `value` expected String, got Int"));
}

#[test]
fn sibling_closures_keep_substitution_state_local() {
    let typed = typecheck_with_builtin_prelude(
        r#"int_id: (Int -> Int) = {|value| value}
str_id: (String -> String) = {|value| value}
left: Int = int_id(1)
right: String = str_id("ok")"#,
    );
    assert!(typed.len() >= 4);
    assert!(typed
        .iter()
        .rev()
        .take(4)
        .all(|node| matches!(node.node, TypedInner::Bind(_, _))));
}

#[test]
fn cyclic_type_definition_is_rejected() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Node {
  next: Node,
}"#,
    );
    let err = typecheck(resolved).expect_err("cyclic type must fail");
    assert!(err.message.contains("Cyclic type definition detected"));
}

#[test]
fn enum_cycle_is_allowed_when_not_shared_by_all_variants() {
    let resolved = resolve_with_builtin_prelude(
        r#"defenum Loop {
  End,
  Next(Loop),
}
value: Loop = Loop::End"#,
    );
    let typed = typecheck(resolved).expect("enum should allow conditional recursion");
    assert!(typed
        .iter()
        .any(|node| matches!(node.node, TypedInner::EnumDef(_, _))));
}

#[test]
fn enum_cycle_is_rejected_when_shared_by_all_variants() {
    let resolved = resolve_with_builtin_prelude(
        r#"defenum Loop {
  A(Loop),
  B(Loop),
}"#,
    );
    let err = typecheck(resolved).expect_err("enum cycle must fail");
    assert!(err.message.contains("Cyclic type definition detected"));
}

#[test]
fn enum_field_access_is_rejected() {
    let resolved = resolve_with_builtin_prelude(
        r#"defenum Direction {
  Up,
  Down,
}
up: Direction = Direction::Up
x = up.idx"#,
    );
    let err = typecheck(resolved).expect_err("enum field access must fail");
    assert!(err
        .message
        .contains("No variant selector 'idx' on Direction"));
}

#[test]
fn match_binding_pattern_is_treated_as_exhaustive() {
    let resolved = resolve_with_builtin_prelude(
        r#"flag = True
answer = match flag {
  value => value,
}"#,
    );
    let typed = typecheck(resolved).expect("binding arm should be exhaustive");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

#[test]
fn match_tuple_binding_pattern_is_treated_as_exhaustive() {
    let resolved = resolve_with_builtin_prelude(
        r#"pair = (1, "two")
answer = match pair {
  (left, right) => right,
}"#,
    );
    let typed = typecheck(resolved).expect("tuple binding arm should be exhaustive");
    assert!(matches!(
        typed.last().map(|node| &node.node),
        Some(TypedInner::Bind(_, _))
    ));
}

#[test]
fn match_guard_must_be_boolean() {
    let resolved = resolve_with_builtin_prelude(
        r#"answer = match 1 {
  n when 1 => n,
  _ => 0,
}"#,
    );
    let err = typecheck(resolved).expect_err("non-boolean guard must fail");
    assert!(err.message.contains("match guard must be Boolean, got Int"));
}

#[test]
fn guarded_match_arm_does_not_satisfy_exhaustiveness() {
    let resolved = resolve_with_builtin_prelude(
        r#"answer = match True {
  flag when flag => 1,
}"#,
    );
    let err = typecheck(resolved).expect_err("guarded-only arm must be non-exhaustive");
    assert!(err
        .message
        .contains("Non-exhaustive match. Missing: True, False"));
}

#[test]
fn struct_literal_rejects_extra_fields() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
User { name: name, age: age, extra: 1 }
  }
}
user = User("alice", 20)"#,
    );
    let err = typecheck(resolved).expect_err("extra fields must fail");
    assert!(err.message.contains("Unknown field 'extra' in User"));
}

#[test]
fn constructor_named_args_reject_duplicate_fields() {
    let resolved = resolve_with_builtin_prelude(
        r#"defrecord Pair(first: Int, second: String)
pair = Pair(first: 1, first: 2)"#,
    );
    let err = typecheck(resolved).expect_err("duplicate named args must fail");
    assert!(err.message.contains("Duplicate field 'first' in Pair"));
}

#[test]
fn struct_literal_field_shorthand_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
User { name, age }
  }
}
user = User("alice", 20)"#,
    );
    typecheck(resolved).expect("struct shorthand should typecheck");
}

#[test]
fn struct_literal_field_shorthand_mixed_with_explicit_typechecks() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def rename(self, name: String, next_age: Int) -> Self {
User { name, age: next_age }
  }

  def new(name: String, age: Int) -> Self {
    User::rename(User { name, age }, name, age)
  }
}"#,
    );
    typecheck(resolved).expect("mixed struct shorthand should typecheck");
}

#[test]
fn struct_literal_field_shorthand_rejects_duplicate_fields() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
User { name, name: name }
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("duplicate shorthand field must fail");
    assert!(err.message.contains("Duplicate field 'name' in User"));
}

#[test]
fn struct_requires_impl_new() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
user = User("alice")"#,
    );
    let err = typecheck(resolved).expect_err("struct without new should fail");
    assert!(err.message.contains("must define `new` in its impl block"));
}

#[test]
fn struct_new_accepts_result_self_return_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Duration {
  private millis: Int,
}
impl Duration {
  def new(value: Int) -> Result<Self, Error> {
    Ok(Duration { millis: value })
  }
}
value: Result<Duration> = Duration(10)"#,
    );
    let typed = typecheck(resolved).expect("Result<Self, Error> constructor should pass");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");
    assert!(matches!(
        &rhs.ty,
        Ty::Result(ok, err)
            if matches!(ok.as_ref(), Ty::Struct(name, _) if name == "Duration")
                && matches!(err.as_ref(), Ty::Error)
    ));
}

#[test]
fn struct_new_rejects_non_self_return_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Int {
    1
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("non-Self constructor return must fail");
    assert!(err
        .message
        .contains("`new` must return Self or Result<Self, E>"));
}

#[test]
fn struct_new_rejects_result_non_self_payload() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Result<List<Self>, Error> {
    Ok([User { name: name }])
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("Result payload must be Self");
    assert!(err
        .message
        .contains("`new` must return Self or Result<Self, E>"));
}

#[test]
fn struct_constructor_call_accepts_result_return_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct Duration {
  private millis: Int,
}
impl Duration {
  def new(value: Int) -> Result<Self, Error> {
    Ok(Duration { millis: value })
  }
}
dur = Duration(10)"#,
    );
    let typed = typecheck(resolved).expect("constructor call should accept Result<Self>");
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("expected binding");
    assert!(matches!(
        &rhs.ty,
        Ty::Result(ok, err)
            if matches!(ok.as_ref(), Ty::Struct(name, _) if name == "Duration")
                && matches!(err.as_ref(), Ty::Error)
    ));
}

#[test]
fn struct_literal_is_rejected_outside_impl_body() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
User { name: name }
  }
}
user = User { name: "alice" }"#,
    );
    let err = typecheck(resolved).expect_err("struct literal outside impl should fail");
    assert!(err
        .message
        .contains("Struct literal `User` is only allowed inside"));
}

#[test]
fn user_function_call_rejects_mixed_named_and_positional_args() {
    let resolved = resolve_with_builtin_prelude(
        r#"def add3(x: Int, y: Int, z: Int) -> Int { x + y + z }
value = add3(1, y: 2, z: 3)"#,
    );
    let err = typecheck(resolved).expect_err("mixed args should fail");
    assert!(err
        .message
        .contains("Cannot mix positional and named arguments"));
}

#[test]
fn impl_self_rebinding_allows_self_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}

impl User {
  def new(name: String) -> Self {
User { name: name }
  }

  def keep(self) -> Self {
self = self
self
  }
}

user = User("alice")
print(to_string(User::keep(user).name))"#,
    );
    let _typed = typecheck(resolved).expect("self rebinding with Self should pass");
}

#[test]
fn impl_self_rebinding_rejects_non_self_type() {
    let resolved = resolve_with_builtin_prelude(
        r#"defstruct User {
  name: String,
}

impl User {
  def new(name: String) -> Self {
User { name: name }
  }

  def bad(self) -> Self {
self = 1
self
  }
}"#,
    );
    let err = typecheck(resolved).expect_err("self rebinding with non-Self must fail");
    assert!(err.message.contains("`self` rebinding requires Self type"));
}

#[test]
fn deferror_show_type_mismatch_points_to_show_expression_span() {
    let source = r#"deferror NotFound(code: String) {
  123
}"#;
    let resolved = resolve_with_builtin_prelude(source);
    let err = typecheck(resolved).expect_err("show block must return String");
    let literal_start = source.find("123").expect("literal should exist in source");
    assert!(err
        .message
        .contains("deferror show block must return String"));
    assert_eq!(err.span.start, literal_start);
}

#[test]
fn operator_and_numeric_trait_calls_typecheck_with_static_dispatch() {
    let typed = typecheck_with_builtin_prelude(
        r#"sum = 1 + 2
quot = Numeric::safe_div(8, 2)
largest = Numeric::max(1.5, 2.5)"#,
    );

    let trait_calls = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => match &rhs.node {
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    dispatch,
                    ..
                } => Some((trait_name.as_str(), method_name.as_str(), dispatch)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(trait_calls
        .iter()
        .any(|(trait_name, method_name, dispatch)| {
            *trait_name == "Add"
                && *method_name == "add"
                && matches!(
                    dispatch,
                    scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::BinOp(
                        spire::ast::BinOp::Add
                    ))
                )
        }));
    assert!(trait_calls
        .iter()
        .any(|(trait_name, method_name, dispatch)| {
            *trait_name == "Numeric"
                && *method_name == "safe_div"
                && matches!(
                    dispatch,
                    scar::typed::TraitDispatch::Static(
                        scar::typed::TraitDispatchTarget::Builtin(name)
                    ) if name == "safe_div"
                )
        }));
    assert!(trait_calls
        .iter()
        .any(|(trait_name, method_name, dispatch)| {
            *trait_name == "Numeric"
                && *method_name == "max"
                && matches!(
                    dispatch,
                    scar::typed::TraitDispatch::Static(
                        scar::typed::TraitDispatchTarget::UserFunction { name, .. }
                    ) if name == "Float::max"
                )
        }));
}

#[test]
fn duration_operator_traits_dispatch_to_surtr_impls() {
    let typed = typecheck_with_builtin_prelude(
        r#"sum = 10ms + 20ms
same = 10ms == 10ms
less = 10ms < 20ms"#,
    );

    let trait_calls = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => match &rhs.node {
                TypedInner::TraitCall {
                    method_name,
                    dispatch,
                    ..
                } => Some((method_name.as_str(), dispatch)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    for (method, expected_name) in [
        ("add", "Duration::add"),
        ("eq", "Duration::eq"),
        ("lt", "Duration::lt"),
    ] {
        assert!(
            trait_calls.iter().any(|(method_name, dispatch)| {
                *method_name == method
                    && matches!(
                        dispatch,
                        scar::typed::TraitDispatch::Static(
                            scar::typed::TraitDispatchTarget::UserFunction { name, .. }
                        ) if name == expected_name
                    )
            }),
            "{method} should dispatch to {expected_name}"
        );
    }
}

#[test]
fn bounded_add_generics_specialize_without_pending_trait_calls() {
    fn has_pending_trait_call(node: &TypedNode) -> bool {
        match &node.node {
            TypedInner::TraitCall { dispatch, args, .. } => {
                matches!(dispatch, scar::typed::TraitDispatch::Pending)
                    || args.iter().any(has_pending_trait_call)
            }
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                has_pending_trait_call(func) || args.iter().any(has_pending_trait_call)
            }
            TypedInner::Block(stmts) => stmts.iter().any(has_pending_trait_call),
            TypedInner::Bind(_, rhs)
            | TypedInner::SafeBind(_, rhs)
            | TypedInner::Semi(rhs)
            | TypedInner::FieldAccess(rhs, _) => has_pending_trait_call(rhs),
            TypedInner::ProcessContextHandler { .. } => false,
            TypedInner::LensPath(_) | TypedInner::PendingLensPath(_) => false,
            TypedInner::LensView { source, .. } => has_pending_trait_call(source),
            TypedInner::LensSet { source, value, .. } => {
                has_pending_trait_call(source) || has_pending_trait_call(value)
            }
            TypedInner::LensOver {
                source, update_fun, ..
            } => has_pending_trait_call(source) || has_pending_trait_call(update_fun),
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => {
                has_pending_trait_call(left) || has_pending_trait_call(right)
            }
            TypedInner::TupleLiteral(items)
            | TypedInner::ListLiteral(items)
            | TypedInner::ConstructorCall(_, items)
            | TypedInner::StructLit(_, items) => items.iter().any(has_pending_trait_call),
            TypedInner::If(cond, then_branch, else_branch) => {
                has_pending_trait_call(cond)
                    || has_pending_trait_call(then_branch)
                    || else_branch.as_deref().is_some_and(has_pending_trait_call)
            }
            TypedInner::Assert(cond, err) => {
                has_pending_trait_call(cond) || has_pending_trait_call(err)
            }
            TypedInner::Ensure(value, pred, err) => {
                has_pending_trait_call(value)
                    || has_pending_trait_call(pred)
                    || has_pending_trait_call(err)
            }
            TypedInner::MapErr(value, err) | TypedInner::Cause(value, err) => {
                has_pending_trait_call(value) || has_pending_trait_call(err)
            }
            TypedInner::RecoverKind(value, marker, handler) => {
                has_pending_trait_call(value)
                    || has_pending_trait_call(marker)
                    || has_pending_trait_call(handler)
            }
            TypedInner::Match(scrutinee, arms) => {
                has_pending_trait_call(scrutinee)
                    || arms.iter().any(|arm| {
                        arm.guard.as_ref().is_some_and(has_pending_trait_call)
                            || has_pending_trait_call(&arm.body)
                    })
            }
            TypedInner::InterpolatedStr(parts) => parts.iter().any(|part| match part {
                scar::typed::TypedInterpolatedPart::Text(_) => false,
                scar::typed::TypedInterpolatedPart::Expr(expr) => has_pending_trait_call(expr),
            }),
            TypedInner::Dbg(args) => args.iter().any(|arg| has_pending_trait_call(&arg.expr)),
            TypedInner::Def(_, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body) => has_pending_trait_call(body),
            TypedInner::Lit(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::DeferrorDef(..)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..) => false,
        }
    }

    let typed = typecheck_with_builtin_prelude(
        r#"def double<$N: Add>(x: $N) -> $N { x + x }
a = double(21)
b = double(1.5)"#,
    );

    let double_defs = typed
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Def(fun_idx, id, ..) if id.name == "double" => Some(*fun_idx),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(double_defs.len(), 2);
    assert_ne!(double_defs[0], double_defs[1]);
    assert!(!typed.iter().any(has_pending_trait_call));
}

#[test]
fn scar_session_preserves_trait_registry_across_chunks() {
    let mut session = session_from_cached_std_prelude();
    let user_resolved = resolve_with_builtin_prelude("value = 1 + 2");
    let typed = session
        .typecheck(user_resolved)
        .expect("trait registry should survive across chunks");

    assert!(typed.iter().any(|node| {
        matches!(
            &node.node,
            TypedInner::Bind(_, rhs)
                if matches!(
                    &rhs.node,
                    TypedInner::TraitCall {
                        method_name,
                        dispatch: scar::typed::TraitDispatch::Static(
                            scar::typed::TraitDispatchTarget::BinOp(spire::ast::BinOp::Add)
                        ),
                        ..
                    } if method_name == "add"
                )
        )
    }));
}

#[test]
fn add_trait_mismatch_lists_available_implementations() {
    let resolved = resolve_with_builtin_prelude("value = Add::add(1, False)");
    let err = typecheck(resolved).expect_err("mismatched add trait call must fail");
    assert!(err.message.contains("Add::add expects argument 2"));
    assert!(err.message.contains("receiver type Int"));
    assert!(err.message.contains("got Boolean"));
    let hint = err.hint.as_deref().expect("trait summary hint");
    assert!(hint.contains("Call target signature: Add::add"));
    assert!(hint.contains("Add is implemented for: Duration, Float, Int"));
}

#[test]
fn add_trait_missing_receiver_lists_available_implementations() {
    let resolved = resolve_with_builtin_prelude("value = Add::add(False, True)");
    let err = typecheck(resolved).expect_err("invalid add receiver must fail");
    assert!(err
        .message
        .contains("Add::add requires a receiver type implementing Add, got Boolean"));
    let hint = err.hint.as_deref().expect("trait summary hint");
    assert!(hint.contains("Call target signature: Add::add"));
    assert!(hint.contains("Add is implemented for: Duration, Float, Int"));
}

#[test]
fn add_operator_missing_impl_lists_available_implementations_in_hint() {
    let resolved = resolve_with_builtin_prelude("value = False + True");
    let err = typecheck(resolved).expect_err("invalid add operator must fail");
    assert!(err
        .message
        .contains("`+` requires both operands to implement Add"));
    let hint = err.hint.as_deref().expect("operator hint");
    assert!(hint.contains("Add is implemented for: Duration, Float, Int"));
}

#[test]
fn bind_operator_missing_impl_lists_available_implementations_in_hint() {
    let resolved = resolve_with_builtin_prelude("value = 1 |>= {|x| Ok(x)}");
    let err = typecheck(resolved).expect_err("plain lhs bind must fail");
    assert!(err
        .message
        .contains("`|>=` requires Chainable implementation on the left, got Int"));
    let hint = err.hint.as_deref().expect("bind hint");
    assert!(hint.contains("Chainable is implemented for:"));
    assert!(hint.contains("List<$A>"));
    assert!(hint.contains("Option<$A>"));
    assert!(hint.contains("Result<$A>"));
}

#[test]
fn from_helper_typechecks_as_generic_trait_call() {
    let typed = typecheck_with_builtin_prelude(r#"value = from(42, String)"#);
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    match &rhs.node {
        TypedInner::TraitCall {
            trait_name,
            method_name,
            receiver_ty,
            dispatch:
                scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::UserFunction {
                    name,
                    ..
                }),
            args,
            ..
        } => {
            assert_eq!(trait_name, "From<String>");
            assert_eq!(method_name, "from");
            assert_eq!(name, "From<String>::Int::from");
            assert_eq!(receiver_ty, &scar::types::Ty::Int);
            assert!(matches!(args[1].ty, scar::types::Ty::TypeRef(_)));
            assert_eq!(rhs.ty, scar::types::Ty::Str);
        }
        other => panic!("expected trait call, got {:?}", other),
    }
}

#[test]
fn try_from_helper_typechecks_as_generic_trait_call() {
    let typed = typecheck_with_builtin_prelude(r#"value = try_from("42", Int)"#);
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    match &rhs.node {
        TypedInner::TraitCall {
            trait_name,
            method_name,
            receiver_ty,
            dispatch:
                scar::typed::TraitDispatch::Static(scar::typed::TraitDispatchTarget::UserFunction {
                    name,
                    ..
                }),
            args,
            ..
        } => {
            assert_eq!(trait_name, "TryFrom<Int>");
            assert_eq!(method_name, "try_from");
            assert_eq!(name, "TryFrom<Int>::String::try_from");
            assert_eq!(receiver_ty, &scar::types::Ty::Str);
            assert!(matches!(args[1].ty, scar::types::Ty::TypeRef(_)));
            assert!(matches!(rhs.ty, scar::types::Ty::Result(_, _)));
        }
        other => panic!("expected trait call, got {:?}", other),
    }
}

#[test]
fn from_helper_suggests_try_from_when_only_fallible_impl_exists() {
    let resolved = resolve_with_builtin_prelude(r#"value = from("42", Int)"#);
    let err = typecheck(resolved).expect_err("from on fallible conversion must fail");
    assert!(err
        .message
        .contains("String -> Int implements TryFrom, not From"));
    assert!(err.message.contains("Use try_from(value, Int)."));
}

#[test]
fn try_from_helper_suggests_from_when_only_infallible_impl_exists() {
    let resolved = resolve_with_builtin_prelude(r#"value = try_from(42, String)"#);
    let err = typecheck(resolved).expect_err("try_from on infallible conversion must fail");
    assert!(err
        .message
        .contains("Int -> String implements From, not TryFrom"));
    assert!(err.message.contains("Use from(value, String)."));
}

#[test]
fn from_and_try_from_impls_are_mutually_exclusive() {
    let overrides = [
        (
            "String",
            r#"@builtin type String

defenum StringEncoding {
  Utf8,
  Ascii,
}

deferror InvalidStringEncoding(detail: String) {
  detail
}

impl String {
  @builtin
  def codepoints(value: String, encoding: StringEncoding) -> Result<List<Int>, InvalidStringEncoding>

  @builtin
  def from_codepoints(values: List<Int>, encoding: StringEncoding) -> Result<String, InvalidStringEncoding>
}

impl Show for String {
  def to_string(self: Self) -> String {
inspect(self)
  }
}

impl From<String> for String {
  def from(self: Self, to: TypeRef<String>) -> String {
self
  }
}

impl TryFrom<Int> for String {
  def try_from(self: Self, to: TypeRef<Int>) -> Result<Int, Error> {
Ok(0)
  }
}

impl From<Int> for String {
  def from(self: Self, to: TypeRef<Int>) -> Int {
0
  }
}

impl Eq for String {
  def eq(self: Self, rhs: Self) -> Boolean {
self == rhs
  }

  def neq(self: Self, rhs: Self) -> Boolean {
self != rhs
  }
}"#,
        ),
        ("StyledDoc", "defmod StyledDoc {}"),
        ("Test", "defmod Test {}"),
    ];

    let err = typecheck_std_modules_with_overrides(&overrides)
        .expect_err("conflicting From/TryFrom impls must fail");
    assert!(err
        .message
        .contains("From and TryFrom cannot both be implemented for String -> Int"));
}

#[test]
fn process_sleep_accepts_duration_literal() {
    let typed = typecheck_with_builtin_prelude(r#"value = Process::sleep(100ms)"#);
    let rhs = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("bind rhs should exist");
    assert!(matches!(rhs.ty, scar::types::Ty::Result(_, _)));
}

#[test]
fn process_self_is_rejected_outside_process_context() {
    let resolved = resolve_with_builtin_prelude(r#"pid = Process::self()"#);
    let err = typecheck(resolved).expect_err("Process::self outside process must fail");
    assert!(err.message.contains("Process::self"));
}

#[test]
fn process_self_typechecks_inside_process_handler() {
    typecheck_module_source_result(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<PID<Counter>> {
    Ok(Process::self())
  }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
    )
    .expect("Process::self should typecheck inside process handler");
}

#[test]
fn typecheck_staged_program_keeps_process_specs() {
    let ast = spire::parse_with_context(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
        spire::ParserContext::module(0, Some("Counter".to_string())),
    )
    .expect("defagent source should parse");

    let staged_module = match ast.into_iter().next().expect("lowered module should exist") {
        spire::ast::Ast::Defagent(_, module_path, ast, process_spec, attrs) => {
            sigil::StagedModuleAst {
                module_path,
                doc_module_path: None,
                ast,
                module_doc: attrs.doc,
                auto_import: attrs.auto_import,
                process_spec: Some(process_spec),
            }
        }
        other => panic!("expected defagent, got {other:?}"),
    };

    let mut stages = std_module_stages();
    stages.push(vec![staged_module]);
    let declaration_index =
        sigil::precollect_declaration_index(&stages).expect("precollect should succeed");
    let resolved =
        sigil::resolve_staged_program_with_state(&stages, Vec::new(), &declaration_index, None)
            .expect("resolve should succeed");
    let typed: TypedProgram =
        crate::typecheck_staged_program(resolved).expect("typecheck should succeed");

    assert_eq!(typed.process_specs.len(), 1);
    let spec = &typed.process_specs[0];
    assert_eq!(spec.module_path, "Counter");
    assert_eq!(spec.process_name, "Counter");
    assert!(!spec.spec.boot);
}
