# CLAUDE.md

Instructions for Claude (or any LLM-driven contributor) working on
sndlab. Mirror the same discipline if you're a human contributor.

## What this project is

sndlab is a TUI sound-design environment + reusable Rust audio engine.
See [README.md](./README.md) for the user-facing introduction and
[book/src/SUMMARY.md](./book/src/SUMMARY.md) for the structure of the
DSL reference.

## Workspace map

```
crates/sndlab-core/   library:  rhai engine + kira/fundsp playback
crates/sndlab/        binary:   TUI shell + MCP server + project model
book/                 mdBook:   user guide and DSL reference
```

When in doubt, **engine logic goes in `sndlab-core`**, **UI / project /
MCP / file I/O goes in `sndlab`**. If a piece of code would be useful
to a non-TUI consumer (a game, a batch renderer, a test harness),
that's a sign it belongs in `sndlab-core`.

## The discipline: scripting surface changes

When you add, change, or remove anything that's visible from a Rhai
patch — a primitive, a parameter, a default, a name — you **must**
update three places in the same commit:

1. **The implementation** in `crates/sndlab-core/`.
2. **The DSL chapter** in `book/src/dsl/<name>.md` (create it if it
   doesn't exist). Include the full signature, semantics, error cases,
   and at least one example.
3. **The status table** in two places:
   - `book/src/dsl/overview.md` — the table at the top.
   - `README.md` — the "DSL at a glance" table.

If a primitive's status flips from `not yet implemented` / `planned`
to `shipped`, both tables must reflect that. If the signature changes,
both tables and the chapter need to match.

The reason this matters: the book is the contract for what patches
*can do*. AI agents using sndlab over MCP only have the book and the
patch file in their context — if the book and the code disagree, the
agent will generate code against the book, the engine will reject it,
and the user will think the agent is broken. The book is the source
of truth that has to stay in sync.

### When to update the `book/src/concepts/` chapters

Less frequently. Concept chapters (patches, buffers, roles, mix graph)
describe model-level decisions and only change when the underlying
*model* changes. Don't touch them for routine primitive additions.

### When to update `book/src/getting-started/`

When the user-facing setup story changes — new system dep, a new
hotkey added to the TUI, a change to how MCP is registered.

### When to update `book/src/recipes/`

When you ship a new example patch worth documenting, or when an
existing recipe's code listing changes because the DSL did.

## Style notes

- **No emojis.** Not in code, not in comments, not in commit messages,
  not in book chapters.
- **Comments explain *why*.** Don't restate what the code does. Don't
  reference the current task or commit (those belong in the PR
  description). One short line is usually enough; multi-line blocks
  are rare.
- **Prefer editing existing files** to creating new ones, except for
  the cases this document explicitly calls out (new DSL primitive →
  new chapter).
- **No speculative abstractions.** Don't design for hypothetical
  future primitives. Don't add feature flags. Don't write
  backwards-compatibility shims; sndlab is pre-1.0 and we change the
  DSL freely.
- **Don't claim a feature works without trying it.** Type checking
  and `cargo build` confirm code compiles; they don't confirm a sound
  is audible. If you can't actually hear the result (e.g. because
  you're running headless), say so explicitly rather than asserting
  success.

## Sound design loop discipline

When using sndlab itself to design sounds (the AI-collaborates-via-MCP
case):

- **Make small edits.** Prefer `apply_edit(old_string, new_string)`
  over `set_buffer(content)` — it's easier for the user to track what
  changed, and the editor preserves their cursor.
- **One change at a time per turn.** Wait for feedback before the next
  edit. The user can't usefully critique three different revisions in
  one go.
- **Quote the parameter you changed.** "Lowered freq from 440 → 330 Hz
  and added a fourth tap at 0.95 s" is much more useful than "tweaked
  the ping".
- **Don't auto-save.** The user controls when the file lands on disk.

## Commit hygiene

- One logical change per commit. A primitive's implementation + its
  chapter + its status-table flip are all *one* logical change.
- Imperative commit subject: `dsl: add sine primitive`, not
  `Added sine primitive`.
- Include a `Co-Authored-By:` trailer when AI-assisted.
- Never amend a commit that's been pushed.
- Never `--force` push to `master`.

## Areas this file does *not* cover

- Detailed engine implementation choices — those live in code comments
  at the relevant `crates/sndlab-core/src/*.rs` site.
- DSL semantics — those live in `book/src/dsl/<name>.md`.
- Roadmap — see `book/src/SUMMARY.md` for the planned shape; see the
  current task list when actively working.
