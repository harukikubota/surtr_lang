// Editor grammar: declaration boundaries, tokens and balanced groups.
// Spire remains responsible for syntax validation. Newlines stay queryable.
module.exports = grammar({
  name: 'surtr',
  word: $ => $.identifier,
  extras: $ => [/[ \t\r]/, $.comment],
  externals: $ => [$.double_content, $.single_content, $.raw_content, $.annotation],
  rules: {
    source_file: $ => repeat($._item),
    _item: $ => choice($.annotation, $.function_declaration, $.container_declaration, $._expression),
    _expression: $ => choice(
      $.keyword, $.boolean, $.identifier, $.type_variable,
      $.number, $.capture_placeholder, $.operator, $.punctuation, $.newline,
      $.parenthesized, $.bracketed, $.block,
      $.double_string, $.single_string, $.raw_string, $.function_literal
    ),
    parenthesized: $ => seq('(', repeat($._item), ')'),
    bracketed: $ => seq('[', repeat($._item), ']'),
    block: $ => seq('{', repeat($._item), '}'),
    function_declaration: $ => prec.right(seq(
      field('kind', alias(choice('def', 'defp', 'defextractor'), $.keyword)),
      field('name', choice($.identifier, $.operator, alias('(,)', $.operator))),
      optional(field('suffix', alias(choice('?', '!'), $.operator))),
      optional($.return_type_arguments),
      field('parameters', $.parenthesized),
      optional(seq(alias('->', $.operator), repeat1($._type_item))),
      repeat($.newline),
      optional($.where_clause),
      optional(choice(
        field('body', $.block),
        seq(alias('=', $.operator), field('body', $.expression_body))
      ))
    )),
    expression_body: $ => prec.right(repeat1($._line_expression)),
    return_type_arguments: $ => seq(alias('::', $.punctuation), $.type_arguments),
    type_arguments: $ => seq(
      alias('<', $.operator),
      repeat(choice($.identifier, $.type_variable, $.parenthesized, $.bracketed, $.type_arguments,
        alias(choice('->', '?', '&', '|'), $.operator), alias(choice('::', ','), $.punctuation))),
      alias('>', $.operator)
    ),
    _line_expression: $ => choice(
      $.keyword, $.boolean, $.identifier, $.type_variable, $.number,
      $.capture_placeholder, $.operator, $.punctuation, $.parenthesized,
      $.bracketed, $.block, $.double_string, $.single_string, $.raw_string,
      $.function_literal
    ),
    _type_item: $ => choice(
      $.identifier, $.type_variable, $.parenthesized, $.bracketed,
      alias(choice('Type', 'type'), $.keyword),
      alias(choice('<', '>', '->', '?', '&', '|'), $.operator),
      alias(choice('::', ','), $.punctuation)
    ),
    where_clause: $ => prec.right(seq(
      alias('where', $.keyword),
      repeat1(choice($._type_item, alias(':', $.punctuation), $.newline))
    )),
    container_declaration: $ => prec.right(seq(
      field('kind', alias(choice(
        'defmod', 'namespace', 'deftrait', 'defstruct', 'defrecord', 'deferror',
        'defenum', 'defagent', 'defgenserver', 'defsupervisor', 'defdynamic_supervisor', 'impl'
      ), $.keyword)),
      field('name', $.container_name),
      repeat($.newline),
      optional($.where_clause),
      optional(field('body', $.block))
    )),
    container_name: $ => prec.right(repeat1(choice($._type_item, alias('for', $.keyword)))),
    keyword: _ => choice(
      'supervisor_init', 'for', 'import', 'include', 'const', 'type', 'Type', 'where',
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
