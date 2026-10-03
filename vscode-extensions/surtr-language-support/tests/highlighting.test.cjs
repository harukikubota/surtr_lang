const assert = require('node:assert/strict');
const { test, before } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const { Registry, parseRawGrammar, INITIAL } = require('vscode-textmate');
const { loadWASM, OnigScanner, OnigString } = require('vscode-oniguruma');

let grammar;
before(async () => {
  const wasm = fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm'));
  await loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
  const grammarPath = path.join(__dirname, '../syntaxes/surtr.tmGrammar.json');
  const registry = new Registry({
    onigLib: Promise.resolve({
      createOnigScanner: patterns => new OnigScanner(patterns),
      createOnigString: value => new OnigString(value)
    }),
    loadGrammar: async scope => scope === 'source.surtr'
      ? parseRawGrammar(fs.readFileSync(grammarPath, 'utf8'), grammarPath) : null
  });
  grammar = await registry.loadGrammar('source.surtr');
});

function tokens(source) {
  let state = INITIAL;
  return source.split('\n').map(line => {
    const result = grammar.tokenizeLine(line, state);
    state = result.ruleStack;
    return result.tokens.map(token => ({
      text: line.slice(token.startIndex, token.endIndex), scopes: token.scopes
    }));
  });
}

function scope(source, text, expected, line = 0) {
  const found = tokens(source)[line].find(token => token.text === text);
  assert.ok(found, `Missing token ${JSON.stringify(text)} in ${JSON.stringify(source)}`);
  assert.ok(found.scopes.includes(expected), `${text}: ${found.scopes.join(', ')} expected ${expected}`);
}

test('current declaration keywords and modifiers', () => {
  for (const word of ['def', 'defp', 'defmod', 'namespace', 'deftrait', 'defstruct',
    'defrecord', 'deferror', 'defenum', 'defextractor', 'defagent', 'defgenserver',
    'defsupervisor', 'defdynamic_supervisor', 'supervisor_init', 'impl', 'for',
    'import', 'include', 'type', 'Type', 'where', 'const']) {
    scope(word, word, 'keyword.declaration.surtr');
  }
  for (const word of ['private', 'public', 'readonly']) {
    scope(word, word, 'storage.modifier.surtr');
  }
  for (const word of ['match', 'cond', 'do', 'when']) {
    scope(word, word, 'keyword.control.surtr');
  }
});

test('only an annotation name at the start of a line is classified', () => {
  for (const [source, name] of [['@doc', '@doc'], ['  @derive(Eq)', '@derive'],
    ['\t@test', '@test'], ['    @result_effect', '@result_effect'], ['@A0_b', '@A0_b'], ['@a', '@a']]) {
    scope(source, name, 'storage.modifier.annotation.surtr');
  }
});

test('@ in expressions and non-annotation spellings stay unclassified', () => {
  for (const source of ['call() @timeout(100ms)', 'call() @ timeout(100ms)',
    'pattern @whole', 'pattern @ whole', ' @ timeout(100ms)', '@@doc', '@_doc', '@0doc']) {
    const found = tokens(source)[0];
    assert.ok(!found.some(token => token.scopes.some(value =>
      value === 'storage.modifier.annotation.surtr' || value === 'keyword.operator.pattern.surtr')));
    const at = found.find(token => token.text.includes('@'));
    assert.ok(at, source);
    assert.deepEqual(at.scopes, ['source.surtr'], source);
  }
});

test('line-leading @ inside strings, comments and interpolation is excluded', () => {
  for (const source of ['"""\n@doc\n"""', '# @doc', '"#{\n@timeout(100ms)\n}"']) {
    assert.ok(!tokens(source).flat().some(token =>
      token.scopes.includes('storage.modifier.annotation.surtr')), source);
  }
});

test('function declarations, captures, qualified calls and return type arguments', () => {
  scope('defp foo(value: Int) -> Int { value }', 'foo', 'entity.name.function.surtr');
  scope('defextractor take(value: Int) { Ok(value) }', 'take', 'entity.name.function.surtr');
  scope('&normalize', 'normalize', 'entity.name.function.surtr');
  scope('&String::trim', 'trim', 'entity.name.function.surtr');
  scope('try_to::<List<Int>>(value)', 'try_to', 'entity.name.function.surtr');
  scope('Map::empty::<Int>()', 'empty', 'entity.name.function.surtr');
  scope('String::trim(value)', '::', 'punctuation.accessor.surtr');
});

