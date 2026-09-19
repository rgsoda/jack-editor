; SQL in a string, which no grammar will tell you about: what a string holds is
; not a fact the language knows, so this is ours and it is a guess.
;
; The guess is made narrow on purpose. One leading verb is not enough - a
; string starting "Select a file" is English, and "Update the settings" is too
; - so every shape here wants two SQL tokens in the right order: SELECT with a
; FROM after it, INSERT with INTO, UPDATE with SET, CREATE with what is being
; created. `(?is)` because a query in a source file is usually several lines
; and nobody agrees on the case.
((string_content) @injection.content
 (#match? @injection.content "(?is)^\\s*(SELECT\\b.*\\bFROM\\b|SELECT\\s+\\d|INSERT\\s+INTO\\b|UPDATE\\b.*\\bSET\\b|DELETE\\s+FROM\\b|CREATE\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|ALTER\\s+TABLE\\b|DROP\\s+(TABLE|INDEX|VIEW|DATABASE|SCHEMA)\\b|WITH\\b.*\\bAS\\s*\\()")
 (#set! injection.language "sql"))
