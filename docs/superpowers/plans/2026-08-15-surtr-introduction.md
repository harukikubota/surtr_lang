# Surtr Introduction Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a single Markdown page that introduces Surtr to Rust／functional-language developers in about ten minutes, using Notebook-style cells and ASCII diagrams with Result, SafeBind, and Facet as the central narrative.

**Architecture:** Add one user-facing page at `docs/site/surtr-introduction.md`. Organize it as a linear Notebook tour: REPL／Script entry, static types, Result and `match`, SafeBind, Result’s Functor／Applicative／Monad operators, state transitions, Facet updates, and a compact survey of the remaining language features. Extract examples from the approved repository sources and avoid inventing syntax.

**Tech Stack:** Markdown, fenced `surtr`／`text`／`bash` code blocks, ASCII diagrams, existing Surtr source examples and docs.

## Global Constraints

- Keep the deliverable to one page: `docs/site/surtr-introduction.md`.
- Target readers are developers familiar with Rust and functional programming.
- Use the framing “静的型付けされたElixir + Rust + ScalaのExtractor” as an introductory comparison, not as a compatibility claim.
- Make Result, SafeBind (`=?`), and Facet materially deeper than the other feature sections.
- Explain Result as a value-level success／failure container with Functor／Applicative／Monad-style composition.
- Include REPL and Script entry points.
- Use Notebook-style cells with readable input and output blocks.
- Prefer ASCII diagrams so the page does not depend on Mermaid rendering.
- Include static typing, functions／closures, structs／enums, pattern matching／Extractors, traits／operators, imports／includes, and Process briefly.
- Skip JSON, File I/O, and Shell because their current APIs are not strong enough for this introduction.
- Source content from `docs/`, `lib/tests/`, `lib/types/`, `lib/traits/`, and `examples/`; do not add unsupported language claims.
- Do not modify the approved design document while implementing the page.

---

### Task 1: Create the page skeleton and source-backed opening cells

**Files:**
- Create: `docs/site/surtr-introduction.md`
- Read: `docs/site/language-guide.md`
- Read: `docs/site/standard-library.md`
- Read: `docs/dev/Rune_cli_spec.md`
- Read: `docs/dev/Xldr_spec.md`
- Read: `examples/guess.srt`

**Interfaces:**
- Consumes: Existing REPL transcript conventions, `surtr run <file.srt>` CLI contract, and the existing `add`／typed-function／guess examples.
- Produces: A page title, comparison-oriented introduction, compiler-flow diagram, REPL cell, Script cell, and static-type／function cell that later tasks extend.

- [ ] **Step 1: Reconfirm the exact source snippets before writing**

Run:

```bash
sed -n '1,180p' docs/site/language-guide.md
sed -n '38,55p' docs/dev/Rune_cli_spec.md
sed -n '1,80p' examples/guess.srt
```

Use only syntax visible in these sources for the opening cells. Keep the REPL example to a short arithmetic expression and keep the Script example to a typed `def` plus `print`.

- [ ] **Step 2: Write the page title and positioning paragraph**

Start with a concise title such as `# Surtr — 型で失敗を運び、式をつなぐ` and a paragraph that describes Surtr as a statically typed language with an Elixir-like approachable flow, Rust-like type-oriented safety, and Scala-Extractor-like pattern decomposition. Explicitly state that this is a mental model for readers, not a compatibility claim.

- [ ] **Step 3: Add the compiler-flow and Notebook conventions**

Add these two readable diagrams:

```text
Elixir       Rust              Scala Extractor
  │           │                       │
  └── actor   └── 静的型・安全性      └── パターン分解
                  │
                  ▼
                Surtr
```

```text
REPL / Script → Surtr source → Spire → Sigil → Scar → Forge → Eldr → VM
```

Explain that each `Cell` contains a small Surtr input and, where useful, its output.

- [ ] **Step 4: Add REPL, Script, and static-type cells**

Include:

```text
xldr(1)> 1 + 2
> 3
```

```surtr
def add(x: Int, y: Int) -> Int {
  x + y
}

print(to_string(add(20, 22)))
```

and:

```bash
surtr run main.srt
```

Then show `Int`, `String`, `Boolean`, `Unit`, a typed binding, and an expression-returning function. Keep explanation to the type／expression contrast.

- [ ] **Step 5: Check the opening page section**

Run:

```bash
git diff --check -- docs/site/surtr-introduction.md
```

Expected: no whitespace errors. Verify that all opening code fences have language labels where the page uses them (`surtr`, `text`, or `bash`).

### Task 2: Build the Result, `match`, and SafeBind narrative

