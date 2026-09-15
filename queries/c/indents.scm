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
