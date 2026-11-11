# README screenshot: docs/screenshot_2_code.png
Source "docs/tapes/common.tape"

# bring the section to the top: find it, go to the end, then find it again (the search wraps
# around and the editor scrolls up to it, leaving it on the first row)
Ctrl+F
Type "Fenced code blocks can name"
Escape
PageDown 30
Ctrl+F
Escape

Show
Sleep 500ms
Screenshot "docs/screenshot_2_code.png"
