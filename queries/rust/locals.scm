; What `gd` needs and the grammar crate does not ship: where names are bound,
; and how far those bindings reach. Capture names are the ones every other
; locals query uses, so JavaScript's - which its crate does ship - drops
; straight into the same code.

; Scopes. A binding reaches to the end of the innermost of these that holds
; it, and `gd` walks out through them from the cursor: the nearest binding of
; a name wins, which is what makes shadowing come out right.
[
  (function_item)
  (function_signature_item)
  (closure_expression)
  (block)
  (match_arm)
  (for_expression)
  (while_expression)
  (loop_expression)
  (if_expression)
  (impl_item)
  (trait_item)
  (source_file)
] @local.scope

; Bindings. `let` first, through the patterns that are common enough to matter
; - a tuple destructuring is how half the bindings in this editor are written.
(let_declaration pattern: (identifier) @local.definition)
(let_declaration pattern: (ref_pattern (identifier) @local.definition))
(let_declaration pattern: (mut_pattern (identifier) @local.definition))
(let_declaration pattern: (tuple_pattern (identifier) @local.definition))
(let_condition pattern: (identifier) @local.definition)

; Function and closure parameters.
(parameter pattern: (identifier) @local.definition)
(parameter pattern: (mut_pattern (identifier) @local.definition))
(parameter pattern: (tuple_pattern (identifier) @local.definition))
(closure_parameters (identifier) @local.definition)
(closure_parameters (mut_pattern (identifier) @local.definition))

; The loop variable, and the names a match arm binds.
(for_expression pattern: (identifier) @local.definition)
(for_expression pattern: (tuple_pattern (identifier) @local.definition))
(match_pattern (identifier) @local.definition)
(tuple_struct_pattern (identifier) @local.definition)
(struct_pattern (field_pattern (identifier) @local.definition))

; Items that are values rather than types: `gd` on one of these should reach
; the declaration, and the tags query only covers the outermost ones.
(const_item name: (identifier) @local.definition)
(static_item name: (identifier) @local.definition)
