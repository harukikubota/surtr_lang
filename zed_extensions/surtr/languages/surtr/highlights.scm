(identifier) @variable
((identifier) @type (#match? @type "^[A-Z]"))
((identifier) @constructor (#match? @constructor "^(Ok|Err)$"))
((identifier) @variable.special (#match? @variable.special "^(_[0-9]*|self)$"))
((identifier) @constant.builtin (#match? @constant.builtin "^__(FILE|DIR|LINE)__$"))
((identifier) @operator (#match? @operator "^(on|and|or|eq|neq|lt|lte|gt|gte)$"))
(keyword) @keyword
(boolean) @boolean
(number) @number
(type_variable) @type
(capture_placeholder) @variable.special
(operator) @operator
(punctuation) @punctuation.delimiter
(comment) @comment
(annotation) @attribute
(function_literal) @function
(double_content) @string
(single_content) @string
(raw_content) @string
["\"" "'" "\"\"\""] @string
(escape) @string.escape
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
"#{" @punctuation.special

; The highlighting tree keeps sibling tokens and groups. Only immediate
; siblings match, so declarations and calls cannot cross a newline.
(_ (keyword) @_def . (identifier) @function
  (#match? @_def "^(def|defp|defextractor)$"))
(_ (identifier) @function . (parenthesized) (#match? @function "^[a-z_]"))
(_ (identifier) @function . (operator) @_question . (parenthesized)
  (#eq? @_question "?"))
(_ (keyword) @_def . (identifier) @function . (operator) @_question @function
  (#match? @_def "^(def|defp)$") (#eq? @_question "?"))
(_ (identifier) @function . (punctuation) @_sep . (operator) @_angle
  (#eq? @_sep "::") (#eq? @_angle "<"))
(_ (operator) @_capture . (identifier) @function (#eq? @_capture "&"))
(_ (punctuation) @_sep . (identifier) @function (#eq? @_sep "::"))
(_ (punctuation) @_dot . (identifier) @property (#eq? @_dot "."))
