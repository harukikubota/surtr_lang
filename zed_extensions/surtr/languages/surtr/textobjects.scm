(function_declaration) @function.around
(function_declaration
  body: (block "{" . (_)* @function.inside . "}"))
(function_declaration body: (expression_body) @function.inside)
(container_declaration) @class.around
(container_declaration
  body: (block "{" . (_)* @class.inside . "}"))
(comment) @comment.around
