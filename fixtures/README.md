# Fixtures

Recorded hook payloads and transcript samples, one folder per harness and
version, e.g. `claude-code/2.1/PreToolUse.json`. Adapter tests replay these so
a harness update that changes a payload shape shows up as a failing test.

- `claude-code/2.1/hooks/`: one payload per hook event, as Claude Code 2.1
  sends them on stdin.
- `claude-code/2.1/projects/`: a main transcript and one subagent transcript
  with its `.meta.json`, laid out as under `~/.claude/projects`.

- `codex/0.165/hooks/` and `codex/0.165/sessions/`: hook payloads, and a
  main and a subagent rollout laid out as under `~/.codex/sessions`.
- `copilot-cli/1.0/hooks/` and `copilot-cli/1.0/session-state/`: camelCase
  hook payloads, and an `events.jsonl` with its `workspace.yaml`.
- `cursor/hooks-v1/hooks/`: hook payloads for `hooks.json` version 1.

Fixtures are written by hand from the harnesses' documented payload shapes
(and, for Codex, its JSON schemas) and contain no real user data.
