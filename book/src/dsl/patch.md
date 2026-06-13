# `patch`

Register a named patch with the engine. The script's only side effect.

## Signature

```rhai
patch(name: string, role: string, signal: Signal)
```

- **`name`** — unique identifier within the project. Re-registering
  the same name overwrites the previous value and emits a `warn`-level
  message in the eval summary.
- **`role`** — one of `"one_shot"` or `"ambient"`. See
  [Roles](../concepts/roles.md).
- **`signal`** — a `Signal` value built from the rest of the DSL.

`patch` has no return value. It runs for its side effect: adding the
signal to the engine's patch table under `name`.

## Example

```rhai
patch("sonar_ping", "one_shot",
    sine(330.0, 3.5).env(0.008, 1.4).gain(0.32));
```

After this script evaluates, the host can `play("sonar_ping")` to hear
the buffer.

## Errors

- `patch: unknown role 'xyz' — expected 'one_shot' or 'ambient'` — the
  role string didn't match a known role.

## Notes

- **Eager rendering.** The `signal` argument is fully rendered to a
  buffer before `patch` returns. Long patches make eval slower; very
  long patches (multi-minute) are better authored as ambient loops.
- **Insertion order.** The order in which patches are registered is
  preserved and reported back in `EvalSummary.patches`. The TUI uses
  this for the patch list and for the "play first patch on Ctrl+R"
  shortcut.
