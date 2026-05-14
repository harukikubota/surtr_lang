use crate::names::{builtin_type_name, TypeName};

/// Built-in function metadata shared across Sigil / Scar / Forge / Eldr.
///
/// Surtr source files under `lib/*.srt` may declare these builtins with
/// `@builtin`, but the canonical definition order and ids live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinMeta {
    pub name: &'static str,
    pub arity: u8,
    /// Type signature string used by type checker bootstrap and validation.
    pub sig_str: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTypeMeta {
    /// Canonical builtin type head that std-module `@builtin type`
    /// declarations must match exactly.
    pub name: &'static str,
    pub params: &'static [&'static str],
}

/// Builtin unique ids start after the first two scope-reserved ids.
pub const BUILTIN_UID_BASE: u32 = 2;

pub const BUILTIN_METAS: &[BuiltinMeta] = &[
    BuiltinMeta {
        name: "print",
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "to_string",
        arity: 1,
        sig_str: "($A) -> String",
    },
    BuiltinMeta {
        name: "inspect",
        arity: 1,
        sig_str: "($A) -> String",
    },
    BuiltinMeta {
        name: "safe_div",
        arity: 2,
        sig_str: "($A, $A) -> Result<$A, ZeroDivisionError>",
    },
    BuiltinMeta {
        name: "safe_mod",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, ZeroDivisionError>",
    },
    BuiltinMeta {
        name: "eprint",
        arity: 1,
        sig_str: "(Error) -> Unit",
    },
    BuiltinMeta {
        name: "set_exit_code",
        arity: 1,
        sig_str: "(Int) -> Unit",
    },
    BuiltinMeta {
        name: "shl",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeShiftCount>",
    },
    BuiltinMeta {
        name: "shr",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeShiftCount>",
    },
    BuiltinMeta {
        name: "len",
        arity: 1,
        sig_str: "(List<$A>) -> Int",
    },
    BuiltinMeta {
        name: "gen_make",
        arity: 2,
        sig_str: "(Int, List<$Item>) -> Generator<$State, $Item>",
    },
    BuiltinMeta {
        name: "gen_idx",
        arity: 1,
        sig_str: "(Generator<$State, $Item>) -> Int",
    },
    BuiltinMeta {
        name: "gen_items",
        arity: 1,
        sig_str: "(Generator<$State, $Item>) -> List<$Item>",
    },
    BuiltinMeta {
        name: "bit_and",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "bit_or",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "bit_xor",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "bit_not",
        arity: 1,
        sig_str: "(Int) -> Int",
    },
    BuiltinMeta {
        name: "test_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Boolean, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "set_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "clear_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "toggle_bit",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, NegativeBitIndex>",
    },
    BuiltinMeta {
        name: "codepoints",
        arity: 2,
        sig_str: "(String, StringEncoding) -> Result<List<Int>, InvalidStringEncoding>",
    },
    BuiltinMeta {
        name: "from_codepoints",
        arity: 2,
        sig_str: "(List<Int>, StringEncoding) -> Result<String, InvalidStringEncoding>",
    },
    BuiltinMeta {
        name: "map_err",
        arity: 2,
        sig_str: "(Result<$T>, Lazy<Error>) -> Result<$T>",
    },
    BuiltinMeta {
        name: "cause",
        arity: 2,
        sig_str: "(Result<$T>, Lazy<Error>) -> Result<$T>",
    },
    BuiltinMeta {
        name: "chain",
        arity: 2,
        sig_str: "(Result<$T>, Result<()>) -> Result<$T>",
    },
    BuiltinMeta {
        name: "__recover_kind",
        arity: 3,
        sig_str: "(Result<$T>, String, (Error -> Result<$T>)) -> Result<$T>",
    },
    BuiltinMeta {
        name: "__test_push",
        arity: 2,
        sig_str: "(String, String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_pop",
        arity: 0,
        sig_str: "() -> Unit",
    },
    BuiltinMeta {
        name: "__test_pass",
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_fail",
        arity: 2,
        sig_str: "(String, String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_fail_error",
        arity: 2,
        sig_str: "(String, Error) -> Unit",
    },
    BuiltinMeta {
        name: "__test_fail_current",
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "group_count",
        arity: 1,
        sig_str: "(List<$A>) -> List<($A, Int)>",
    },
    BuiltinMeta {
        name: "zip",
        arity: 2,
        sig_str: "(List<$A>, List<$B>) -> List<($A, $B)>",
    },
    BuiltinMeta {
        name: "empty_map",
        arity: 0,
        sig_str: "() -> HashMap<$V>",
    },
    BuiltinMeta {
        name: "map_from_entries",
        arity: 1,
        sig_str: "(List<(String, $V)>) -> HashMap<$V>",
    },
    BuiltinMeta {
        name: "map_len",
        arity: 1,
        sig_str: "(HashMap<$V>) -> Int",
    },
    BuiltinMeta {
        name: "map_contains_key",
        arity: 2,
        sig_str: "(HashMap<$V>, String) -> Boolean",
    },
    BuiltinMeta {
        name: "map_get",
        arity: 2,
        sig_str: "(HashMap<$V>, String) -> Result<$V, NoneError>",
    },
    BuiltinMeta {
        name: "map_insert",
        arity: 3,
        sig_str: "(HashMap<$V>, String, $V) -> HashMap<$V>",
    },
    BuiltinMeta {
        name: "map_remove",
        arity: 2,
        sig_str: "(HashMap<$V>, String) -> HashMap<$V>",
    },
    BuiltinMeta {
        name: "map_keys",
        arity: 1,
        sig_str: "(HashMap<$V>) -> List<String>",
    },
    BuiltinMeta {
        name: "map_values_list",
        arity: 1,
        sig_str: "(HashMap<$V>) -> List<$V>",
    },
    BuiltinMeta {
        name: "view",
        arity: 2,
        sig_str: "(Facet<$S, $A>, $S) -> Result<$A>",
    },
    BuiltinMeta {
        name: "preview",
        arity: 2,
        sig_str: "(Facet<$S, $A>, $S) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__facet_chain",
        arity: 2,
        sig_str: "(Facet<$S, $A>, Facet<$A, $B>) -> Facet<$S, $B>",
    },
    BuiltinMeta {
        name: "__facet_put",
        arity: 3,
        sig_str: "(Facet<$S, $A>, $S, $A) -> $S",
    },
    BuiltinMeta {
        name: "set",
        arity: 3,
        sig_str: "(Facet<$S, $A>, $S, $A) -> Result<$S>",
    },
    BuiltinMeta {
        name: "over",
        arity: 3,
        sig_str: "(Facet<$S, $A>, $S, ($A -> Result<$A>)) -> Result<$S>",
    },
    BuiltinMeta {
        name: "over_result",
        arity: 3,
        sig_str: "(Facet<$S, Result<$A>>, $S, (Result<$A> -> Result<Result<$A>>)) -> Result<$S>",
    },
    BuiltinMeta {
        name: "case_set",
        arity: 3,
        sig_str: "(Facet<$S, $A>, $S, $A) -> Result<$S>",
    },
    BuiltinMeta {
        name: "case_over",
        arity: 3,
        sig_str: "(Facet<$S, $A>, $S, ($A -> Result<$A>)) -> Result<$S>",
    },
    BuiltinMeta {
        name: "__facet_list_get",
        arity: 2,
        sig_str: "(List<$A>, Int) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__facet_list_set",
        arity: 3,
        sig_str: "(List<$A>, Int, $A) -> Result<List<$A>>",
    },
    BuiltinMeta {
        name: "__facet_map_get",
        arity: 2,
        sig_str: "(HashMap<$A>, String) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__facet_map_set_existing",
        arity: 3,
        sig_str: "(HashMap<$A>, String, $A) -> Result<HashMap<$A>>",
    },
    BuiltinMeta {
        name: "__test_capture_stdout",
        arity: 0,
        sig_str: "() -> List<String>",
    },
    BuiltinMeta {
        name: "__test_capture_stderr",
        arity: 0,
        sig_str: "() -> List<String>",
    },
    BuiltinMeta {
        name: "__test_push_stdin",
        arity: 1,
        sig_str: "(String) -> Unit",
    },
    BuiltinMeta {
        name: "__test_begin_it",
        arity: 0,
        sig_str: "() -> Unit",
    },
    BuiltinMeta {
        name: "compile",
        arity: 1,
        sig_str: "(String) -> Result<Regex, RegexCompileError>",
    },
    BuiltinMeta {
        name: "is_match",
        arity: 2,
        sig_str: "(Regex, String) -> Boolean",
    },
    BuiltinMeta {
        name: "captures",
        arity: 2,
        sig_str: "(Regex, String) -> Result<RegexCaptures, NoneError>",
    },
    BuiltinMeta {
        name: "whole",
        arity: 1,
        sig_str: "(RegexCaptures) -> String",
    },
    BuiltinMeta {
        name: "capture_count",
        arity: 1,
        sig_str: "(RegexCaptures) -> Int",
    },
    BuiltinMeta {
        name: "get",
        arity: 2,
        sig_str: "(RegexCaptures, Int) -> Result<String, NoneError>",
    },
    BuiltinMeta {
        name: "get_name",
        arity: 2,
        sig_str: "(RegexCaptures, String) -> Result<String, NoneError>",
    },
    BuiltinMeta {
        name: "find",
        arity: 2,
        sig_str: "(Regex, String) -> Result<RegexMatch, NoneError>",
    },
    BuiltinMeta {
        name: "find_all",
        arity: 2,
        sig_str: "(Regex, String) -> List<RegexMatch>",
    },
    BuiltinMeta {
        name: "split",
        arity: 2,
        sig_str: "(Regex, String) -> List<String>",
    },
    BuiltinMeta {
        name: "__regex_replace",
        arity: 3,
        sig_str: "(Regex, String, String) -> String",
    },
    BuiltinMeta {
        name: "replace_all",
        arity: 3,
        sig_str: "(Regex, String, String) -> String",
    },
    BuiltinMeta {
        name: "escape",
        arity: 1,
        sig_str: "(String) -> String",
    },
    BuiltinMeta {
        name: "group_names",
        arity: 1,
        sig_str: "(Regex) -> List<String>",
    },
    BuiltinMeta {
        name: "text",
        arity: 1,
        sig_str: "(RegexMatch) -> String",
    },
    BuiltinMeta {
        name: "start",
        arity: 1,
        sig_str: "(RegexMatch) -> Int",
    },
    BuiltinMeta {
        name: "end",
        arity: 1,
        sig_str: "(RegexMatch) -> Int",
    },
    BuiltinMeta {
        name: "project_args",
        arity: 0,
        sig_str: "() -> List<String>",
    },
    BuiltinMeta {
        name: "io_get",
        arity: 1,
        sig_str: "(String) -> Result<String, InputError>",
    },
    BuiltinMeta {
        name: "io_get_line",
        arity: 1,
        sig_str: "(String) -> Result<String, InputError>",
    },
    BuiltinMeta {
        name: "file_read",
        arity: 1,
        sig_str: "(String) -> Result<String>",
    },
    BuiltinMeta {
        name: "file_write",
        arity: 2,
        sig_str: "(String, String) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "file_append",
        arity: 2,
        sig_str: "(String, String) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "file_exists",
        arity: 1,
        sig_str: "(String) -> Boolean",
    },
    BuiltinMeta {
        name: "file_delete",
        arity: 1,
        sig_str: "(String) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "file_with_open",
        arity: 3,
        sig_str: "(String, FileMode, (FileHandle -> Result<$A>)) -> Result<$A>",
    },
    BuiltinMeta {
        name: "file_read_chunk",
        arity: 2,
        sig_str: "(FileHandle, Int) -> Result<String>",
    },
    BuiltinMeta {
        name: "file_write_chunk",
        arity: 2,
        sig_str: "(FileHandle, String) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "file_flush",
        arity: 1,
        sig_str: "(FileHandle) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "filesystem_path",
        arity: 1,
        sig_str: "(String) -> Result<FilePath, Error>",
    },
    BuiltinMeta {
        name: "filesystem_join",
        arity: 2,
        sig_str: "(FilePath, String) -> Result<FilePath, Error>",
    },
    BuiltinMeta {
        name: "filesystem_parent",
        arity: 1,
        sig_str: "(FilePath) -> Result<FilePath, Error>",
    },
    BuiltinMeta {
        name: "filesystem_name",
        arity: 1,
        sig_str: "(FilePath) -> Result<String, Error>",
    },
    BuiltinMeta {
        name: "filesystem_extension",
        arity: 1,
        sig_str: "(FilePath) -> Option<String>",
    },
    BuiltinMeta {
        name: "filesystem_exists",
        arity: 1,
        sig_str: "(FilePath) -> Result<Boolean, Error>",
    },
    BuiltinMeta {
        name: "filesystem_stat",
        arity: 1,
        sig_str: "(FilePath) -> Result<FileSystemEntry, Error>",
    },
    BuiltinMeta {
        name: "filesystem_ls",
        arity: 1,
        sig_str: "(FilePath) -> Result<FileSystemSnapshot, Error>",
    },
    BuiltinMeta {
        name: "filesystem_tree_depth",
        arity: 2,
        sig_str: "(FilePath, Int) -> Result<FileSystemSnapshot, Error>",
    },
    BuiltinMeta {
        name: "filesystem_mkdir",
        arity: 1,
        sig_str: "(FilePath) -> Result<Unit, Error>",
    },
    BuiltinMeta {
        name: "filesystem_mkdir_all",
        arity: 1,
        sig_str: "(FilePath) -> Result<Unit, Error>",
    },
    BuiltinMeta {
        name: "filesystem_rm",
        arity: 1,
        sig_str: "(FilePath) -> Result<Unit, Error>",
    },
    BuiltinMeta {
        name: "filesystem_mv",
        arity: 2,
        sig_str: "(FilePath, FilePath) -> Result<Unit, Error>",
    },
    BuiltinMeta {
        name: "filesystem_cp",
        arity: 2,
        sig_str: "(FilePath, FilePath) -> Result<Unit, Error>",
    },
    BuiltinMeta {
        name: "shell_pwd",
        arity: 0,
        sig_str: "() -> Result<FilePath, Error>",
    },
    BuiltinMeta {
        name: "shell_cd",
        arity: 1,
        sig_str: "(FilePath) -> Result<Unit, Error>",
    },
    BuiltinMeta {
        name: "shell_exec",
        arity: 2,
        sig_str: "(String, List<String>) -> Result<CommandResult, Error>",
    },
    BuiltinMeta {
        name: "seed",
        arity: 1,
        sig_str: "(Int) -> RandomGenerator",
    },
    BuiltinMeta {
        name: "int_until",
        arity: 1,
        sig_str: "(Int) -> Result<Int, InvalidRandomRange>",
    },
    BuiltinMeta {
        name: "int_range",
        arity: 2,
        sig_str: "(Int, Int) -> Result<Int, InvalidRandomRange>",
    },
    BuiltinMeta {
        name: "next_int_until",
        arity: 2,
        sig_str: "(RandomGenerator, Int) -> Result<(Int, RandomGenerator), InvalidRandomRange>",
    },
    BuiltinMeta {
        name: "next_int_range",
        arity: 3,
        sig_str:
            "(RandomGenerator, Int, Int) -> Result<(Int, RandomGenerator), InvalidRandomRange>",
    },
    BuiltinMeta {
        name: "kind",
        arity: 1,
        sig_str: "(Error) -> String",
    },
    BuiltinMeta {
        name: "message",
        arity: 1,
        sig_str: "(Error) -> String",
    },
    BuiltinMeta {
        name: "format",
        arity: 1,
        sig_str: "(Error) -> String",
    },
    BuiltinMeta {
        name: "__process_pid",
        arity: 2,
        sig_str: "($Owner, (-> Result<$State>)) -> PID<$Process>",
    },
    BuiltinMeta {
        name: "__process_spawn",
        arity: 2,
        sig_str: "($Owner, (-> Result<$State>)) -> Result<PID<$Process>>",
    },
    BuiltinMeta {
        name: "__dynamic_supervisor_spawn",
        arity: 1,
        sig_str: "((-> Result<$State>)) -> Result<PID<$Process>>",
    },
    BuiltinMeta {
        name: "__dynamic_supervisor_adopt",
        arity: 1,
        sig_str: "(PID<$Process>) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__dynamic_supervisor_status",
        arity: 0,
        sig_str: "() -> Result<SupervisorStatus>",
    },
    BuiltinMeta {
        name: "__supervisor_spawn",
        arity: 2,
        sig_str: "($Supervisor, (-> Result<$State>)) -> Result<PID<$Process>>",
    },
    BuiltinMeta {
        name: "__supervisor_adopt",
        arity: 2,
        sig_str: "($Supervisor, PID<$Process>) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__supervisor_status",
        arity: 1,
        sig_str: "($Supervisor) -> Result<SupervisorStatus>",
    },
    BuiltinMeta {
        name: "__supervisor_workers",
        arity: 3,
        sig_str: "($Supervisor, (-> Result<$State>), WorkerStrategy) -> Result<Workers<$Process>>",
    },
    BuiltinMeta {
        name: "__process_state",
        arity: 1,
        sig_str: "(PID<$Process>) -> Result<$State>",
    },
    BuiltinMeta {
        name: "__process_store",
        arity: 2,
        sig_str: "(PID<$Process>, $State) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__genserver_call_reply",
        arity: 3,
        sig_str: "(PID<$Process>, $State, $Reply) -> Result<$Reply>",
    },
    BuiltinMeta {
        name: "__genserver_call_reply_later",
        arity: 3,
        sig_str: "(PID<$Process>, $State, (-> Result<$Reply>)) -> Result<$Reply>",
    },
    BuiltinMeta {
        name: "__genserver_call_stop_normal",
        arity: 2,
        sig_str: "(PID<$Process>, $Reply) -> Result<$Reply>",
    },
    BuiltinMeta {
        name: "__genserver_call_stop_error",
        arity: 2,
        sig_str: "(PID<$Process>, Error) -> Result<$Reply>",
    },
    BuiltinMeta {
        name: "__genserver_cast_next",
        arity: 2,
        sig_str: "(PID<$Process>, $State) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__genserver_cast_stop_normal",
        arity: 1,
        sig_str: "(PID<$Process>) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__genserver_cast_stop_error",
        arity: 2,
        sig_str: "(PID<$Process>, Error) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__process_self",
        arity: 0,
        sig_str: "() -> PID<$Process>",
    },
    BuiltinMeta {
        name: "__process_context_handler",
        arity: 2,
        sig_str: "($Owner, String) -> PID<$Handler>",
    },
    BuiltinMeta {
        name: "__out_handler_write",
        arity: 2,
        sig_str: "(PID<OutHandler>, String) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__process_sleep",
        arity: 1,
        sig_str: "(Duration) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "Pending",
        arity: 0,
        sig_str: "() -> ProcessInit<$T>",
    },
    BuiltinMeta {
        name: "PendingAfter",
        arity: 1,
        sig_str: "(Duration) -> ProcessInit<$T>",
    },
    BuiltinMeta {
        name: "Ready",
        arity: 1,
        sig_str: "($T) -> ProcessInit<$T>",
    },
    BuiltinMeta {
        name: "__task_call",
        arity: 1,
        sig_str: "((-> Result<$A>)) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__task_async",
        arity: 1,
        sig_str: "((-> Result<$A>)) -> TaskHandle<$A>",
    },
    BuiltinMeta {
        name: "__task_await",
        arity: 1,
        sig_str: "(TaskHandle<$A>) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__task_launch",
        arity: 1,
        sig_str: "((-> Result<Unit>)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__task_cast",
        arity: 1,
        sig_str: "((-> Unit)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__task_call_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Result<$A>)) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__task_async_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Result<$A>)) -> TaskHandle<$A>",
    },
    BuiltinMeta {
        name: "__task_await_timeout",
        arity: 2,
        sig_str: "(Duration, TaskHandle<$A>) -> Result<$A>",
    },
    BuiltinMeta {
        name: "__task_launch_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Result<Unit>)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__task_cast_timeout",
        arity: 2,
        sig_str: "(Duration, (-> Unit)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__workers_submit",
        arity: 2,
        sig_str: "(Workers<$Worker>, (PID<$Worker> -> Result<Unit>)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__workers_submit_timeout",
        arity: 3,
        sig_str: "(Duration, Workers<$Worker>, (PID<$Worker> -> Result<Unit>)) -> Result<Unit>",
    },
    BuiltinMeta {
        name: "__workers_broadcast",
        arity: 2,
        sig_str: "(Workers<$Worker>, (PID<$Worker> -> Result<$A>)) -> List<Result<$A>>",
    },
    BuiltinMeta {
        name: "__workers_broadcast_timeout",
        arity: 3,
        sig_str: "(Duration, Workers<$Worker>, (PID<$Worker> -> Result<$A>)) -> List<Result<$A>>",
    },
    BuiltinMeta {
        name: "__workers_reserve",
        arity: 1,
        sig_str: "(Workers<$Worker>) -> Result<WorkerLease<$Worker>>",
    },
    BuiltinMeta {
        name: "__workers_size",
        arity: 1,
        sig_str: "(Workers<$Worker>) -> Int",
    },
    BuiltinMeta {
        name: "__operator_int_add",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "__operator_int_sub",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "__operator_int_mul",
        arity: 2,
        sig_str: "(Int, Int) -> Int",
    },
    BuiltinMeta {
        name: "__operator_float_add",
        arity: 2,
        sig_str: "(Float, Float) -> Float",
    },
    BuiltinMeta {
        name: "__operator_float_sub",
        arity: 2,
        sig_str: "(Float, Float) -> Float",
    },
    BuiltinMeta {
        name: "__operator_float_mul",
        arity: 2,
        sig_str: "(Float, Float) -> Float",
    },
    BuiltinMeta {
        name: "__operator_int_eq",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_neq",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_lt",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_lte",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_gt",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_int_gte",
        arity: 2,
        sig_str: "(Int, Int) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_eq",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_neq",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_lt",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_lte",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_gt",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_float_gte",
        arity: 2,
        sig_str: "(Float, Float) -> Boolean",
    },
    BuiltinMeta {
        name: "__compare_int",
        arity: 2,
        sig_str: "(Int, Int) -> Ordering",
    },
    BuiltinMeta {
        name: "__compare_float",
        arity: 2,
        sig_str: "(Float, Float) -> Ordering",
    },
    BuiltinMeta {
        name: "__ordering_is_lt",
        arity: 1,
        sig_str: "(Ordering) -> Boolean",
    },
    BuiltinMeta {
        name: "__ordering_is_lte",
        arity: 1,
        sig_str: "(Ordering) -> Boolean",
    },
    BuiltinMeta {
        name: "__ordering_is_gt",
        arity: 1,
        sig_str: "(Ordering) -> Boolean",
    },
    BuiltinMeta {
        name: "__ordering_is_gte",
        arity: 1,
        sig_str: "(Ordering) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_string_eq",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_string_neq",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_boolean_eq",
        arity: 2,
        sig_str: "(Boolean, Boolean) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_boolean_neq",
        arity: 2,
        sig_str: "(Boolean, Boolean) -> Boolean",
    },
    BuiltinMeta {
        name: "__operator_string_concat",
        arity: 2,
        sig_str: "(String, String) -> String",
    },
    BuiltinMeta {
        name: "json_parse",
        arity: 1,
        sig_str: "(String) -> Result<JsonValue, JsonParseError>",
    },
    BuiltinMeta {
        name: "json_stringify",
        arity: 1,
        sig_str: "(JsonValue) -> Result<String, JsonEncodeError>",
    },
    BuiltinMeta {
        name: "string_len",
        arity: 1,
        sig_str: "(String) -> Int",
    },
    BuiltinMeta {
        name: "string_contains",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
    },
    BuiltinMeta {
        name: "string_starts_with",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
    },
    BuiltinMeta {
        name: "string_ends_with",
        arity: 2,
        sig_str: "(String, String) -> Boolean",
    },
    BuiltinMeta {
        name: "string_split",
        arity: 2,
        sig_str: "(String, String) -> List<String>",
    },
    BuiltinMeta {
        name: "string_replace",
        arity: 3,
        sig_str: "(String, String, String) -> String",
    },
];