test('boolean, result constructors, unit, type parameters and field paths', () => {
  scope('True', 'True', 'constant.language.boolean.surtr');
  scope('False', 'False', 'constant.language.boolean.surtr');
  scope('Ok(1)', 'Ok', 'support.function.constructor.surtr');
  scope('Err(error)', 'Err', 'support.function.constructor.surtr');
  scope('()', '()', 'constant.language.unit.surtr');
  scope('$Element', '$Element', 'entity.name.type.parameter.surtr');
  scope('Tuple._1', '_1', 'variable.other.member.surtr');
  scope('User.name', 'name', 'variable.other.member.surtr');
});

test('reserved infix names, ordinary calls, and canonical Pattern consumers', () => {
  for (const word of ['on', 'and', 'or', 'eq', 'neq', 'lt', 'lte', 'gt', 'gte']) {
    scope(`a ${word} b`, word, 'keyword.operator.word.surtr');
  }
  for (const word of ['if', 'if_then', 'return', 'guard', 'pure']) {
    scope(`${word}(value)`, word, 'entity.name.function.surtr');
  }
  for (const word of ['if_let', 'if_let_then', 'is_match', 'apply_pattern']) {
    scope(`${word}(value, _)`, word, 'support.function.pattern.surtr');
    scope(`Kernel::${word}(value, _)`, word, 'support.function.pattern.surtr');
  }
  scope('Regex::is_match(rx, text)', 'is_match', 'entity.name.function.surtr');
});

test('operators are consumed whole, including current pipe and composition operators', () => {
  for (const op of ['|>', '|*>', '|*|', '|>=', '<|>', '>>', '>*', '>=>',
    '=?', '<-', '=>', '->', '&&', '||', '==', '!=', '<=', '>=', '++',
    '+', '-', '*', '/', '%', '!', '=', '<', '>', '&', '~', '^', '|', '..', '?']) {
    scope(`a ${op} b`, op, 'keyword.operator.surtr');
  }
});

test('capture placeholders and numbered projections have their own scopes', () => {
  for (const n of [1, 2, 16]) {
    scope(`&f(&${n})`, `&${n}`, 'variable.parameter.capture.surtr');
    scope(`apply_pattern(value, _${n})`, `_${n}`, 'variable.language.placeholder.surtr');
  }
  scope('value |> f(_1)', '_1', 'variable.language.placeholder.surtr');
});

test('integer bases, floats, durations and ranges', () => {
  for (const value of ['42', '0b101', '0o77', '0d42', '0xFa', '3.14']) {
    scope(value, value, 'constant.numeric.surtr');
  }
  scope('100ms', '100', 'constant.numeric.surtr');
  scope('100ms', 'ms', 'keyword.other.unit.surtr');
  scope('1..10', '..', 'keyword.operator.surtr');
  scope('-42', '-', 'keyword.operator.surtr');
});

test('backtick function literals do not leak comment or string syntax', () => {
  for (const literal of ['`+`', '`/`', '`%`', '`(,)`', '`Boolean::not`', '`combine`']) {
    scope(literal, literal.slice(1, -1), 'entity.name.function.surtr');
  }
});

test('both quoted strings highlight interpolation expressions and escape boundaries', () => {
  for (const quote of ['"', "'"]) {
    const source = `${quote}value #{f({|x| x + 1})} end${quote} + 2`;
    scope(source, 'f', 'entity.name.function.surtr');
    scope(source, '1', 'constant.numeric.surtr');
    scope(source, ' end', `string.quoted.${quote === '"' ? 'double' : 'single'}.surtr`);
    scope(source, '2', 'constant.numeric.surtr');
    scope(`${quote}\\#{literal}${quote}`, '\\#{', 'constant.character.escape.surtr');
    scope(`${quote}\\n${quote}`, '\\n', 'constant.character.escape.surtr');
  }
});

test('interpolation respects nested strings, comments and braces across lines', () => {
  const source = '"#{do {\n# } ignored\nOk(\'brace }\')\n}} tail"\nmatch value { _ => 0 }';
  scope(source, '# } ignored', 'comment.line.number-sign.surtr', 1);
  scope(source, 'Ok', 'support.function.constructor.surtr', 2);
  scope(source, ' tail', 'string.quoted.double.surtr', 3);
  scope(source, 'match', 'keyword.control.surtr', 4);
});