**Files:**
- Modify: `docs/site/surtr-introduction.md`
- Read: `docs/site/error-handling.md`
- Read: `docs/site/pattern-matching.md`
- Read: `docs/site/function-operators.md`
- Read: `examples/guess.srt`
- Read: `examples/guess_types.srt`

**Interfaces:**
- Consumes: The opening page from Task 1 and the documented Result／pattern／SafeBind behavior.
- Produces: Cells explaining `Ok`／`Err`, `match`, Extractor-style decomposition, SafeBind, and the `match` versus `=?` choice.

- [ ] **Step 1: Add the Result container cell**

Use the visual shape:

```text
Result<T>
├── Ok(value)   ── success path
└── Err(error)  ── failure path
```

Show a `parse_port`-style function returning `Result<Int>` and state that failure is returned as a value rather than raised as an exception. Keep the error example concrete, such as `InvalidPort(port)`.

- [ ] **Step 2: Add the `match` and Extractor-style cell**

Use a `Result` match with `Ok(flag)`, `Err(NoneError)`, and `Err(err)`. Explain that constructors and enum variants expose data through patterns, and that exhaustive coverage is checked. Do not describe Scala Extractors as an implementation dependency; describe the similar pattern-decomposition experience.

- [ ] **Step 3: Add the SafeBind cell**

Use this source-backed shape:

```surtr
def load_pair(a: String, b: String) -> Result<Int> {
  left: Int =? try_from::<Int>(a)
  right: Int =? try_from::<Int>(b)

  Int::safe_div(left + right, 2)
}
```

Add a compact branch diagram showing `Ok(left)` continuing, and either `Err` returning immediately. Explain that `=?` is language-level SafeBind and that `match` remains the explicit choice for recovery or error transformation.

- [ ] **Step 4: Connect the narrative to the guess example**

Add one short paragraph pointing to `examples/guess.srt`: input parsing, `try_from::<Int>`, `map_err`, `|>=`, and concrete errors are combined in a small Script. Do not reproduce the entire game; use it as an evidence link and optional next read.

- [ ] **Step 5: Check Result terminology and scope**

Run:

```bash
rg -n 'Result|SafeBind|match|Extractor|JSON|File I/O|Shell' docs/site/surtr-introduction.md
```

Expected: Result／SafeBind／match appear in the main body; JSON, File I/O, and Shell do not appear in the page body. Confirm that `Error` is described as the observable abstraction around concrete errors, not as a general user-owned value type.

### Task 3: Explain Result as a compositional container and show state transitions

**Files:**
- Modify: `docs/site/surtr-introduction.md`
- Read: `lib/tests/result.srt`
- Read: `lib/traits/operator/functor.srt`
- Read: `lib/traits/operator/applicative.srt`
- Read: `lib/traits/operator/monad.srt`
- Read: `examples/task_state_types.srt`
- Read: `examples/task_state_machine.srt`

**Interfaces:**
- Consumes: Result／SafeBind cells from Task 2.
- Produces: A compact operator table, a compositional pipeline example, and a state-machine example that makes `|>=` concrete.

- [ ] **Step 1: Add the operator correspondence table**

Include exactly these conceptual rows:

| 抽象 | Surtr | 役割 |
|---|---|---|
| pure | `Ok(value)` / `return(value)` | 値をResultへ入れる |
| fmap | `result |*> f` | 成功値だけを変換する |
| bind | `result |>= f` | Resultを返す処理へ接続する |
| applicative | `result |*| other` | Result内の関数と値を組み合わせる |
| lifted compose | `&f >* &g` | 後段の純粋関数をResultの内側へ接続する |
| Kleisli compose | `&f >=> &g` | Result返却関数を直列合成する |

Use inline code so Markdown does not interpret the operators as formatting.

- [ ] **Step 2: Add the Result pipeline Cell**

Use `parse_int`, `require_small`, and `pipeline = &parse_int >=> &require_small` as the focused example. Show one successful `Ok(42)` result and one propagated `Err(...)` result. Explain that the operators encode the same success／failure propagation rules that `=?` makes visible in local bindings.

- [ ] **Step 3: Add the monad-law evidence without turning the page into theory**

Mention that `lib/tests/result.srt` checks left／right identity and associativity-style behavior for `|>=` and `>=>`. Use one sentence to establish that Result is not merely a convention for error returns; it has reusable composition laws.

- [ ] **Step 4: Add the Task state-transition Cell**

Use the existing chain:

```surtr
draft = Task::start("write task-state sample")
open = Task::open(draft)
doing = open |>= Task::assign("haruca")
done = doing |>= Task::complete()
archived = done |>= Task::archive()
```

Add `Draft → Open → Doing → Done → Archived` and state that invalid transitions become `Err(InvalidTaskTransition(...))`, so later steps do not run.

