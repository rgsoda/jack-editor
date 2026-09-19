; CSS is one shape: a selector, a brace, declarations. The `block` is the brace
; and everything in it, and it starts on the line the brace is on - which is
; where a step opens.

[
  (block)
  (keyframe_block_list)
] @indent

[
  "}"
  ")"
] @outdent
