const assert = require('node:assert/strict');
const { test } = require('node:test');
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const repo = path.resolve(root, '..');
const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { cwd: repo, encoding: 'utf8' }));
const binary = path.join(metadata.target_directory, 'debug', process.platform === 'win32' ? 'surtr.exe' : 'surtr');
const snippets = JSON.parse(fs.readFileSync(path.join(root, 'surtr/snippets/surtr.json'), 'utf8'));

// This catalog uses default tab stops, linked numeric stops and escaped dollars.
// Reject unsupported placeholder forms instead of silently changing the source.
function expand(body) {
  const source = body.join('\n');
  const defaults = new Map([...source.matchAll(/\$\{(\d+):([^{}]*)\}/g)].map(m => [m[1], m[2]]));
  defaults.set('0', '');
  return source.replace(/\\\$|\$\{(\d+):([^{}]*)\}|\$(\d+)|\$/g, (token, definition, value, reference) => {
    if (token === '\\$') return '$';
    const key = definition || reference;
    assert.ok(defaults.has(key), `Unsupported or undefined snippet stop: ${token}`);
    return defaults.get(key);
  });
}

const cases = {
  def: { after: 'print(inspect(name(3)))', output: '3' },
  defp: { after: 'print(inspect(name(3)))', output: '3' },
  defexpr: { after: 'print(inspect(name(3)))', output: '3' },
  defbool: { after: 'print(inspect(positive?(1)))', output: 'True' },
  defgeneric: { after: 'print(inspect(identity(3)))\nprint(identity("hello"))', output: '3\nhello' },
  defmod: { mode: 'module' },
  defstruct: { after: 'instance = Example(1)\nprint(inspect(instance.value))', output: '1' },
  defrecord: { after: 'instance = Example(1)\nprint(inspect(instance.value))', output: '1' },
  defenum: { after: 'value = match Example::Value(1) { Example::Empty => 0, Example::Value(n) => n }\nprint(inspect(value))', output: '1' },
  deferror: { after: 'print(Error::message(ExampleError("detail")))', output: 'detail' },
  deftrait: {},
  impl: { before: 'defstruct Example { value: Int }', after: 'print(inspect(Example::get(Example(1))))', output: '1' },
  impltrait: { before: 'defrecord Example(value: Int)\ndeftrait ExampleTrait { def get(self: Self) -> Int }', after: 'print(inspect(ExampleTrait::get(Example(1))))', output: '1' },
  const: { after: 'print(inspect(LIMIT))', output: '3' },
  type: { after: 'def apply_transform(value: Int, transform: Transform<Int, Int>) -> Int { transform(value) }\nprint(inspect(apply_transform(3, {|value: Int| value})))', output: '3' },
  match: { wrap: source => `value = ${source}\nprint(inspect(value))`, output: '()' },
  do: { wrap: source => `value = ${source}\nprint(inspect(value))`, output: 'Ok(1)' },
  closure: { wrap: source => `f = ${source}\nprint(inspect(f(3)))`, output: '3' },
  safebind: { wrap: source => `def checked() -> Result<Int> {\n${source}\nOk(value)\n}\nprint(inspect(checked()))`, output: 'Ok(1)' },
  doc: { after: 'def documented() -> Unit { () }' },
  test: { mode: 'test', passed: 1 },
  describe: { mode: 'test', passed: 1, before: 'import Test;' },
  it: { mode: 'test', passed: 1, before: 'import Test;' },
  xit: { mode: 'test', passed: 1, before: 'import Test;' },
  pend: { mode: 'test', passed: 0, pending: 1, before: 'import Test;' },
  assert_eq: { mode: 'test', passed: 1, wrap: assertionCase },
  assert_true: { mode: 'test', passed: 1, wrap: assertionCase },
  assert_false: { mode: 'test', passed: 1, wrap: assertionCase },
};
function assertionCase(source) { return `import Test;\nit("assertion") {\n${source}\n}`; }

test('every registered snippet has an executable language contract', () => {
  assert.deepEqual(Object.values(snippets).map(s => s.prefix).sort(), Object.keys(cases).sort());
});

for (const snippet of Object.values(snippets)) {
  test(`expanded ${snippet.prefix} follows the current Surtr contract`, () => {
    const context = cases[snippet.prefix];
    assert.ok(context, `Missing contract for ${snippet.prefix}`);
    const expanded = expand(snippet.body);
    const source = [context.before || '', context.wrap ? context.wrap(expanded) : expanded, context.after || ''].join('\n');
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'surtr-snippet-'));
    try {
      const file = path.join(dir, 'snippet.srt');
      fs.writeFileSync(file, source);
      let entry = file;
      if (context.mode === 'module') {
        entry = path.join(dir, 'entry.srt');
        fs.writeFileSync(entry, 'include "./snippet.srt"\n()\n');
      }
      const args = context.mode === 'test' ? ['test', '--include-xit', entry] : ['run', entry];
      const output = execFileSync(binary, args, { cwd: repo, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] });
      if (context.mode === 'test') {
        assert.match(output, new RegExp(`passed=${context.passed}, failed=0`));
        assert.match(output, new RegExp(`pending=${context.pending || 0}`));
        assert.match(output, /script_errors=0, policy_errors=0/);
      } else {
        assert.equal(output.trim(), context.output || '', source);
      }
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
}
