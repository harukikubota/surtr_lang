use super::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Instant;

// Count only allocations made by the measured test thread, excluding input construction.
struct CountingAllocator;
thread_local! { static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) }; }
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get().map(|n| n + 1)));
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get().map(|n| n + 1)));
        System.realloc(ptr, layout, size)
    }
}
fn leaf(tag: u32) -> CanonicalTy {
    CanonicalTy::new(CanonicalTypeHead::Nominal(tag), vec![])
}
fn nested(depth: usize, mut ty: CanonicalTy) -> CanonicalTy {
    for _ in 0..depth {
        ty = CanonicalTy::new(CanonicalTypeHead::Nominal(10), vec![ty]);
    }
    ty
}
fn hole() -> CanonicalTy {
    CanonicalTy::new(CanonicalTypeHead::Hole, vec![])
}
fn callable(input: CanonicalTy, output: CanonicalTy) -> CanonicalTy {
    CanonicalTy::new(CanonicalTypeHead::Function, vec![input, output])
}
#[test]
fn canonical_unifier_preserves_boundaries() {
    let v = CanonicalTy::variable;
    let mut u = CanonicalUnifier::default();
    assert!(u.unify(&v(0), &v(0)));
    assert!(!u.unify(&v(0), &nested(2, v(0))));
    assert!(u.unify(&v(0), &nested(1, v(1))));
    assert!(!u.unify(&v(1), &nested(1, v(0))));
    assert!(u.unify(&v(1), &leaf(1)));
    assert_eq!(u.resolve(&v(0)), nested(1, leaf(1)));
    assert!(!u.unify(&v(0), &nested(1, leaf(2))));
    let mut u = CanonicalUnifier {
        rigid_variables: HashSet::from([0, 1]),
        ..Default::default()
    };
    assert!(u.unify(&v(0), &v(0)));
    assert!(!u.unify(&v(0), &v(1)));
    assert!(!u.unify(&v(0), &leaf(1)));
    assert!(!u.unify(&leaf(1), &v(0)));
    assert!(u.unify(&v(0), &v(2)));
    assert!(u.unify(&v(3), &v(1)));
    assert_eq!(u.resolve(&v(2)), v(0));
    assert_eq!(u.resolve(&v(3)), v(1));
    let mut u = CanonicalUnifier::default();
    assert!(u.unify(&v(0), &hole()));
    assert!(!u.unify(&callable(leaf(1), leaf(2)), &callable(v(0), leaf(2))));
    assert!(!u.unify(&callable(hole(), leaf(2)), &callable(leaf(1), leaf(2))));
    assert!(!u.unify(&callable(leaf(1), leaf(2)), &callable(hole(), hole())));
    assert!(
        !CanonicalUnifier::default().unify(&callable(leaf(1), leaf(2)), &callable(hole(), leaf(2)))
    );
    assert!(!u.unify(&leaf(10), &nested(1, leaf(1))));
    assert!(u.unify_constructor_identity(&nested(2, hole()), &nested(2, leaf(1))));
    assert!(!u.unify_constructor_identity(&nested(2, leaf(1)), &nested(2, hole())));
    assert!(u.unify_constructor_identity(&v(1), &nested(2, leaf(1))));
    assert_eq!(u.resolve(&v(1)), nested(2, leaf(1)));
}
#[test]
fn canonical_unifier_nested_allocation_growth() {
    let mut results = vec![];
    for depth in [32, 64, 128, 256] {
        for mode in [
            "equal",
            "different",
            "variable",
            "constructor",
            "constructor_different",
            "constructor_hole",
        ] {
            let left = nested(
                depth,
                if mode == "constructor_hole" {
                    hole()
                } else {
                    leaf(1)
                },
            );
            let right = nested(
                depth,
                match mode {
                    "different" | "constructor_different" => leaf(2),
                    "variable" => CanonicalTy::variable(0),
                    _ => leaf(1),
                },
            );
            ALLOCATIONS.with(|n| n.set(Some(0)));
            let start = Instant::now();
            for _ in 0..100 {
                let mut u = CanonicalUnifier::default();
                let result = if mode.starts_with("constructor") {
                    u.unify_constructor_identity(&left, &right)
                } else {
                    u.unify(&left, &right)
                };
                assert_eq!(std::hint::black_box(result), !mode.ends_with("different"));
            }
            let elapsed = start.elapsed();
            let allocations = ALLOCATIONS.with(|n| n.replace(None).unwrap());
            eprintln!(
                "{mode} depth={depth}: allocations/iteration={}, time/iteration={:?}",
                allocations / 100,
                elapsed / 100
            );
            results.push((mode, depth, allocations / 100));
        }
    }
    for (mode, depth, allocations) in results {
        assert!(
            allocations <= 4,
            "{mode} depth={depth}: {allocations} allocations exceed constant bound"
        );
    }
}

#[test]
fn canonical_unifier_overlap_preserves_variable_namespaces() {
    let key = |target| CanonicalTraitImplPatternKey {
        trait_ref: CanonicalTraitRef {
            trait_id: 0,
            arguments: vec![],
        },
        target,
    };
    let generic = key(nested(4, CanonicalTy::variable(0)));
    let concrete = key(nested(4, leaf(1)));
    let different = key(nested(4, leaf(2)));
    assert!(Checker::canonical_patterns_overlap(&generic, &concrete));
    assert!(Checker::canonical_patterns_overlap(&concrete, &generic));
    assert!(!Checker::canonical_patterns_overlap(&concrete, &different));
    assert!(!Checker::canonical_patterns_overlap(&different, &concrete));
    assert!(Checker::canonical_patterns_overlap(
        &key(CanonicalTy::variable(0)),
        &generic
    ));
}

#[test]
fn canonical_applicability_rejects_callable_hole_input() {
    let mut unifier = CanonicalUnifier::default();
    assert!(!unifier.unify(&callable(leaf(1), leaf(2)), &callable(hole(), leaf(2))));
    assert!(!unifier.unify(&callable(hole(), leaf(2)), &callable(leaf(1), leaf(2))));
    assert!(unifier.unify(&callable(hole(), leaf(2)), &callable(hole(), leaf(2))));
}

#[test]
fn canonical_unifier_shared_callable_inputs_preserve_hole_identity() {
    let mut u = CanonicalUnifier::default();
    let left = CanonicalTy::new(CanonicalTypeHead::Function, vec![hole(), leaf(1), leaf(2)]);
    let right = CanonicalTy::new(
        CanonicalTypeHead::Function,
        vec![CanonicalTy::variable(0), CanonicalTy::variable(0), leaf(2)],
    );
    // The first input binds variable 0 to Hole. The second input must reject
    // a concrete type versus that same Hole identity.
    assert!(!u.unify(&left, &right));
}
