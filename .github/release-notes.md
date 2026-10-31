See every local AI coding agent, its subagents, and which ones are waiting for your approval. Works with Claude Code, Codex, Copilot CLI and Cursor.

**What's new in 1.4**

- **Usage and cost.** A new Usage page shows tokens and an estimated cost per day, harness, repo and model over 7, 30 or 90 days, using list prices bundled with the app. History rows and session pages show each session's estimate.
- **Stuck and loop alerts.** Cards and session pages flag a tool that has run for 10 minutes, the same command failing 3 times in a row, a session that went quiet for 15 minutes, and two agents editing the same file. Turn on notifications for them in Settings → Notifications.
- **Files changed.** A Files tab on each session lists what the agent wrote and shows the `git diff` of each file.
- **Labels.** Pin, rename, tag and add a note to any session. Pinned sessions are kept past the history limit, and tags become filters on Overview and History.
- **Export.** Save a session as Markdown or JSON from its page. Keys and tokens are redacted.
- **Mini widget.** A small always-on-top window with who needs you and every live agent. Turn it on from the tray menu or Settings → Quick access.

**New in 1.3**

- **Approve or deny from AgentScope.** Turn it on for Claude Code in Settings → Integrations, then answer permission prompts from the Approvals inbox, the tray menu, or a small popup. Anything you don't answer goes back to the terminal.
- **Phone push.** Settings → Phone push sends alerts through ntfy, Pushover, Discord, Slack or Telegram. Off by default.

**Also new since 1.0**

- **Updates in place.** AgentScope now updates itself from GitHub Releases. Coming from 1.0, install this version once by hand; later ones arrive inside the app.
- **Command palette.** Press Ctrl+K (⌘K on a Mac) to jump to any session, view or action. Ctrl/⌘+Enter goes straight to the agent.
- **Global shortcut.** Settings → Quick access can set a key that focuses the agent that has waited longest, from any app.
- **Diagnostics.** Settings → Diagnostics shows what each harness is sending, and copies a redacted debug report for bug reports.

**Downloads**

- macOS: the `.dmg`, or the `.app.zip` (`aarch64` for Apple silicon, `x64` for Intel Macs)
- Windows: the `-setup.exe` (or the `.msi`), or the `_portable.zip` to run without installing
- Linux: the `.AppImage`, `.deb` or `.rpm`

These builds are not code signed, so macOS and Windows ask once before the first launch. The [install guide](https://github.com/Iruora/AgentScope#install) shows the one-time step.
