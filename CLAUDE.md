@AGENTS.md

## Claude Code specifics

- `/clear` can only be run by the user. When directive 2 in AGENTS.md says stop, make the checkpoint line the last thing in your reply, then stop. When it says continue, note in one line that the WP is done, then start the next one.
- The prompt cache has a 1-hour TTL here (5 minutes once the plan is in usage overage). The numbers in AGENTS.md assume Opus 5.5 (0.05× cache reads). With 0.1× reads, as on most other models, the ratio is 20, so halve the thresholds.
- Switching `/model` or `/fast` mid-session re-writes the whole context at the write price, because caches are per model and per speed; an effort change may too. Switch right after a `/clear`.
- Prefer `/clear` with `docs/STATUS.md` as the handoff over `/compact`: a compact summary is billed as output (5×), and STATUS.md is the reviewed, versioned handoff.
- Present options with AskUserQuestion: 2–4 choices, recommended one first, labelled "(Recommended)".
- Read big docs one section at a time: `grep -n '^## ' <file>`, then Read with offset and limit.
- After pushing, follow CI with `gh run watch`. On failure, use `gh run view --log-failed`.
