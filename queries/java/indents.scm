; Braces, and the bodies the grammar gives their own names to - a class body is
; not a block, though it indents like one.

[
  (block)
  (class_body)
  (interface_body)
  (enum_body)
  (enum_body_declarations)
  (annotation_type_body)
  (constructor_body)
  (switch_block)
  (array_initializer)
  (argument_list)
  (formal_parameters)
  (annotation_argument_list)
  (element_value_array_initializer)
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
  (formal_parameters)
  (type_arguments)
] @align
