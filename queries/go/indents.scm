; Braces, and the two Go shapes that are not braces: a composite literal's
; body, and the parenthesised run of a grouped import or var block.

[
  (block)
  (literal_value)
  (argument_list)
  (parameter_list)
  (type_parameter_list)
  (field_declaration_list)
  (interface_type)
  (import_spec_list)
  (const_declaration)
  (var_declaration)
  (type_declaration)
  (expression_case)
  (default_case)
  (communication_case)
] @indent

[
  "}"
  ")"
  "]"
] @outdent

; A continuation line under an open paren takes the column, not a tab - but
; only when the paren has something after it to line up under. Go's own
; formatter never leaves one open, so this is for the code gofmt has not seen.
[
  (argument_list)
  (parameter_list)
  (type_parameter_list)
] @align
