use eldr::builtin::inspect_value;
use eldr::value::Value;
use forge::ChunkMeta;

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
) -> Result<Vec<String>, eldr::RuntimeError> {
    if let Some(v) = value {
        if !matches!(v, Value::Unit) {
            return Ok(vec![inspect_value(vm, v)?]);
        }
    }

    if let Some(meta) = meta {
        if !meta.bindings.is_empty() {
            return meta
                .bindings
                .iter()
                .filter_map(|b| {
                    if let Some(facet_info) = &b.facet_info {
                        return Some(Ok(format!(
                            "{}: {} = {}",
                            b.name,
                            crate::surface_rendered_name(&b.ty),
                            crate::surface_rendered_name(&facet_info.full_path)
                        )));
                    }

                    let val = vm.get_local(b.slot_id)?;
                    let rendered_ty = rendered_binding_type(&b.ty, &val);
                    Some(
                        inspect_value(vm, &val).map(|displayed| {
                            format!("{}: {} = {}", b.name, rendered_ty, displayed)
                        }),
                    )
                })
                .collect();
        }
        if let Some(facet_info) = &meta.result_facet_info {
            return Ok(vec![format!(
                "{} = {}",
                facet_info.ty, facet_info.full_path
            )]);
        }
        if !meta.type_defs.is_empty() {
            return Ok(meta.type_defs.iter().map(|t| t.name.clone()).collect());
        }
    }

    Ok(vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_rendering_propagates_invalid_value_errors() {
        let vm = eldr::VM::new(sindr::ir::Bytecode::default());
        for value in [
            Value::Tagged {
                tag: 0,
                fields: vec![],
            },
            Value::Tuple(vec![Value::Tagged {
                tag: 9999,
                fields: vec![],
            }]),
        ] {
            assert!(format_result_lines(&vm, Some(&value), None).is_err());
        }
        assert_eq!(
            format_result_lines(&vm, Some(&Value::Int(3.into())), None).unwrap(),
            vec!["3"]
        );
    }
}
