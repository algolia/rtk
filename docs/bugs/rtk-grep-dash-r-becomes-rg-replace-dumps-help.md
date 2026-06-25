# RTK proxies `grep -r` through `rg`, where `-r` means `--replace` → ripgrep dumps `--help`

> ✅ **RESOLVED in 0.42.0-algolia.4** (verified 2026-06-25 against a fresh `main` build).
> The residual cause after the `-r`/`-E` strip was `-h`: `-rhoE` reduced to `-ho`, and rg reads `-h` as
> `--help` (GNU grep `-h` is `--no-filename`). rtk now strips the literal `-h` before ripgrep (`-rhoE` → `-o`)
> AND, in output-format mode, re-adds rg's own `--no-filename` to preserve the intent — so the reporter's
> `grep -rhoE "..." resources/views | sort | uniq -c` aggregates by match, not by `file:match`.
> Re-verified: bare matches, no filename prefix, no ripgrep help text.

**Date:** 2026-06-24
**Severity:** Medium (silently turns a recursive search into a no-op that prints ripgrep's help; easy to miss mid-pipeline, and the "output" looks like unrelated noise)
**Component:** command rewrite / hook (input side) — `grep` executed via `rg`, GNU flag not translated
**rtk version:** (run `rtk --version`; observed in current Algolia build, 2026-06-24)

## Summary

A GNU `grep` invocation using the combined short flag `-rhoE` (recursive +
no-filename + only-matching + extended-regex) is proxied through ripgrep. In GNU
`grep`, `-r` is **`--recursive`** and takes no argument. In ripgrep, `-r` is
**`--replace=TEXT`** and **consumes the next token as its replacement string**.

So `rg -rhoE "<pattern>"` is parsed as `-r hoE` (replacement = `hoE`) + `-E`,
leaving **no search pattern**. ripgrep, given no pattern and stdin/help conditions,
prints its **full `--help` text** instead of searching. The caller sees a wall of
ripgrep option docs where they expected match lines.

This is distinct from the existing reports:
- `rtk-proxies-grep-as-rg-breaks-bre-regex.md` — same direction (grep→rg) but the
  root cause there is **regex dialect** (BRE vs ERE). Here the pattern is fine; the
  **`-r` short flag has a different meaning** in the two tools.
- `rtk-rewrites-rg-to-grep-dropping-flags.md` — opposite direction (rg→grep).

## Observed

```
$ grep -rhoE "navbar-(default|inverse)|btn-(primary|info|success|warning|default)" resources/views | sort | uniq -c
      1   -0, --null                      Print a NUL byte after file paths.
      1   -A, --after-context=NUM         Show NUM lines after each match.
      1   -a, --text                      Search binary files as if they were text.
      ... (ripgrep's entire --help, piped through sort|uniq -c) ...
```

A sibling command in the **same** shell call, `grep -oE "\.btn-primary\{..."`
(no `-r`), was proxied correctly and returned the expected CSS match. Only the
`-r`-bearing invocation broke. That isolates the cause to the `-r` flag, not the
regex or the proxy in general.

## Expected

`grep -r PATTERN DIR` should recurse `DIR` and print matches. When proxied to `rg`,
GNU `grep -r` (recursive, no arg) must NOT map to `rg -r` (replace, takes arg).
Correct translation: drop `-r`/`-R`/`--recursive` entirely (ripgrep recurses by
default) and keep the remaining flags, so `grep -rhoE P D` → `rg -hoE P D` (or
`rg --no-filename --only-matching P D`). The combined-flag form `-rhoE` must be
decomposed before translating, not passed through verbatim.

## Workaround

Avoid `-r` when the rewrite is in play (ripgrep recurses by default), or split the
combined flags so `-r` isn't adjacent to others:

```bash
# works (no -r; the second grep in the failing session proved this path is fine):
grep -hoE "PATTERN" -r .            # still risky if -r passed through
# safest: call ripgrep directly with its own flag semantics
rg -hoN -e "PATTERN" resources/views | sort | uniq -c
# or force raw grep through the proxy escape hatch:
rtk proxy grep -rhoE "PATTERN" resources/views
```

## Minimal reproduction

```bash
mkdir -p /tmp/rgr/sub && printf 'btn-primary navbar-default\n' > /tmp/rgr/sub/a.txt
grep -rhoE "btn-(primary|info)|navbar-(default)" /tmp/rgr        # via rtk hook
# EXPECTED: lines "btn-primary" and "navbar-default"
# OBSERVED: ripgrep --help text (because -r ate "hoE" as the replacement, no pattern left)
```

## Suggested fix

In the grep→rg translation layer, normalize bundled short flags first, then map
GNU grep flags to their rg equivalents with a per-flag table. Critically:
`-r`/`-R`/`--recursive` → (omit; rg default). Never forward a bare `-r` to rg,
where it silently changes arity (`--replace` consumes the next argv).

## Additional occurrence — 2026-06-24 (combined `-rho`)

Reproduced again with the combined short flag `-rho` (recursive + no-filename +
only-matching), confirming the root cause is the `-r` short flag, independent of
the regex dialect. Same symptom: ripgrep dumped its full `--help`, piped through
`sort | uniq -c`, so the caller saw a wall of option docs instead of matches.

```
$ grep -rho -i "error\":[^,}]*\|terminal\|max retries\|..." DIR/*.jsonl | sort | uniq -c | head
      1   -z, --search-zip                Search in compressed files.
      1   -x, --line-regexp               Show matches surrounded by line boundaries.
      ... (ripgrep --help) ...
```

Here `-rho` → `rg -r ho` (replacement = `ho`) + `-i`, leaving no pattern → help.
Note the glob target (`DIR/*.jsonl`) and BRE `\|` alternation were also present,
but the `-r` arity bug fires first. Workaround used: scan the files with a Python
one-liner instead of grep, which sidesteps the proxy entirely. Reinforces the
suggested fix: decompose bundled short flags and drop `-r` before forwarding to rg.
