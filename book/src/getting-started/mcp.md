# Using sndlab with Claude Code

sndlab runs an MCP server on `http://127.0.0.1:7777/mcp` that exposes
its editor buffer and engine to any MCP client. The intended client is
[Claude Code](https://claude.com/claude-code): the agent reads and
edits the same file you're looking at; you press F5 to evaluate; you
tell the agent what to tweak. The iteration loop is ~1 second per turn.

## Register the server

While sndlab is running, in another terminal:

```sh
claude mcp add sndlab http://127.0.0.1:7777/mcp
```

That's a one-time setup. Subsequent sndlab launches reuse the same
registration as long as the port is free.

## Tools exposed

| Tool | Purpose |
|---|---|
| `get_buffer` | Return the current editor buffer (whatever the user is seeing). |
| `set_buffer` | Replace the whole buffer with new content. Use sparingly — prefer `apply_edit`. |
| `apply_edit` | Find `old_string` exactly once and replace with `new_string`. The buffer must contain exactly one match; if it doesn't, the edit returns an error so you can disambiguate. |
| `list_patches` | List all patches the most recent successful eval registered, with role and duration. |
| `eval` | Re-evaluate the buffer. Errors land in `last_error`. |
| `play` | Play a named registered patch. The user hears the audio; you don't. |
| `last_error` | Read the most recent eval/play error, or `"no error"`. |

All tools are synchronous from the MCP side; the actual engine work
runs on the eframe main thread within a single frame (~16 ms) after
the tool returns. `eval` and `play` are fire-and-forget — the tool
returns immediately and the result lands in the log + `last_error`.

## A typical iteration

1. The user is editing `patches.rhai` in sndlab; you can see them
   typing because every frame the editor pane updates with their
   keystrokes (and a `get_buffer` from you reflects the latest).
2. They say "make the ping a bit lower-pitched."
3. You call `get_buffer` to see the current code.
4. You call `apply_edit` with `old_string: "sine(330.0,"` and
   `new_string: "sine(280.0,"` to drop the pitch.
5. The next time the user presses F5 (or you call `eval` followed by
   `play`), they hear the change.
6. They tell you whether it's right.

## What the agent can't do

- **Hear the audio.** Audio plays out of the user's speakers; the MCP
  server has no way to capture it. You have to ask them how it sounded.
- **Open files.** The MCP surface is just the editor buffer — no
  filesystem access. File loading lands with the project model (task 9).
- **Force a save.** Even after `set_buffer` or `apply_edit`, the user
  controls when the file lands on disk.

## Troubleshooting

- The status bar at the bottom of sndlab shows the MCP endpoint URL.
  If it's missing, the server failed to bind — check the log pane for
  the error (usually port in use).
- Default port is 7777. If you need to change it, the port is
  currently hard-coded in `crates/sndlab/src/app.rs`; the next iteration
  exposes it via a CLI flag.
