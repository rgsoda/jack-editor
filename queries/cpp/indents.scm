; C's shapes, plus the ones C++ adds: a namespace's declaration_list, a
; template's parameter list, and the initialiser list on a constructor.

[
  (compound_statement)
  (field_declaration_list)
  (enumerator_list)
  (initializer_list)
  (argument_list)
  (parameter_list)
  (case_statement)
  (declaration_list)
  (template_parameter_list)
  (template_argument_list)
  (field_initializer_list)
  (lambda_capture_specifier)
  (requirement_seq)
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
  (template_argument_list)
  (template_parameter_list)
] @align