- [ ] **Step 5: Check the operator section for readability**

Run:

```bash
sed -n '/Resultをモナドコンテナ/,/Facet/p' docs/site/surtr-introduction.md
git diff --check -- docs/site/surtr-introduction.md
```

Expected: the table and two Cells fit on a screen-sized reading segment and no operator is missing from the approved set.

### Task 4: Connect Facet to Result and survey the remaining language features

**Files:**
- Modify: `docs/site/surtr-introduction.md`
- Read: `docs/site/facet.md`
- Read: `lib/tests/facet.srt`
- Read: `docs/site/language-guide.md`
- Read: `docs/site/language-features.md`
- Read: `examples/process/agent_singleton_counter/entry.srt`
- Read: `examples/process/task_call/entry.srt`
- Read: `lib/tests/process.srt`

**Interfaces:**
- Consumes: The Result composition model from Task 3.
- Produces: A Result-connected Facet section, a compact feature tour, and a practical “where to go next” summary.

- [ ] **Step 1: Add the Facet read／update Cell**

Introduce a small `User` struct with `name`, `score`, and a Result-valued `nickname` field. Show `Facet::view`, `Facet::over`, and `=?` around each fallible operation. Keep `over_result` as the special operation for rewriting the whole Result focus.

- [ ] **Step 2: Add the Facet relationship diagram**

Use an ASCII diagram equivalent to:

```text
User
 ├─ name      ── view        ── String
 ├─ score     ── over        ── Result<User>
 └─ nickname  ── over_result ── Result<User>
                                │
                                └─ =? で次の処理へ接続
```

Mention `/` path composition, `~source.path`, container paths, and `bulk_update` in one compact list. Do not present Facet as a general-purpose lens library; call it a same-scope path capability with Result-aware operations.

- [ ] **Step 3: Add the compact feature tour**

Give one or two sentences each to:

- `defstruct` / `defenum` / `impl`
- closure, capture `&`, and function values
- `List`, `Tuple`, `Range`, `String`, and `Float`
- traits and operator dispatch
- `import` and `include`
- Extractor／sequence decomposition／exhaustiveness
- Process, `Task::async`, and `Task::await`

Do not add JSON, File I/O, or Shell to this list.

- [ ] **Step 4: Add the closing entry-point summary**

End with a compact mapping:

```text
試す        → surtr repl
実行する    → surtr run example.srt
型で守る    → Result / SafeBind / match
合成する    → |*> / |>= / >* / >=>
構造化する  → defstruct / defenum / impl
更新する    → Facet
並行処理    → Process / Task
```

Add source links or plain repository paths to the most relevant next-reading files, prioritizing Result, Facet, task state, and process examples.

- [ ] **Step 5: Check the feature scope**

Run:

```bash
rg -n 'JSON|File I/O|Shell|Process|Task|Facet|Result|SafeBind|Extractor' docs/site/surtr-introduction.md
```

Expected: Process, Task, Facet, Result, SafeBind, and Extractor are present; JSON, File I/O, and Shell are absent.

### Task 5: Validate the finished page and commit the content

**Files:**
- Modify: `docs/site/surtr-introduction.md`
- Read: `docs/superpowers/specs/2026-08-15-surtr-introduction-design.md`

**Interfaces:**
- Consumes: The complete page from Tasks 1–4 and the approved design.
- Produces: A self-contained, source-backed, one-page introduction ready for review.

- [ ] **Step 1: Check the final structure and source claims**

Run:

```bash
rg -n '^# |^## |^### |^```|Cell|Result|SafeBind|Facet' docs/site/surtr-introduction.md
```

Verify that the page has a single top-level title, sequential Cell headings, readable code／output blocks, and no unsupported claims beyond the approved framing.

- [ ] **Step 2: Scan for accidental scope expansion and placeholders**

Run:

```bash
if rg -n 'TBD|TODO|FIXME|JSON|File I/O|Shell' docs/site/surtr-introduction.md; then exit 1; else echo 'scope and placeholder scan passed'; fi
```

Expected: the command prints `scope and placeholder scan passed`.

- [ ] **Step 3: Run whitespace and repository-status checks**

Run:

```bash
git diff --check
git status --short
```

Expected: no diff-check output; only the intended introduction page is uncommitted.

- [ ] **Step 4: Commit the page**

Run:

```bash
git add docs/site/surtr-introduction.md
git commit -m "docs: add Surtr introduction page"
```

- [ ] **Step 5: Report the finished artifact**

Provide the absolute file link, summarize the Result／SafeBind／Facet emphasis, and state that the page intentionally omits JSON, File I/O, and Shell. No compiler test suite is required because this change only adds Markdown; validation is the source／scope／whitespace checks above.
