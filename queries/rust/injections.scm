; Where the code says so, it is not a guess. `sqlx::query!("...")` names the
; language in the macro around the string, and so do its relatives: the whole
; point of those macros is that the string is SQL and is checked as SQL at
; compile time. Reading none of the string means `SELECT id` on its own, or
; `VACUUM`, or anything else the heuristic below will not risk on two tokens,
; is highlighted here anyway.
; `query_file!` is not here: its string is a path, not a query.
((macro_invocation
   macro: [(identifier) @_name
           (scoped_identifier name: (identifier) @_name)]
   (token_tree [(string_literal (string_content) @injection.content)
                (raw_string_literal (string_content) @injection.content)]))
 (#match? @_name "^query(_as|_scalar)?(_unchecked)?$")
 (#set! injection.language "sql"))

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
