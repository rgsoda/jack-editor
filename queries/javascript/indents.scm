[
  (statement_block)
  (class_body)
  (switch_body)
  (object)
  (object_pattern)
  (array)
  (array_pattern)
  (arguments)
  (formal_parameters)
  (template_substitution)
  (named_imports)
  (jsx_element)
  (jsx_self_closing_element)
] @indent

[
  "}"
  "]"
  ")"
  (jsx_closing_element)
] @outdent

; A continuation line under an open paren takes the column, not a tab - but
; only when the paren has something after it to line up under.
[
  (arguments)
  (formal_parameters)
  (array)
  (parenthesized_expression)
] @align
