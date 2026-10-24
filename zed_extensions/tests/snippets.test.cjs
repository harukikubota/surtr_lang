const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');

test('Test snippets use the current module API and preserve assertion results', () => {
  const file = path.join(root, 'surtr/snippets/surtr.json');
  assert.ok(fs.existsSync(file), 'Surtr snippets must be registered');
  const snippets = JSON.parse(fs.readFileSync(file, 'utf8'));
  const byPrefix = new Map(Object.values(snippets).map(s => [s.prefix, s]));
  const moduleSource = fs.readFileSync(path.join(root, '../lib/test.srt'), 'utf8');
  for (const prefix of ['test', 'describe', 'it', 'xit', 'pend', 'assert_eq', 'assert_true', 'assert_false']) {
    assert.ok(moduleSource.includes(`def ${prefix}(`), `Unknown Test member: ${prefix}`);
    const snippet = byPrefix.get(prefix);
    assert.ok(snippet, `Missing snippet: ${prefix}`);
    const body = snippet.body.join('\n');
    assert.ok(body.includes(`${prefix}(`));
    assert.ok(!body.includes('@test'));
    if (['test', 'describe', 'it', 'xit'].includes(prefix)) {
      assert.match(body, /assert_eq\(.*\)\n\s*\}/, 'Return the last assertion Result');
    }
  }
  assert.match(byPrefix.get('test').body.join('\n'), /^import Test;/);
});
