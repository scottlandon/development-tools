# Daily view (1), Mainframe (default theme), 7 days, by cost  ->  docs/daily.png
# Regenerate every screenshot with scripts/screenshots.sh, run from the repo root.
# 1138 px wide (vhs needs an even width for rounded corners); 13 pt gives ~142 columns,
# enough for the wide layout with the Models / Projects / Token mix sidebar.
Output target/screenshots/daily.webm
Set Shell "bash"
Set Width 1138
Set Height 712
Set FontSize 13
Set FontFamily "Menlo"
Set LetterSpacing 0
Set LineHeight 1.3
Set Padding 10
Set Margin 0
Set BorderRadius 14
Set Theme { "name": "mainframe", "background": "#000000", "foreground": "#ffffff", "cursor": "#000000" }
Env COLORTERM "truecolor"
Hide
Type "target/release/cc-usage --demo --color truecolor --theme mainframe --range 7d"
Enter
Show
Wait+Screen /Daily · 7 days/
Sleep 2s
Screenshot tools/cc-usage/docs/daily.png
Sleep 2s
Type "q"
Sleep 500ms
