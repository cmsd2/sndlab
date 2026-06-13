# Roles: ambient vs one-shot

Every patch has a **role** that tells the engine what to do with it.

## `"one_shot"`

A finite sound that plays once and ends. Examples: a sonar ping, a UI
click, a weapon launch, a depth-charge detonation.

- Triggered explicitly — by a keypress in the TUI, by the
  `play(name)` call, by an MCP `play` tool call, or by an event in a
  hosting application.
- The engine renders the patch to a buffer, hands it to Kira, and
  forgets about it.
- Multiple one-shots can play concurrently; the mixer handles polyphony.

## `"ambient"`

A continuous sound that loops while the project is open or while the
hosting application says so. Examples: ocean rumble, machinery hum,
periscope drone.

- Started automatically on project load (or on explicit `play_ambient`
  call).
- The engine loops the rendered buffer until told to stop.
- Modulation parameters (e.g. depth → low-pass cutoff) are applied at
  loop time, not at render time. This is what lets ambient patches
  respond to game state.

> The full modulation API ships with the mix model work (task 10). For
> now, ambient patches play at a fixed mix level.

## Choosing a role

A useful heuristic: if the sound has a natural end (envelope falls to
silence, decay tails out), it's a one-shot. If you'd describe its
behaviour as "stays on while X is true," it's ambient.
