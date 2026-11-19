# Projects view (3), Amber, 7 days, by cost  ->  docs/amber.png
# Regenerate every screenshot with scripts/screenshots.sh, run from the repo root.
# The terminal background matches the theme's painted ground (#150F08) so the frame is seamless.
Output target/screenshots/amber.webm
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
Set Theme { "name": "amber", "background": "#150F08", "foreground": "#FFB000", "cursor": "#150F08" }
Env COLORTERM "truecolor"
Hide
Type "target/release/cc-usage --demo --color truecolor --theme amber --range 7d"
Enter
Show
Wait+Screen /1 Daily/
Type "3"
Wait+Screen /Projects · [0-9]/
Sleep 2s
Screenshot tools/cc-usage/docs/amber.png
Sleep 2s
Type "q"
Sleep 500ms
