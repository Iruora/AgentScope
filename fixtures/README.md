# Fixtures

Recorded hook payloads and transcript samples, one folder per harness and
version, e.g. `claude-code/2.1/PreToolUse.json`. Adapter tests replay these so
a harness update that changes a payload shape shows up as a failing test.

- `claude-code/2.1/hooks/`: one payload per hook event, as Claude Code 2.1
  sends them on stdin.
- `claude-code/2.1/projects/`: a main transcript and one subagent transcript
  with its `.meta.json`, laid out as under `~/.claude/projects`.

Fixtures are written by hand from real payload shapes and contain no real
user data. Codex, Copilot CLI, and Cursor fixtures arrive in step 4.
