; Python indents on a colon, not a brace, and that changes where the step comes
; from. The grammar's `block` starts on the first line of the body rather than
; on the line that opened it, so a block cannot be the thing that indents -
; nothing would be inside it yet. The statement that owns the block is, and it
; starts on the header line where a brace language's `{` would be.
;
; Which is why the clauses are not on this list: an `else` body's ancestors are
; the block, the else_clause and the if_statement, and counting both the clause
; and the statement would indent it twice. The clause earns its keep as an
; @outdent instead, so the `else:` line itself comes back under its `if`.
[
  (function_definition)
  (class_definition)
  (if_statement)
  (for_statement)
  (while_statement)
  (with_statement)
  (try_statement)
  (match_statement)
  (case_clause)

  (argument_list)
  (parameters)
  (list)
  (set)
  (tuple)
  (dictionary)
  (list_comprehension)
  (set_comprehension)
  (dictionary_comprehension)
  (generator_expression)
  (parenthesized_expression)
  (tuple_pattern)
  (list_pattern)
] @indent

; A `case` is indented under its `match`, so it is not on this list: the two
; levels of a match statement are the ones Python actually writes.
[
  (elif_clause)
  (else_clause)
  (except_clause)
  (finally_clause)
  "]"
  ")"
  "}"
] @outdent
