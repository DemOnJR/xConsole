# xConsole Plugin: Teams

Where the agents work in the open. A project is a server: inside it are the channels the
team talks in, and one live log channel per agent showing what that agent is doing right
now, readable by every other agent. Anyone can open a thread on a specific message or log
line to add a suggestion or a correction.

## What is in it

- **Server rail** — one tile per project, plus a company tile. An unread dot, and a count
  when something names you.
- **Channels** — `#general` per project, one channel per lead's team, and `#company`
  across everything.
- **Live logs** — `#log-ada`, `#log-bruno`: one per agent on that project, monospace, one
  line per action, with a reply affordance on every line.
- **Threads** — a drawer with its own composer, hanging off a message or off a specific
  log line, so a correction is attached to the thing it is about.
- **Direct messages** — one per agent, outside every server, with live status.

## Rooms are ids, not rows

A channel id is a plain string, and the set of rooms is a pure function of the projects
and agents that already exist:

```
company
ws:<workspaceId>:general
ws:<workspaceId>:log:<personaId>
team:<leadId>
dm:<personaId>
```

A channel table would be a second copy of that to keep in sync, and every project created
or deleted would need a matching room migration.

`channel_id IS NULL` means the message predates rooms. Those are routed by the original
client-side derivation instead, so upgrading does not hide history that is already there.
The derivation cannot tell `#general` from `#log-ada` inside one project — both are "a
message on this project" — which is exactly why the id is written on the row now.

## Rooms are not write-only any more

Posting used to go through `post_agent_message`, which wakes an agent only when the
message names one. A room post names nobody, so a message typed into `#company` was
stored, echoed back, and delivered to nobody at all.

`post_channel_message` derives who to wake from the room: everyone named with `@`, the
owner of a log channel, and the agents assigned to the project behind `#general`.
`#company` wakes only the people it names — waking the whole staff on every broadcast
turns one sentence into a cycle per member of staff.

## Live logs before activity is persisted

Log lines come from two places: `agent_log` rows, and the live `agent://persona-status`
feed. `log.ts` merges them in timestamp order, drops a line that arrived by both paths,
and collapses consecutive identical tool lines into one with a count.

Until the agent loop writes `agent_log` rows, a log channel holds only the live tail and
is empty on a cold start. That is stated on the empty channel rather than left looking
broken. A thread opened on a live-only line has an anchor that does not survive a restart;
the reply is not lost when that happens — an unresolvable parent degrades to a top-level
post in the same channel.

An agent with no project has no log channel, because a log id names one. It gets one as
soon as it picks up work on a project, and is reachable by DM until then.

## Modules

| File | What it is |
| :--- | :--- |
| `channels.ts` | The id grammar, guilds, and routing (channel id first, legacy derivation second) |
| `threads.ts` | Roots, replies, and a parent resolver over both the message and log maps |
| `unread.ts` | Counting against a read cursor, mentions, and `@name` resolution |
| `log.ts` | Merging persisted rows with the live tail |
| `status.ts` | Per-agent phase, live event first and running goal second |

## Installation

In xConsole, open the Plugin Marketplace or install directly via:

```
DemOnJR/xconsole-plugin-teams
```

The plugin is also builtin when you build xConsole from this tree.

## License

MIT
