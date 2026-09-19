; Keys, and nothing else. The grammar's own query colours every plain scalar as
; a string several patterns before it gets round to saying which of them are
; keys, and here the earliest pattern wins - so on its own, a YAML file is a
; wall of green. This half goes on top, the same way C++'s goes on top of C's.

(block_mapping_pair
  key: (flow_node
    [
      (double_quote_scalar)
      (single_quote_scalar)
    ] @property))

(block_mapping_pair
  key: (flow_node
    (plain_scalar
      (string_scalar) @property)))

(flow_mapping
  (_
    key: (flow_node
      [
        (double_quote_scalar)
        (single_quote_scalar)
      ] @property)))

(flow_mapping
  (_
    key: (flow_node
      (plain_scalar
        (string_scalar) @property))))
