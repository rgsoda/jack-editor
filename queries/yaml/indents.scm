; YAML nests by indentation, which makes it Python's problem rather than a
; brace language's: a `block_mapping` starts at its first key, on the line
; *after* the one that opened it, so it cannot be what indents its own
; contents. The pair that owns it is, because it starts up on the key line
; where a brace would have been.
;
;   a:            <- block_mapping_pair `a`, the step opens here
;     b: 1        <- inside it
;
; `block_mapping` and `block_sequence` are deliberately not on the list. Both
; start on the same line as their first item, so counting them would either do
; nothing or double the pair that already counted.

[
  (block_mapping_pair)
  (block_sequence_item)
  (flow_mapping)
  (flow_sequence)
] @indent

[
  "}"
  "]"
] @outdent
