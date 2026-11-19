# Models view (2), Catppuccin Mocha, 7 days, by cost  ->  docs/catppuccin.png
# Regenerate every screenshot with scripts/screenshots.sh, run from the repo root.
# The terminal background matches the theme's painted ground (#1E1E2E) so the frame is seamless.
Output target/screenshots/catppuccin.webm
Set Shell "bash"
Set Width 952
Set Height 568
Set FontSize 16
Set FontFamily "Menlo"
Set LetterSpacing 0
Set LineHeight 1.13
Set Padding 10
Set Margin 0
Set BorderRadius 14
Set Theme { "name": "catppuccin", "background": "#1E1E2E", "foreground": "#CDD6F4", "cursor": "#1E1E2E" }
Env COLORTERM "truecolor"
Hide
Type "target/release/cc-usage --demo --color truecolor --theme catppuccin --range 7d"
Enter
Show
Wait+Screen /1 Daily/
Type "2"
Wait+Screen /Models · [0-9]/
Sleep 2s
Screenshot tools/cc-usage/docs/catppuccin.png
Sleep 2s
Type "q"
Sleep 500ms