/// Canonical builtin type declarations accepted from standard definition sources.
///
/// These entries define the exact source-level heads the compiler accepts,
/// including generic parameter names such as `List<$A>` and `Result<$T>`.
pub const BUILTIN_TYPE_METAS: &[BuiltinTypeMeta] = &[
    BuiltinTypeMeta {
        name: TypeName::Int.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Float.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::String.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Boolean.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Unit.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Closure.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::MatchArms.as_str(),
        params: &["$Scrutinee", "$Result"],
    },
    BuiltinTypeMeta {
        name: TypeName::CondClauses.as_str(),
        params: &["$Result"],
    },
    BuiltinTypeMeta {
        name: TypeName::BulkUpdateEntries.as_str(),
        params: &["$State"],
    },
    BuiltinTypeMeta {
        name: TypeName::Error.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Regex.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::RegexCaptures.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::RegexMatch.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::RandomGenerator.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::FileHandle.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::List.as_str(),
        params: &["$A"],
    },
    BuiltinTypeMeta {
        name: TypeName::HashMap.as_str(),
        params: &["$V"],
    },
    BuiltinTypeMeta {
        name: TypeName::Generator.as_str(),
        params: &["$State", "$Item"],
    },
    BuiltinTypeMeta {
        name: TypeName::Result.as_str(),
        params: &["$T"],
    },
    BuiltinTypeMeta {
        name: TypeName::ProcessInit.as_str(),
        params: &["$T"],
    },
    BuiltinTypeMeta {
        name: TypeName::Lazy.as_str(),
        params: &["$T"],
    },
    BuiltinTypeMeta {
        name: TypeName::TypeRef.as_str(),
        params: &["$T"],
    },
    BuiltinTypeMeta {
        name: TypeName::Hole.as_str(),
        params: &[],
    },
    BuiltinTypeMeta {
        name: TypeName::Facet.as_str(),
        params: &["$S", "$A"],
    },
    BuiltinTypeMeta {
        name: TypeName::Workers.as_str(),
        params: &["$Worker"],
    },
    BuiltinTypeMeta {
        name: TypeName::WorkerLease.as_str(),
        params: &["$Worker"],
    },
    BuiltinTypeMeta {
        name: TypeName::TaskHandle.as_str(),
        params: &["$T"],
    },
];

