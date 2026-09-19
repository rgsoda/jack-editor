; What a line's indentation should be, as a walk up the tree: every @indent
; ancestor that began on an earlier line is one step, and an @outdent node that
; begins on this line takes one back - which is what puts a closing brace back
; under the thing it closes.

[
  (block)
  (declaration_list)
  (enum_variant_list)
  (field_declaration_list)
  (field_initializer_list)
  (match_block)
  (use_list)
  (arguments)
  (parameters)
  (closure_parameters)
  (ordered_field_declaration_list)
  (array_expression)
  (tuple_expression)
  (tuple_pattern)
  (struct_pattern)
  (token_tree)
  (where_clause)
] @indent

[
  "}"
  "]"
  ")"
] @outdent

; Aligning, rather than stepping: when a call or a list puts something after
; its opening delimiter, what follows on the next line lines up under that
; first item instead of taking a tab. An open paren with nothing after it is
; still a step, so both styles get what they write.
[
  (arguments)
  (parameters)
  (closure_parameters)
  (tuple_expression)
  (tuple_pattern)
  (array_expression)
  (type_arguments)
  (type_parameters)
] @align
