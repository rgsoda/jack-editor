; SQL in a string. The reasoning is in queries/rust/injections.scm. Go's
; backtick string is the one that matters here: a query long enough to want a
; raw string is a query long enough to want highlighting.
((raw_string_literal (raw_string_literal_content) @injection.content)
 (#match? @injection.content "(?is)^\\s*(SELECT\\b.*\\bFROM\\b|SELECT\\s+\\d|INSERT\\s+INTO\\b|UPDATE\\b.*\\bSET\\b|DELETE\\s+FROM\\b|CREATE\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|ALTER\\s+TABLE\\b|DROP\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|WITH\\b.*\\bAS\\s*\\()")
 (#set! injection.language "sql"))

((interpreted_string_literal (interpreted_string_literal_content) @injection.content)
 (#match? @injection.content "(?is)^\\s*(SELECT\\b.*\\bFROM\\b|SELECT\\s+\\d|INSERT\\s+INTO\\b|UPDATE\\b.*\\bSET\\b|DELETE\\s+FROM\\b|CREATE\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|ALTER\\s+TABLE\\b|DROP\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|WITH\\b.*\\bAS\\s*\\()")
 (#set! injection.language "sql"))
