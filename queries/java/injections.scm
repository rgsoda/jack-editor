; SQL in a string. The reasoning is in queries/rust/injections.scm.
((string_literal (string_fragment) @injection.content)
 (#match? @injection.content "(?is)^\\s*(SELECT\\b.*\\bFROM\\b|SELECT\\s+\\d|INSERT\\s+INTO\\b|UPDATE\\b.*\\bSET\\b|DELETE\\s+FROM\\b|CREATE\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|ALTER\\s+TABLE\\b|DROP\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|WITH\\b.*\\bAS\\s*\\()")
 (#set! injection.language "sql"))
