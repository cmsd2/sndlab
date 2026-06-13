# sndlab

A TUI sound-design environment for designing procedural audio in Rust.
Edit Rhai patches in a terminal editor, hear them play through Kira, and
expose the buffer over MCP so an AI agent can collaborate on the same
script the user is looking at.

## Status

Bootstrapping. The workspace compiles; the binary is a stub. Coming up,
in order:

1. TUI shell — ratatui + tui-textarea editor + log pane.
2. Rhai patch DSL — `sine`, `noise`, `env`, `gain`, `mix`, `tap`, `patch`.
3. Syntax highlighting via syntect.
4. Project model — directory of `.rhai` files + `project.ron` manifest.
5. Mix model — ambient layers + scene-arm + trigger.
6. MCP HTTP+SSE server with `get_buffer` / `apply_edit` / `play` tools.
7. Example project.

## License

MIT. See [LICENSE](LICENSE).
