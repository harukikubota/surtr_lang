const assert = require('node:assert/strict');
const { test } = require('node:test');
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const cli = path.join(root, 'node_modules/.bin/tree-sitter');
const grammar = path.join(root, 'tree-sitter-surtr');
function inspect(source, queryName = 'highlights.scm') {
  const query = path.join(root, 'surtr/languages/surtr', queryName);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'surtr-highlight-'));
  const file = path.join(dir, 'input.srt');
  try {
    fs.writeFileSync(file, source);
    const options = { cwd: grammar, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] };
    const tree = execFileSync(cli, ['parse', file], options);
    const captures = execFileSync(cli, ['query', '--captures', query, file], {
      ...options
    });
    return { tree, captures };
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

function has(source, capture, text) {
  const { captures } = inspect(source);
  assert.ok(captures.split('\n').some(line =>
    line.includes(` ${capture},`) && line.endsWith(`text: \`${text}\``)), captures);
}

test('declarations, calls, types, booleans and ordinary keyword prefixes', () => {
  has('def positive?(value: Int) -> Boolean { value > 0 }', 'function', 'positive');
  has('def positive?(value: Int) -> Boolean { value > 0 }', 'function', '?');
  has('String::trim(value)', 'function', 'trim');
  has('try_to::<List<Int>>(value)', 'function', 'try_to');
  has('deftrait Eq {}', 'keyword', 'deftrait');
  has('True', 'boolean', 'True');
  has('Ok(1)', 'constructor', 'Ok');
  has('$Element', 'type', '$Element');
  has('User.name', 'property', 'name');
  has('deftrait_extra', 'variable', 'deftrait_extra');
  has('if(value, 1, 0)', 'function', 'if');
});

test('comments, annotations, numeric bases, captures and complete operators', () => {
  has('  @doc "text"', 'attribute', '@doc');
  has('# def foo "x"', 'comment', '# def foo "x"');
  for (const n of ['42', '0b101', '0o77', '0d42', '0xFa', '3.14', '100ms']) {
    has(n, 'number', n);
  }
  for (const op of ['|>', '|*>', '|*|', '|>=', '<|>', '>>', '>*', '>=>', '=?', '<-', '=>', '->', '..']) {
    has(`a ${op} b`, 'operator', op);
  }
  has('&16', 'variable.special', '&16');
  has('_1', 'variable.special', '_1');
  has('`Boolean::not`', 'function', '`Boolean::not`');
});

test('quoted strings preserve nested interpolation, escapes and following code', () => {
  for (const q of ['"', "'"]) {
    const source = `${q}before #{f({|x| x + 1})} after${q}\ndef next() { () }`;
    has(source, 'function', 'f');
    has(source, 'number', '1');
    has(source, 'function', 'next');
    has(`${q}\\u{1b} \\#{literal}${q}`, 'string.escape', '\\u{1b}');
    assert.ok(!inspect(`${q}\\#{literal}${q}`).tree.includes('(interpolation'));
  }
});

test('raw strings keep quotes, backslashes, escaped interpolation and comments as text', () => {
  const source = '@doc """\nraw " and "" and \\n # text \\#{literal} #{1 + 2}\n"""\ndef next() { () }';
  has(source, 'attribute', '@doc');
  has(source, 'number', '2');
  has(source, 'function', 'next');
  const { tree } = inspect(source);
  assert.equal((tree.match(/\(interpolation/g) || []).length, 1, tree);
  assert.ok(!tree.includes('(comment'), tree);
});

test('interpolation handles nested strings and comment braces across lines', () => {
  const source = '"#{do {\n# } ignored\nOk(\'brace }\')\n}} tail"\nmatch value { _ => 0 }';
  has(source, 'comment', '# } ignored');
  has(source, 'constructor', 'Ok');
  has(source, 'keyword', 'match');
});

test('annotation and keyword classifications do not leak into other contexts', () => {
  for (const source of ['call() @timeout(100ms)', '@@doc', '@_doc', '@0doc',
    '"""\n@doc\n"""', '"#{\n@timeout(100ms)\n}"']) {
    assert.ok(!inspect(source).captures.includes(' - attribute,'), source);
  }
  for (const name of ['and_then', 'True_value', 'if_let_more']) {
    const { captures } = inspect(name);
    assert.ok(!captures.includes(' - keyword,') && !captures.includes(' - boolean,'), captures);
  }
  const constructor = inspect('Ok(1)').captures;
  assert.ok(!constructor.split('\n').some(line => line.includes(' - function,') && line.endsWith('text: `Ok`')));
});

test('every standard library source parses without losing string or group boundaries', () => {
  const lib = path.resolve(root, '../lib');
  const sources = fs.readdirSync(lib, { recursive: true })
    .filter(name => name.endsWith('.srt')).map(name => path.join(lib, name));
  assert.ok(sources.length > 0);
  execFileSync(cli, ['parse', '--quiet', ...sources], {
    cwd: grammar, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe']
  });
});

test('matching brackets exclude literal and commented brackets, including escaped interpolation', () => {
  const source = 'f(["#{1}"]) # {} ignored\n"literal [ ] \\#{text}"';
  const { captures } = inspect(source, 'brackets.scm');
  assert.equal((captures.match(/ - open,/g) || []).length, 5, captures);
  assert.equal((captures.match(/ - close,/g) || []).length, 5, captures);
});
