/* eslint-disable */
/* generated from Rust viewer schema */

export type ConstantView =
  | {
      idx: number;
      kind: "Int";
      value: string;
      [k: string]: unknown;
    }
  | {
      idx: number;
      kind: "Tag";
      value: number;
      [k: string]: unknown;
    }
  | {
      idx: number;
      kind: "Float";
      value: number;
      [k: string]: unknown;
    }
  | {
      idx: number;
      kind: "Str";
      value: string;
      [k: string]: unknown;
    }
  | {
      idx: number;
      kind: "Bool";
      value: boolean;
      [k: string]: unknown;
    }
  | {
      idx: number;
      kind: "Unit";
      [k: string]: unknown;
    };
export type OpcodeView =
  | {
      const_idx: number;
      kind: "LoadConst";
      [k: string]: unknown;
    }
  | {
      builtin: string;
      builtin_id: number;
      kind: "LoadBuiltinRef";
      [k: string]: unknown;
    }
  | {
      fun_idx: number;
      kind: "LoadFunctionRef";
      [k: string]: unknown;
    }
  | {
      kind: "LoadLocal";
      local_idx: number;
      [k: string]: unknown;
    }
  | {
      kind: "StoreLocal";
      local_idx: number;
      [k: string]: unknown;
    }
  | {
      kind: "AddInt";
      [k: string]: unknown;
    }
  | {
      kind: "SubInt";
      [k: string]: unknown;
    }
  | {
      kind: "MulInt";
      [k: string]: unknown;
    }
  | {
      kind: "BitNotInt";
      [k: string]: unknown;
    }
  | {
      kind: "BitAndInt";
      [k: string]: unknown;
    }
  | {
      kind: "BitOrInt";
      [k: string]: unknown;
    }
  | {
      kind: "BitXorInt";
      [k: string]: unknown;
    }
  | {
      kind: "AddFloat";
      [k: string]: unknown;
    }
  | {
      kind: "SubFloat";
      [k: string]: unknown;
    }
  | {
      kind: "MulFloat";
      [k: string]: unknown;
    }
  | {
      kind: "EqInt";
      [k: string]: unknown;
    }
  | {
      kind: "NeqInt";
      [k: string]: unknown;
    }
  | {
      kind: "LtInt";
      [k: string]: unknown;
    }
  | {
      kind: "GtInt";
      [k: string]: unknown;
    }
  | {
      kind: "LteInt";
      [k: string]: unknown;
    }
  | {
      kind: "GteInt";
      [k: string]: unknown;
    }
  | {
      kind: "EqFloat";
      [k: string]: unknown;
    }
  | {
      kind: "NeqFloat";
      [k: string]: unknown;
    }
  | {
      kind: "LtFloat";
      [k: string]: unknown;
    }
  | {
      kind: "GtFloat";
      [k: string]: unknown;
    }
  | {
      kind: "LteFloat";
      [k: string]: unknown;
    }
  | {
      kind: "GteFloat";
      [k: string]: unknown;
    }
  | {
      kind: "EqStr";
      [k: string]: unknown;
    }
  | {
      kind: "NeqStr";
      [k: string]: unknown;
    }
  | {
      kind: "EqBool";
      [k: string]: unknown;
    }
  | {
      kind: "NeqBool";
      [k: string]: unknown;
    }
  | {
      kind: "ConcatStr";
      [k: string]: unknown;
    }
  | {
      kind: "StringIsEmpty";
      [k: string]: unknown;
    }
  | {
      kind: "StringHead";
      [k: string]: unknown;
    }
  | {
      kind: "StringTail";
      [k: string]: unknown;
    }
  | {
      kind: "NegInt";
      [k: string]: unknown;
    }
  | {
      kind: "NegFloat";
      [k: string]: unknown;
    }
  | {
      kind: "NotBool";
      [k: string]: unknown;
    }
  | {
      kind: "ListNew";
      len: number;
      [k: string]: unknown;
    }
  | {
      kind: "ListEmpty";
      [k: string]: unknown;
    }
  | {
      kind: "ListNil";
      [k: string]: unknown;
    }
  | {
      kind: "ListCons";
      [k: string]: unknown;
    }
  | {
      kind: "ListIsEmpty";
      [k: string]: unknown;
    }
  | {
      kind: "ListHead";
      [k: string]: unknown;
    }
  | {
      kind: "ListTail";
      [k: string]: unknown;
    }
  | {
      kind: "ListFromItems";
      len: number;
      [k: string]: unknown;
    }
  | {
      field_count: number;
      kind: "StructNew";
      [k: string]: unknown;
    }
  | {
      field_index: number;
      kind: "GetField";
      [k: string]: unknown;
    }
  | {
      kind: "GetTag";
      [k: string]: unknown;
    }
  | {
      kind: "EqTag";
      [k: string]: unknown;
    }
  | {
      arity: number;
      builtin: string;
      builtin_id: number;
      kind: "CallBuiltin";
      span_end: number;
      span_start: number;
      [k: string]: unknown;
    }
  | {
      arity: number;
      fun_idx: number;
      kind: "Call";
      span_end: number;
      span_start: number;
      [k: string]: unknown;
    }
  | {
      capture_count: number;
      kind: "CaptureClosure";
      [k: string]: unknown;
    }
  | {
      arg_count: number;
      kind: "CapturePartial";
      [k: string]: unknown;
    }
  | {
      kind: "MakeError";
      template_id: number;
      [k: string]: unknown;
    }
  | {
      kind: "MakeErrorLiteral";
      kind_const_idx: number;
      message_const_idx: number;
      [k: string]: unknown;
    }
  | {
      arity: number;
      kind: "CallClosure";
      span_end: number;
      span_start: number;
      [k: string]: unknown;
    }
  | {
      kind: "Jump";
      target_pc: number;
      [k: string]: unknown;
    }
  | {
      kind: "JumpIfFalse";
      target_pc: number;
      [k: string]: unknown;
    }
  | {
      kind: "JumpIfTrue";
      target_pc: number;
      [k: string]: unknown;
    }
  | {
      kind: "Pop";
      [k: string]: unknown;
    }
  | {
      kind: "Return";
      [k: string]: unknown;
    }
  | {
      kind: "Halt";
      [k: string]: unknown;
    };

