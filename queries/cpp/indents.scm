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
