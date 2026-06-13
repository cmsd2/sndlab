# Using sndlab with Claude Code

sndlab can run an MCP server that exposes its editor buffer to an AI
agent — most usefully, [Claude Code](https://claude.com/claude-code).
The agent reads and edits the same file you're looking at; you press
`Ctrl+R` to evaluate and hear the result; you tell the agent what to
tweak.

> Pending the MCP server work (task 11). When the server ships, this
> chapter covers:
>
> - Registering sndlab as an MCP server in Claude Code
>   (`claude mcp add sndlab http://127.0.0.1:<port>/mcp`).
> - The tools exposed: `get_buffer`, `apply_edit`, `list_patches`,
>   `play`, `last_error`.
> - The typical iteration loop and where the failure modes live.
