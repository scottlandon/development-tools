# Security policy

## Reporting a vulnerability

Please **don't open a public issue**. Report it privately through GitHub:
<https://github.com/scottlandon/development-tools/security/advisories/new>

Include what you found, how to reproduce it, and which tool and version it affects. This is a
personal project, so I can't promise a timeline, but I'll acknowledge reports as soon as I can.

## Scope

The tools here run locally. `cc-usage` is read-only by design: it opens only `*.jsonl` transcripts (by default
under `~/.claude/projects`), writes no files, makes no network connections and runs no other programs.

Worth reporting:

- anything that breaks those promises: a file written, a socket opened, a program run, or a file read that
  isn't a transcript;
- transcript content (prompts, replies, tool output, keys) being kept, shown or logged, since only counts,
  model names, timestamps, project folder names and a fast-mode yes/no flag should survive parsing;
- a crash or unbounded resource use on a crafted transcript file.

Only the latest release is supported.
