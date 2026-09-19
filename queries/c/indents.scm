; C's block is a compound_statement; the rest are the lists that a brace or a
; paren opens - a struct's fields, an enum's constants, an initialiser.

[
  (compound_statement)
  (field_declaration_list)
  (enumerator_list)
  (initializer_list)
  (argument_list)
  (parameter_list)
  (case_statement)
] @indent

[
  "}"
  ")"
  "]"
] @outdent

; A continuation line under an open paren takes the column, not a tab - but
; only when the paren has something after it to line up under.
[
  (argument_list)
  (parameter_list)
  (initializer_list)
] @align
