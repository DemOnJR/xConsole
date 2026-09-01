# xConsole Plugin: Teams

Live view of named agents in xConsole: what each one is doing, the task they are on, and the conversation they have with each other.

## Features

- One list per project team, plus a company-wide group
- Per-person live status (thinking, working, waiting, verifying) from the tool that is actually running
- Team chat: what they say to each other, plus messages you send into the thread
- Click a person to see their current task and recent messages

Status is the same event the canvas agent uses (`agent://persona-status`). The canvas status line also shows the running tool instead of a rotating verb.

## Installation

In xConsole, open the Plugin Marketplace or install directly via:

```
DemOnJR/xconsole-plugin-teams
```

The plugin is also builtin when you build xConsole from this tree.

## License

MIT
