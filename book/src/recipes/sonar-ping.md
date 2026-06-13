# Sonar ping

The classic submarine "ping": a low-pitched sine that rings out into a
field of decaying echoes.

> Code listing pending the DSL primitives shipping (task 7). The
> finished form will be something like:
>
> ```rhai
> patch("sonar_ping", "one_shot", || {
>     let body = sine(330.0, 3.5).env(0.008, 1.4).gain(0.32);
>     body.with_taps([
>         tap(0.13, 0.55),
>         tap(0.31, 0.38),
>         tap(0.58, 0.26),
>         tap(0.95, 0.17),
>         tap(1.45, 0.10),
>         tap(2.05, 0.06),
>     ])
> });
> ```
>
> The reasoning behind each number — frequency choice, attack length,
> tap timing — will be explained inline as the recipe lands.
