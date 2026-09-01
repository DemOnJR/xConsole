# xConsole Plugin: Teams

Live view of named agents in xConsole: Slack-style channels per project and reporting team, who said what, and what each agent is on.

## Features

- Channels: `#company`, one `#project` per workspace, one team channel per lead (even when agents were hired company-wide)
- Direct messages: one thread per agent, with initials so the feed is not an anonymous list
- Per-person live status (thinking, working, waiting, verifying) from the tool that is actually running
- Click a member to open their DM

Status is the same event the canvas agent uses (`agent://persona-status`). The canvas status line also shows the running tool instead of a rotating verb.

## Installation

In xConsole, open the Plugin Marketplace or install directly via:

```
DemOnJR/xconsole-plugin-teams
```

The plugin is also builtin when you build xConsole from this tree.

## License

MIT
