# Contributing

sndlab is a small project. The contribution shape that matters most is
**keeping the book in lockstep with the code**: every new DSL primitive
needs a chapter, every new manifest field needs documentation, every
new MCP tool needs a description.

See [CLAUDE.md](https://github.com/cmsd2/sndlab/blob/master/CLAUDE.md)
in the repository root for the discipline the project follows when AI
assistance is involved. Human contributors are asked to follow the
same discipline.

## Building

```sh
cargo build                  # compile the workspace
cargo run -p sndlab          # launch the TUI
cargo test --workspace       # run all tests
mdbook serve book            # preview this book at http://localhost:3000
```

## Code style

- No emojis in code or docs unless asked.
- Comments explain *why* — the *what* should be evident from the code.
- New primitives include both the implementation and the book chapter
  in the same commit.

## License

MIT. By contributing, you agree your contribution is licensed under the
same terms.
