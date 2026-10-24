; Indent the contents, keeping closing delimiters at the outer indentation.
(block "{" @start "}" @end) @indent
(parenthesized "(" @start ")" @end) @indent
(bracketed "[" @start "]" @end) @indent
(interpolation "#{" @start "}" @end) @indent
