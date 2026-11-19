# Daily view (1), Daylight (light theme), 7 days, by cost  ->  docs/daylight.png
# Regenerate every screenshot with scripts/screenshots.sh, run from the repo root.
# The terminal background matches the theme's painted ground (#F3F0EA) so the frame is seamless.
Output target/screenshots/daylight.webm
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
Set Theme { "name": "daylight", "background": "#F3F0EA", "foreground": "#1F2933", "cursor": "#F3F0EA" }
Env COLORTERM "truecolor"
Hide
Type "target/release/cc-usage --demo --color truecolor --theme daylight --range 7d"
Enter
Show
Wait+Screen /Daily · 7 days/
Sleep 2s
Screenshot tools/cc-usage/docs/daylight.png
Sleep 2s
Type "q"
Sleep 500ms
