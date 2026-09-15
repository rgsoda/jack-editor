; An element indents what it contains; its closing tag comes back out. A
; void element - <br>, <img> - has no end tag and so never indents anything.

(element) @indent
(script_element) @indent
(style_element) @indent

(end_tag) @outdent
