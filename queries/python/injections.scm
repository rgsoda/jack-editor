; SQL in a string. The reasoning, and the shape of the guess, is in
; queries/rust/injections.scm - this is the same rule over Python's nodes.
((string (string_content) @injection.content)
 (#match? @injection.content "(?is)^\\s*(SELECT\\b.*\\bFROM\\b|SELECT\\s+\\d|INSERT\\s+INTO\\b|UPDATE\\b.*\\bSET\\b|DELETE\\s+FROM\\b|CREATE\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|ALTER\\s+TABLE\\b|DROP\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|WITH\\b.*\\bAS\\s*\\()")
 (#set! injection.language "sql"))