export interface ViewerFile {
  chunks: ChunkView[];
  constants: ConstantView[];
  errors: ErrorTemplateView[];
  format: string;
  functions: FunctionView[];
  header: ViewerHeader;
  opcodes: OpcodeRowView[];
  schema_version: number;
  sources: SourceFileView[];
  [k: string]: unknown;
}
export interface ChunkView {
  chunk_id: string;
  padded_size: number;
  payload_offset: number;
  size: number;
  tag: string;
  [k: string]: unknown;
}
export interface ErrorTemplateView {
  format: string;
  kind: string;
  num_params: number;
  source_ref?: SourceRefView | null;
  template_id: number;
  [k: string]: unknown;
}
export interface SourceRefView {
  column: number;
  line: number;
  source_id: string;
  span_end: number;
  span_start: number;
  [k: string]: unknown;
}
export interface FunctionView {
  arity: number;
  chunk_id: string;
  end_pc?: number | null;
  entry_pc: number;
  fun_idx: number;
  function_id: string;
  name?: string | null;
  num_locals: number;
  opcode_pcs: number[];
  source_ref?: SourceRefView | null;
  [k: string]: unknown;
}
export interface ViewerHeader {
  debug_level: number;
  magic: string;
  num_chunks: number;
  version: number;
  [k: string]: unknown;
}
export interface OpcodeRowView {
  function_id?: string | null;
  label?: string | null;
  op: OpcodeView;
  pc: number;
  source_ref?: SourceRefView | null;
  [k: string]: unknown;
}
export interface SourceFileView {
  content_hash?: string | null;
  name?: string | null;
  normalized_path?: string | null;
  source_id: string;
  text?: string | null;
  [k: string]: unknown;
}
