// A highlighting grammar: tokens and balanced groups, not a second typechecker
// or a replacement for Spire's parser. Newlines remain visible to queries.
module.exports = grammar({
  name: 'surtr',
  word: $ => $.identifier,
  extras: $ => [/[ \t\r]/, $.comment],
  externals: $ => [$.double_content, $.single_content, $.raw_content, $.annotation],
  rules: {
    source_file: $ => repeat($._item),
    _item: $ => choice($.annotation, $._expression),
    _expression: $ => choice(
      $.keyword, $.boolean, $.identifier, $.type_variable,
      $.number, $.capture_placeholder, $.operator, $.punctuation, $.newline,
      $.parenthesized, $.bracketed, $.block,
      $.double_string, $.single_string, $.raw_string, $.function_literal
    ),
    parenthesized: $ => seq('(', repeat($._item), ')'),
    bracketed: $ => seq('[', repeat($._item), ']'),
    block: $ => seq('{', repeat($._item), '}'),
    keyword: _ => choice(
      'def', 'defp', 'defextractor', 'defmod', 'namespace', 'deftrait',
      'defstruct', 'defrecord', 'deferror', 'defenum', 'defagent',
      'defgenserver', 'defsupervisor', 'defdynamic_supervisor', 'supervisor_init',
      'impl', 'for', 'import', 'include', 'const', 'type', 'Type', 'where',
      'private', 'public', 'readonly', 'match', 'do', 'when', 'cond', 'bulk_update'
    ),
    boolean: _ => choice('True', 'False'),
    identifier: _ => /[A-Za-z_][A-Za-z_0-9]*/,
    type_variable: _ => /\$[A-Za-z_][A-Za-z_0-9]*/,
    number: _ => /(?:0b[01]+|0o[0-7]+|0d[0-9]+|0x[0-9a-fA-F]+|[0-9]+\.[0-9]+|[0-9]+)(?:ms)?/,
    capture_placeholder: _ => /&(?:1[0-6]|[1-9])/,
    operator: _ => choice(
      '|*>', '|>=', '|*|', '<|>', '>=>', '++', '=?', '==', '!=', '<=', '>=',
      '<-', '&&', '||', '..', '=>', '->', '|>', '>>', '>*',
      '+', '-', '*', '/', '%', '!', '=', '<', '>', '&', '~', '^', '|', '?'
    ),
    punctuation: _ => choice('::', '.', ',', ':', ';', '@', '$'),
    newline: _ => '\n',
    comment: _ => token(seq('#', /[^\n]*/)),
    function_literal: _ => token(seq('`', /[^`\n]+/, '`')),
    double_string: $ => seq(field('open', '"'), repeat(choice($.double_content, $.escape, $.interpolation)), field('close', '"')),
    single_string: $ => seq(field('open', "'"), repeat(choice($.single_content, $.escape, $.interpolation)), field('close', "'")),
    raw_string: $ => seq(field('open', '"""'), repeat(choice($.raw_content, $.interpolation)), field('close', '"""')),
    escape: _ => token.immediate(/\\(?:[nt\\"']|u\{[0-9a-fA-F]{1,6}\}|#\{)/),
    interpolation: $ => seq(token(prec(1, '#{')), repeat($._expression), '}'),
  }
});
