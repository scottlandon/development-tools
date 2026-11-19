# Live view (5), Mainframe, default 1 hour window  ->  docs/live.png
# Regenerate every screenshot with scripts/screenshots.sh, run from the repo root.
Output target/screenshots/live.webm
Set Shell "bash"
Set Width 1138
Set Height 532
Set FontSize 14
Set FontFamily "Menlo"
Set LetterSpacing 0
Set LineHeight 1.2
Set Padding 10
Set Margin 0
Set BorderRadius 14
Set Theme { "name": "mainframe", "background": "#000000", "foreground": "#ffffff", "cursor": "#000000" }
Env COLORTERM "truecolor"
Hide
Type "target/release/cc-usage --demo --color truecolor --theme mainframe"
Enter
Show
Wait+Screen /1 Daily/
Type "5"
Wait+Screen /Tokens \/ min/
Sleep 2s
Screenshot tools/cc-usage/docs/live.png
Sleep 2s
Type "q"
Sleep 500ms
