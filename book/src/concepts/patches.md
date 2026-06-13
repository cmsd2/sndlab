# Patches

A **patch** is a named, procedurally-generated sound. It is the unit of
work in sndlab. You write a patch by calling the `patch` function with:

- A **name** — used for triggering and for cross-references between
  patches in the same project.
- A **role** — either `"one_shot"` (plays once, dies) or `"ambient"`
  (loops continuously until stopped). See [Roles](./roles.md).
- A **body** — a Rhai closure that returns a signal expression.

```rhai
patch("name", "one_shot", || {
    sine(440.0, 1.0)
});
```

The closure runs once at *evaluation* time. Whatever it returns becomes
the signal graph that's rendered to a buffer when the patch is played.
Patches are deterministic — same script, same buffer — which is what
makes them safe to bake into a game build.

## What's in a body

Rhai is a real scripting language with variables, conditionals, loops
and arithmetic. The body of a patch is just code; you can compute
parameters, sweep frequencies in a `for` loop, decide envelope shapes
based on flags. The DSL primitives ([sine](../dsl/sine.md),
[noise](../dsl/noise.md), [env](../dsl/env.md), …) are the operators
the body uses to *describe* the sound; everything else around them is
ordinary scripting.

## Naming

Patch names are global within a project. Two patches with the same name
collide; the second one wins, with a warning in the log. Conventional
shape:

- Snake case: `sonar_ping`, `hull_creak`, `merchant_screw`.
- Domain-grouped: `ui_click`, `weapon_torpedo_launch`, `ambient_ocean`.
- Avoid generic names like `test` or `default` — the more patches you
  add to a project, the more ambiguous the log gets.
