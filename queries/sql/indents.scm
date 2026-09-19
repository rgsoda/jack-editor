; SQL indents by clause, and the clauses are siblings rather than nested: a
; `select` holds SELECT and its column list, and the `from` beside it holds
; FROM and the rest. So indenting `select` puts the columns one step in and
; leaves FROM where it was, which is what a formatted query looks like:
;
;   SELECT
;       a,
;       b
;   FROM t
;
; `from` is deliberately not on the list. WHERE is a child of it in this
; grammar, and a step for that would push every WHERE line to the right.

[
  (select)
  (subquery)
  (column_definitions)
  (case)
  (block)
] @indent

; A parenthesised list lines up under its first item when there is one, the
; way an argument list does in any other language.
[
  (subquery)
  (column_definitions)
] @align

[
  ")"
  (keyword_end)
] @outdent
