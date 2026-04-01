pub mod bytecode;
pub mod codegen;
pub mod error;
pub mod opcode;
pub mod registry;

pub use codegen::{
    codegen, BindingInfo, ChunkMeta, ForgeCheckpoint, ForgeSession, ReplTypeKind, TypeDefDisplay,
};

#[cfg(test)]
mod tests {
    use super::{codegen, ForgeSession};
    use crate::opcode::Opcode;
    use crate::registry::TypeKind;

    const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/builtin.srt");

    fn typed_with_builtin_prelude(source: &str) -> Vec<scar::typed::TypedNode> {
        let mut ast = spire::parse(BUILTIN_PRELUDE_SOURCE).expect("builtin prelude should parse");
        let mut user_ast = spire::parse(source).expect("source should parse");
        ast.append(&mut user_ast);
        let resolved = sigil::resolve(ast).expect("source should resolve");
        scar::typecheck(resolved).expect("source should typecheck")
    }

    fn codegen_source(source: &str) -> sindr::ir::Bytecode {
        let typed = typed_with_builtin_prelude(source);
        codegen(typed).expect("codegen should succeed")
    }

    fn codegen_source_with_origin(source: &str, file_name: &str) -> sindr::ir::Bytecode {
        let mut ast = spire::parse_with_source(BUILTIN_PRELUDE_SOURCE, "builtin.srt")
            .expect("builtin prelude should parse");
        let mut user_ast =
            spire::parse_with_source(source, file_name).expect("source should parse");
        ast.append(&mut user_ast);
        let resolved = sigil::resolve(ast).expect("source should resolve");
        let typed = scar::typecheck(resolved).expect("source should typecheck");
        codegen(typed).expect("codegen should succeed")
    }

    #[test]
    fn does_not_emit_deprecated_frame_opcodes() {
        let bytecode = codegen_source(
            r#"x = 1
if(True, print("ok"), print("ng"))
print(to_string(x + 2))"#,
        );

        assert!(!bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::MakeFrame(_) | Opcode::PopFrame)));
    }

    #[test]
    fn function_table_preserves_fun_idx_index_invariant() {
        let bytecode = codegen_source(
            r#"def add(x: Int, y: Int) -> Int { x + y }
deferror Oops {
  "oops"
}
print(to_string(add(1, 2)))"#,
        );

        assert_eq!(bytecode.functions.len(), 2);
        for (idx, entry) in bytecode.functions.iter().enumerate() {
            assert_eq!(entry.fun_idx as usize, idx);
        }
    }

    #[test]
    fn field_access_emits_getfield_with_resolved_index() {
        let bytecode = codegen_source(
            r#"defstruct User {
  name: String,
  age: Int,
}

user: User = User { name: "alice", age: 30 }
print(to_string(user.age))"#,
        );

        assert!(bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::GetField(1))));
    }

    #[test]
    fn type_registry_reserves_result_tags_and_starts_user_tags_from_two() {
        let bytecode = codegen_source(
            r#"defstruct User {
  name: String,
  age: Int,
}

defrecord Point(x: Float, y: Float)
print("ok")"#,
        );

        assert_eq!(bytecode.type_registry.entries.len(), 2);
        assert_eq!(bytecode.type_registry.entries[0].tag, 2);
        assert_eq!(bytecode.type_registry.entries[0].name, "User");
        assert_eq!(bytecode.type_registry.entries[0].kind, TypeKind::Struct);
        assert_eq!(bytecode.type_registry.entries[1].tag, 3);
        assert_eq!(bytecode.type_registry.entries[1].name, "Point");
        assert_eq!(bytecode.type_registry.entries[1].kind, TypeKind::Record);
    }

    #[test]
    fn codegen_emits_source_map_for_programs() {
        let bytecode = codegen_source(r#"print("ok")"#);
        let source_map = bytecode.source_map.expect("source map should be present");
        assert!(!source_map.entries.is_empty());
        assert_eq!(source_map.entries[0].opcode_index, 0);
    }

    #[test]
    fn codegen_chunk_emits_source_map() {
        let typed = typed_with_builtin_prelude(r#"print("ok")"#);
        let mut session = ForgeSession::new();
        let (chunk, _) = session
            .codegen_chunk(typed)
            .expect("chunk codegen should succeed");
        let source_map = chunk.source_map.expect("chunk source map should be present");
        assert!(!source_map.entries.is_empty());
        assert_eq!(source_map.entries[0].opcode_index, 0);
    }

    #[test]
    fn codegen_preserves_source_origin_in_source_map() {
        let bytecode = codegen_source_with_origin(r#"print("ok")"#, "main.srt");
        let source_map = bytecode.source_map.expect("source map should be present");
        assert!(
            source_map
                .entries
                .iter()
                .any(|entry| entry.source_name.as_deref() == Some("main.srt"))
        );
        assert!(
            source_map
                .entries
                .iter()
                .any(|entry| entry.source_name.as_deref() == Some("builtin.srt"))
        );
    }
}
