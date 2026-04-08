use eldr::builtin::inspect_value;
use eldr::value::Value;
use forge::ChunkMeta;

/// Render display lines for one evaluated result.
///
/// Returns binding lines (`name: Type = value`), type-def names, or the
/// inspected value string. Returns an empty `Vec` when there is nothing to show.
///
/// Pure function — no I/O.
pub fn format_result_lines(
    vm: &eldr::VM,
    value: Option<&Value>,
    meta: Option<&ChunkMeta>,
) -> Vec<String> {
    if let Some(v) = value {
        if !matches!(v, Value::Unit) {
            return vec![inspect_value(vm, v)];
        }
    }

    if let Some(meta) = meta {
        if !meta.bindings.is_empty() {
            return meta
                .bindings
                .iter()
                .filter_map(|b| {
                    let val = vm.get_local(b.slot_id)?;
                    Some(format!(
                        "{}: {} = {}",
                        b.name,
                        b.ty,
                        inspect_value(vm, &val)
                    ))
                })
                .collect();
        }
        if !meta.type_defs.is_empty() {
            return meta.type_defs.iter().map(|t| t.name.clone()).collect();
        }
    }

    vec![]
}
