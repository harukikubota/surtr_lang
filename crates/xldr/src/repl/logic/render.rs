use eldr::builtin::inspect_value;
use eldr::value::Value;
use forge::{ChunkMeta, ReplCallableDisplay};

fn rendered_binding_type(binding_ty: &str, value: &Value) -> String {
    match value {
        Value::Pid(pid) => format!("PID<{}>", crate::surface_rendered_name(&pid.process_name)),
        _ => crate::surface_rendered_name(binding_ty),
    }
}

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
                    if let Some(lens_info) = &b.lens_info {
                        return Some(format!(
                            "{}: {} = {}",
                            b.name,
                            crate::surface_rendered_name(&b.ty),
                            crate::surface_rendered_name(&lens_info.full_path)
                        ));
                    }

                    let val = vm.get_local(b.slot_id)?;
                    let rendered_ty = rendered_binding_type(&b.ty, &val);
                    let displayed = b.callable_display.as_ref().map_or_else(
                        || inspect_value(vm, &val),
                        |display| match display {
                            ReplCallableDisplay::FnCapture { module, name, sig } => format!(
                                "FnCapture(module: {}, name: {}, sig: {})",
                                crate::surface_rendered_name(module),
                                name,
                                crate::surface_rendered_name(sig)
                            ),
                            ReplCallableDisplay::Closure { sig } => {
                                format!("Closure{}", crate::surface_rendered_name(sig))
                            }
                        },
                    );

                    Some(format!("{}: {} = {}", b.name, rendered_ty, displayed))
                })
                .collect();
        }
        if let Some(lens_info) = &meta.result_lens_info {
            return vec![format!("{} = {}", lens_info.ty, lens_info.full_path)];
        }
        if !meta.type_defs.is_empty() {
            return meta.type_defs.iter().map(|t| t.name.clone()).collect();
        }
    }

    vec![]
}