pub fn builtin_meta_by_name(name: &str) -> Option<&'static BuiltinMeta> {
    BUILTIN_METAS.iter().find(|meta| meta.name == name)
}

pub fn builtin_runtime_name<'a>(declared_name: &'a str, qualified_name: Option<&str>) -> &'a str {
    let qualified_name = qualified_name.map(|name| name.strip_prefix("Global::").unwrap_or(name));
    match qualified_name {
        Some("IO::get") => "io_get",
        Some("IO::get_line") => "io_get_line",
        Some("File::read") => "file_read",
        Some("File::write") => "file_write",
        Some("File::append") => "file_append",
        Some("File::exists") => "file_exists",
        Some("File::delete") => "file_delete",
        Some("File::with_open") => "file_with_open",
        Some("File::read_chunk") => "file_read_chunk",
        Some("File::write_chunk") => "file_write_chunk",
        Some("File::flush") => "file_flush",
        Some("FS::path") => "filesystem_path",
        Some("FS::join") => "filesystem_join",
        Some("FS::parent") => "filesystem_parent",
        Some("FS::name") => "filesystem_name",
        Some("FS::extension") => "filesystem_extension",
        Some("FS::exists") => "filesystem_exists",
        Some("FS::stat") => "filesystem_stat",
        Some("FS::ls") => "filesystem_ls",
        Some("FS::tree_depth") => "filesystem_tree_depth",
        Some("FS::mkdir") => "filesystem_mkdir",
        Some("FS::mkdir_all") => "filesystem_mkdir_all",
        Some("FS::rm") => "filesystem_rm",
        Some("FS::mv") => "filesystem_mv",
        Some("FS::cp") => "filesystem_cp",
        Some("Shell::pwd") => "shell_pwd",
        Some("Shell::cd") => "shell_cd",
        Some("Shell::exec") => "shell_exec",
        Some("String::len") => "string_len",
        Some("String::contains") => "string_contains",
        Some("String::starts_with") => "string_starts_with",
        Some("String::ends_with") => "string_ends_with",
        Some("String::split") => "string_split",
        Some("String::replace") => "string_replace",
        Some("Json::parse") => "json_parse",
        Some("Json::stringify") => "json_stringify",
        Some("Facet::chain") => "__facet_chain",
        Some("Facet::put") => "__facet_put",
        Some("Process::self") => "__process_self",
        Some("Process::sleep") => "__process_sleep",
        Some("Agent::pid") => "__process_pid",
        Some("Agent::spawn") => "__process_spawn",
        Some("Agent::state") => "__process_state",
        Some("Agent::store") => "__process_store",
        Some("Agent::self") => "__process_self",
        Some("Agent::context_handler") => "__process_context_handler",
        Some("GenServer::pid") => "__process_pid",
        Some("GenServer::spawn") => "__process_spawn",
        Some("GenServer::state") => "__process_state",
        Some("GenServer::store") => "__process_store",
        Some("GenServer::self") => "__process_self",
        Some("GenServer::context_handler") => "__process_context_handler",
        Some("Supervisor::spawn") => "__supervisor_spawn",
        Some("Supervisor::adopt") => "__supervisor_adopt",
        Some("Supervisor::status") => "__supervisor_status",
        Some("Supervisor::workers") => "__supervisor_workers",
        Some("OutHandler::write") => "__out_handler_write",
        Some("DynamicSupervisor::spawn") => "__dynamic_supervisor_spawn",
        Some("DynamicSupervisor::adopt") => "__dynamic_supervisor_adopt",
        Some("DynamicSupervisor::status") => "__dynamic_supervisor_status",
        Some("Workers::submit") => "__workers_submit",
        Some("Workers::broadcast") => "__workers_broadcast",
        Some("Workers::reserve") => "__workers_reserve",
        Some("Workers::size") => "__workers_size",
        Some("Task::call") => "__task_call",
        Some("Task::async") => "__task_async",
        Some("Task::await") => "__task_await",
        Some("Task::launch") => "__task_launch",
        Some("Task::cast") => "__task_cast",
        Some("Regex::replace") => "__regex_replace",
        _ => declared_name,
    }
}

