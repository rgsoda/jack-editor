; TOML has no nesting to speak of: a table's keys sit at the left margin, as
; every TOML file written by hand does. What is left is the two things that
; really do open and close - an array over several lines, and an inline table.

[
  (array)
  (inline_table)
] @indent

[
  "]"
  "}"
] @outdent