test('raw triple strings interpolate without treating backslashes as ordinary escapes', () => {
  const source = '"""\nraw \\n #{1 + 2}\n"""\ndo { Ok(1) }';
  scope(source, '1', 'constant.numeric.surtr', 1);
  scope(source, 'do', 'keyword.control.surtr', 3);
  assert.ok(!tokens(source)[1].some(token => token.text === '\\n' &&
    token.scopes.includes('constant.character.escape.surtr')));
});

test('triple strings after @doc use ordinary string scopes', () => {
  for (const source of ['@doc """example"""\ndef foo() { () }',
    '@doc\n"""example"""\ndef foo() { () }']) {
    const all = tokens(source).flat();
    assert.ok(all.some(token => token.scopes.includes('string.quoted.triple.surtr')));
    assert.ok(!all.some(token => token.scopes.includes('comment.block.documentation.surtr')));
    assert.ok(all.some(token => token.text === 'foo' && token.scopes.includes('entity.name.function.surtr')));
  }
});

test('regex generated strings and comments contain operator and keyword spellings', () => {
  for (const quote of ['"', "'"]) {
    scope(`re${quote}^a+#$${quote}`, '^a+#$', 'string.regexp.surtr');
    scope(`re${quote}#{pattern}${quote}`, '#{', 'punctuation.section.interpolation.begin.surtr');
  }
  scope('# @doc def foo |> "text"', '# @doc def foo |> "text"', 'comment.line.number-sign.surtr');
});

test('keyword prefixes and ordinary identifier suffixes are not split', () => {
  for (const name of ['deftrait_extra', 'and_then', 'if_let_more', 'True_value', 'value42']) {
    const found = tokens(name)[0];
    assert.ok(!found.some(token => token.scopes.some(value =>
      value.startsWith('keyword.') || value === 'constant.language.boolean.surtr')));
  }
});

test('obsolete spellings do not restore old syntax categories', () => {
  assert.ok(!tokens('@@doc')[0].some(token =>
    token.scopes.includes('storage.modifier.annotation.surtr')));
  assert.ok(!tokens('if')[0].some(token => token.scopes.includes('keyword.control.surtr')));
  assert.ok(!tokens('|=>')[0].some(token => token.text === '|=>'));
});

test('incomplete documentation or strings do not swallow subsequent declarations', () => {
  scope('@doc\ndef next() { () }', 'def', 'keyword.declaration.surtr', 1);
  scope('@doc # note\n"""body"""\ndef next() { () }', 'next', 'entity.name.function.surtr', 2);
  scope('"escaped \\" quote"\ndef next() { () }', 'next', 'entity.name.function.surtr', 1);
});

test('editor configuration selects current tokens and indents brace blocks', () => {
  const config = JSON.parse(fs.readFileSync(path.join(__dirname, '../language-configuration.json'), 'utf8'));
  const word = new RegExp(config.wordPattern, 'g');
  for (const value of ['$Element', '&16', 'Kernel::apply_pattern', '0xFF', '100ms']) {
    assert.deepEqual(value.match(word), [value]);
  }
  const indent = new RegExp(config.indentationRules.increaseIndentPattern);
  for (const line of ['deftrait Eq {', 'do::<Result> {', 'defagent Cache { # state',
    'defdynamic_supervisor Pool {', '*{|value: Int|', 'User {']) {
    assert.ok(indent.test(line), line);
  }
  assert.ok(!indent.test('# do {'));
  assert.ok(config.autoClosingPairs.some(pair => pair.open === '`' && pair.close === '`'));
});

test('standard library sources finish tokenization outside strings and documentation', () => {
  const libPath = path.join(__dirname, '../../../lib');
  const sources = fs.readdirSync(libPath, { recursive: true }).filter(name => name.endsWith('.srt'));
  assert.ok(sources.length > 0);
  for (const name of sources) {
    let state = INITIAL;
    const lines = fs.readFileSync(path.join(libPath, name), 'utf8').split('\n');
    for (const [index, line] of lines.entries()) {
      const result = grammar.tokenizeLine(line, state);
      assert.ok(!result.stoppedEarly, `${name}:${index + 1}: tokenization stopped early`);
      state = result.ruleStack;
    }
    assert.equal(state.depth, 1, `${name}: unclosed string, interpolation or documentation scope`);
  }
});