pub fn builtin_meta_for_decl(
    declared_name: &str,
    qualified_name: Option<&str>,
) -> Option<&'static BuiltinMeta> {
    builtin_meta_by_name(builtin_runtime_name(declared_name, qualified_name))
}

pub fn builtin_id_by_name(name: &str) -> Option<u16> {
    BUILTIN_METAS
        .iter()
        .position(|meta| meta.name == name)
        .and_then(|idx| (idx <= u16::MAX as usize).then_some(idx as u16))
}

pub fn builtin_meta_by_id(builtin_id: u16) -> Option<&'static BuiltinMeta> {
    BUILTIN_METAS.get(builtin_id as usize)
}

pub fn builtin_type_meta_by_name(name: &str) -> Option<&'static BuiltinTypeMeta> {
    BUILTIN_TYPE_METAS.iter().find(|meta| meta.name == name)
}

pub fn builtin_type_supports_inherent_impl(name: &str) -> bool {
    builtin_type_name(name).is_some_and(TypeName::supports_inherent_impl)
}

pub fn builtin_uid(builtin_id: u16) -> u32 {
    BUILTIN_UID_BASE + u32::from(builtin_id)
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_id_by_name, builtin_meta_by_id, builtin_meta_by_name, builtin_meta_for_decl,
        builtin_runtime_name, builtin_uid, BUILTIN_METAS,
    };

    #[test]
    fn builtin_ids_match_definition_order() {
        for (idx, meta) in BUILTIN_METAS.iter().enumerate() {
            let id = idx as u16;
            assert_eq!(builtin_id_by_name(meta.name), Some(id));
            assert_eq!(builtin_uid(id), 2 + idx as u32);
        }
    }

    #[test]
    fn builtin_lookup_returns_none_for_unknown_values() {
        assert!(builtin_meta_by_id(u16::MAX).is_none());
        assert!(builtin_meta_by_name("__missing__").is_none());
    }

    #[test]
    fn qualified_put_builtins_resolve_to_distinct_runtime_names() {
        assert_eq!(
            builtin_runtime_name("chain", Some("Facet::chain")),
            "__facet_chain"
        );
        assert_eq!(
            builtin_runtime_name("replace", Some("String::replace")),
            "string_replace"
        );
        assert_eq!(
            builtin_runtime_name("put", Some("Facet::put")),
            "__facet_put"
        );
        assert_eq!(
            builtin_runtime_name("replace", Some("Regex::replace")),
            "__regex_replace"
        );
        assert_eq!(
            builtin_meta_for_decl("put", Some("Facet::put"))
                .expect("facet put builtin metadata")
                .sig_str,
            "(Facet<$S, $A>, $S, $A) -> $S"
        );
        assert_eq!(
            builtin_meta_for_decl("replace", Some("Regex::replace"))
                .expect("regex replace builtin metadata")
                .sig_str,
            "(Regex, String, String) -> String"
        );
    }

    #[test]
    fn qualified_string_split_builtin_resolves_to_runtime_name() {
        assert_eq!(
            builtin_runtime_name("split", Some("String::split")),
            "string_split"
        );
        assert_eq!(
            builtin_meta_for_decl("split", Some("String::split"))
                .expect("string split builtin metadata")
                .sig_str,
            "(String, String) -> List<String>"
        );
    }

    #[test]
    fn qualified_json_builtins_resolve_to_runtime_names() {
        assert_eq!(
            builtin_runtime_name("parse", Some("Json::parse")),
            "json_parse"
        );
        assert_eq!(
            builtin_runtime_name("stringify", Some("Json::stringify")),
            "json_stringify"
        );
    }

    #[test]
    fn qualified_string_len_builtin_resolves_to_runtime_name() {
        assert_eq!(
            builtin_runtime_name("len", Some("String::len")),
            "string_len"
        );
        assert_eq!(
            builtin_meta_for_decl("len", Some("String::len"))
                .expect("string len builtin metadata")
                .sig_str,
            "(String) -> Int"
        );
    }

    #[test]
    fn qualified_string_predicate_builtins_resolve_to_runtime_names() {
        let cases = [
            ("contains", "String::contains", "string_contains"),
            ("starts_with", "String::starts_with", "string_starts_with"),
            ("ends_with", "String::ends_with", "string_ends_with"),
        ];

        for (declared, qualified, runtime) in cases {
            assert_eq!(builtin_runtime_name(declared, Some(qualified)), runtime);
            assert!(
                builtin_meta_for_decl(declared, Some(qualified)).is_some(),
                "{qualified} should have builtin metadata"
            );
        }
    }

    #[test]
    fn qualified_filesystem_and_shell_builtins_resolve_to_runtime_names() {
        let cases = [
            ("path", "FS::path", "filesystem_path"),
            ("join", "FS::join", "filesystem_join"),
            ("parent", "FS::parent", "filesystem_parent"),
            ("name", "FS::name", "filesystem_name"),
            ("extension", "FS::extension", "filesystem_extension"),
            ("exists", "FS::exists", "filesystem_exists"),
            ("stat", "FS::stat", "filesystem_stat"),
            ("ls", "FS::ls", "filesystem_ls"),
            ("tree_depth", "FS::tree_depth", "filesystem_tree_depth"),
            ("mkdir", "FS::mkdir", "filesystem_mkdir"),
            ("mkdir_all", "FS::mkdir_all", "filesystem_mkdir_all"),
            ("rm", "FS::rm", "filesystem_rm"),
            ("mv", "FS::mv", "filesystem_mv"),
            ("cp", "FS::cp", "filesystem_cp"),
            ("pwd", "Shell::pwd", "shell_pwd"),
            ("cd", "Shell::cd", "shell_cd"),
            ("exec", "Shell::exec", "shell_exec"),
        ];

        for (declared, qualified, runtime) in cases {
            assert_eq!(builtin_runtime_name(declared, Some(qualified)), runtime);
            assert!(
                builtin_meta_for_decl(declared, Some(qualified)).is_some(),
                "{qualified} should have builtin metadata"
            );
        }
    }

    #[test]
    fn supervisor_spawn_hidden_builtin_signature_matches_surface() {
        let meta = builtin_meta_by_name("__supervisor_spawn").expect("supervisor spawn builtin");
        assert_eq!(meta.arity, 2);
        assert_eq!(
            meta.sig_str,
            "($Supervisor, (-> Result<$State>)) -> Result<PID<$Process>>"
        );
    }

    #[test]
    fn supervisor_adopt_hidden_builtin_signature_matches_surface() {
        let meta = builtin_meta_by_name("__supervisor_adopt").expect("supervisor adopt builtin");
        assert_eq!(meta.arity, 2);
        assert_eq!(meta.sig_str, "($Supervisor, PID<$Process>) -> Result<Unit>");
    }

    #[test]
    fn supervisor_status_hidden_builtin_signature_matches_surface() {
        let meta = builtin_meta_by_name("__supervisor_status").expect("supervisor status builtin");
        assert_eq!(meta.arity, 1);
        assert_eq!(meta.sig_str, "($Supervisor) -> Result<SupervisorStatus>");
    }
}
