# RTK strips genuine ripgrep short flags (`-r` replace, `-E` encoding), corrupting real `rg` invocations

> ✅ **RESOLVED (unreleased, commit `cba316f`)** — `rg` now routes to a native `rtk rg`
> handler that forwards every argument verbatim, so `-r` (`--replace`) and `-E` (`--encoding`)
> reach ripgrep with their genuine meaning. Re-verified by the differential scoreboard row
> `rg -r REPL world rep.txt` (was CORRUPT "No such file", now COMPACTED, faithful to truth).
> See `tests/grep_rg_scoreboard.rs`.
>
> 🔶 *(original report, discovered 2026-06-28 against a fresh `main` build)*
> The `rtk grep` handler assumed every caller is GNU `grep` and stripped `-r`/`-R`/`-E`/`-h`
> before forwarding to ripgrep — correct for `grep`, but these are *valid, differently-meaning*
> flags when the caller actually typed `rg`. Root cause: the hook collapsed `grep` and `rg` to
> a single dialect-blind `rtk grep`. Fixed by splitting the registry rule and threading a
> `Dialect` so the `rg` path skips all grep-isms.

**Date:** 2026-06-28
**Severity:** Medium (genuine `rg --replace`/`--encoding` short forms break; `-r` fails loudly here, but the class includes silent mis-search)
**Component:** output/arg handling — `src/cmds/system/grep_cmd.rs` `strip_grep_only_flags` (input side)
**rtk version:** 0.42.0-algolia.4

## Summary

The grep-compat layer (`strip_grep_only_flags`, `grep_cmd.rs:320-347`) drops the
short flags `-r`/`-R` (grep `--recursive`), `-E` (grep ERE), and `-h` (grep
`--no-filename`) before forwarding the argv to ripgrep, because ripgrep reuses
those letters for *value-taking* flags that would eat the pattern. That reasoning
is sound **only when the caller typed `grep`.** When the caller typed `rg` — which
the hook also rewrites to `rtk grep` — those same letters are the user's real
ripgrep flags, and stripping them changes or breaks the command:

| rg short flag | ripgrep meaning        | rtk strips it → effect                         |
|---------------|------------------------|------------------------------------------------|
| `-r TEXT`     | `--replace=TEXT`       | replacement dropped; `TEXT` mis-parsed as path |
| `-E ENC`      | `--encoding=ENC`       | encoding dropped; wrong-encoding decode/no-op  |
| `-h`          | `--help`               | help suppressed (minor)                        |

Asymmetry that proves the point: the **long** forms survive (`--replace`,
`--encoding` are not in the strip list), so `rg --replace X p f` works while
`rg -r X p f` breaks — same flag, opposite outcome based purely on spelling.

## Observed (fresh `main` build; `RTK_DISABLED=1` bypasses the hook for the control)

```
$ printf 'hello world\n' > rep.txt

$ RTK_DISABLED=1 rg -r GOODBYE world rep.txt      # genuine ripgrep --replace
hello GOODBYE

$ rtk grep -r GOODBYE world rep.txt               # hook target
rg: world: No such file or directory (os error 2)
```

`-r` is stripped, so `GOODBYE` is no longer the replacement; the surviving argv
`GOODBYE world rep.txt` makes ripgrep read `world` as a path → error. The
long-form control confirms it is the strip, not ripgrep:

```
$ rtk grep --replace GOODBYE world rep.txt        # long form NOT stripped
rep.txt:1:hello GOODBYE                            # works
```

## Expected

A genuine `rg -r/-E/-h` invocation must run ripgrep with those flags intact.
`grep` callers still need them stripped (rg reuses the letters). Both are only
achievable once the handler knows which tool the user invoked.

## Why this is the same bug as the BRE/`\|` reports

Every report in this cluster is one root cause: the hook maps both `grep` and
`rg` to `rtk grep` (`src/discover/rules.rs:92-94`), discarding the source-tool
identity at `src/discover/registry.rs:832`. The handler then guesses a single
dialect, and whichever tool it *didn't* assume gets corrupted:

- assume grep → genuine `rg` patterns/flags corrupted (this report; `\|` over-match)
- assume rg   → genuine `grep` BRE patterns/flags corrupted (the documented grep reports)

There is no correct single-dialect guess. The fix is to stop guessing.

## Workaround

- Long flags instead of short: `rg --replace TEXT` (not `-r`), `rg --encoding ENC` (not `-E`).
- Or bypass the filter: `rtk proxy rg -r TEXT pattern file`, or `RTK_DISABLED=1 rg ...`.

## Minimal reproduction

```bash
printf 'hello world\n' > /tmp/rep.txt
RTK_DISABLED=1 rg -r GOODBYE world /tmp/rep.txt   # → "hello GOODBYE"
rtk grep      -r GOODBYE world /tmp/rep.txt        # → "rg: world: No such file or directory"
```

## Proper fix (tracked)

Route by source-tool identity so `rtk grep` (GNU grep dialect) and `rtk rg`
(ripgrep RE2 dialect) are distinct: split the registry rule
(`^grep\s+` → `rtk grep`, `^rg\s+` → `rtk rg`), thread the dialect into
`grep_cmd::run`, and skip the grep-isms (`-r`/`-E`/`-h` strip, `\|`→`|`) on the
`rg` path. The root cause is **identical upstream** (`rtk-ai/rtk`), so this is a
backport candidate. Lands with the next upstream catchup unless prioritized.
