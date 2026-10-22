@AGENTS.md

## Claude Code specifics

- `/clear` can only be run by the user. At a checkpoint (directive 2 in AGENTS.md), make the checkpoint line the last thing in your reply, then stop.
- Present options with AskUserQuestion: 2–4 choices, recommended one first, labelled "(Recommended)".
- Read big docs one section at a time: `grep -n '^## ' <file>`, then Read with offset and limit.
- After pushing, follow CI with `gh run watch`. On failure, use `gh run view --log-failed`.
