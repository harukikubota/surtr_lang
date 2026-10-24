(function_declaration
  kind: (keyword) @context
  name: (_) @name
  suffix: (operator)? @name) @item
(container_declaration
  kind: (keyword) @context
  name: (container_name) @name) @item
(_ (keyword) @_kind . (identifier) @name @item
  (#match? @_kind "^(const|type|Type)$"))
