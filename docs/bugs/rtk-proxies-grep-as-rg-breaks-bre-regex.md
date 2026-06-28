# RTK proxies `grep` through ripgrep, breaking BRE patterns (literal `(` → "unclosed group")

> 🔶 **KNOWN LIMITATION as of 0.42.0-algolia.4** (confirmed 2026-06-25 against a fresh `main` build).
> The hook rewrites BOTH `grep` and `rg` to `rtk grep`, so the handler is **dialect-blind** — it cannot
> tell whether the caller wrote a BRE `grep` pattern (literal `(`) or an ERE `rg` pattern (group `(`).
> A BRE→ERE translator would therefore be a *guess* that silently corrupts genuine `rg` patterns — the
> exact silent-reinterpretation hazard this report warns about — so we deliberately do NOT translate.
> (The existing `\|`→`|` alternation rewrite shares this hazard for `rg 'a\|b'`; noted, not expanded.)
> **Workaround:** `grep -F 'rpc('` for fixed strings, or `rtk proxy grep …` for raw grep.
> **Proper fix** (route by source-tool identity so grep speaks BRE and rg speaks ERE) is tracked to land
> with the next upstream catchup, in the rewrite/permission layer upstream just hardened.

**Date:** 2026-06-10
**Severity:** Medium (silently changes regex dialect; valid `grep` patterns error out or, worse, could match differently without erroring)
**Component:** command rewrite / hook (input side) — `grep` executed via `rg`
**rtk version:** 0.42.0-algolia.2

## Summary

A plain GNU `grep` invocation is proxied through ripgrep (`rg`). GNU `grep`
defaults to **BRE** (Basic Regular Expressions), where `(`, `)`, `{`, `}` are
**literal** characters. ripgrep uses **ERE/PCRE-style** syntax, where `(` opens a
group. So a pattern that is valid and literal under `grep` becomes a regex
**syntax error** under `rg`.

This is distinct from the existing reports:
- `rtk-rewrites-rg-to-grep-dropping-flags.md` is the **opposite direction**
  (`rg` → `grep`, dropping rg flags).
- `rtk-mangles-grep-output-identifiers.md` is about **output** mangling.
This one is **input-side `grep` → `rg`, changing regex dialect**.

## Observed

Command run:
```
grep -oc 'rpc(' live.js
grep -oc 'validate_billet' live.js
```

Output (note the ripgrep error and the `(?:rpc()` rewrite — proof rg executed):
```
rpc calls:           live.js:5            # rg parse error for this one:
rg: regex parse error:
    (?:rpc()
    ^
error: unclosed group
validate_billet:     live.js:1
```

`validate_billet` (no parens) matched fine via `rg`; `rpc(` (literal paren under
BRE) blew up because `rg` parsed `(` as a group opener.

## Expected

GNU `grep -oc 'rpc(' live.js` treats `(` as a literal and counts lines/matches
containing the substring `rpc(`. No regex error. Proxying must preserve `grep`'s
BRE semantics (or pass `-F`/translate the pattern) rather than feed a BRE pattern
to ripgrep's ERE engine.

## Why this is dangerous beyond the error

The error case is the *lucky* one — it fails loudly. The silent-corruption risk:
a BRE pattern using `{`, `}`, `|`, `+`, `?` literally would be **silently
reinterpreted** as ERE quantifiers/alternation by ripgrep, returning a different
match set with **no error at all**, leading to wrong conclusions.

## Workaround (found)

Use fixed-string matching, which is dialect-independent:
```
grep -oFc 'rpc(' live.js      # -F = literal, worked correctly
```
Or escape per the *target* engine — but that's fragile since the caller doesn't
know rg will run.

## Minimal anonymized reproduction

```
printf 'a.rpc(x)\nb.rpc(y)\nz\n' > /tmp/t.txt
grep -c 'rpc(' /tmp/t.txt     # GNU grep: prints 2
                              # under RTK: "rg: regex parse error ... unclosed group"
```

---

## Occurrence 2 — 2026-06-23 — grep-only flag `--include` passed verbatim to `rg`

**rtk version:** (same proxy path; grep → rg)

Same root cause (a `grep` invocation proxied through `rg`), different facet:
grep-specific **flags** are forwarded untranslated. `grep --include=GLOB` (the
GNU recursive-filter flag) has no `rg` equivalent spelling — `rg` uses
`-g/--glob` — so `rg` rejects it outright.

### Observed
Command run:
```
grep -rn '<pattern>' --include='*.py' --include='*.md' --include='*.toml' --include='*.sh' .
```
Output (proof rg executed — it suggests an rg-only flag):
```
rg: unrecognized flag --include

similar flags that are available: --include-zero
```

### Expected
GNU `grep -rn 'pat' --include='*.py' .` recursively greps only `*.py` files. The
proxy must translate `--include=GLOB` → `rg -g 'GLOB'` (and `--exclude` →
`-g '!GLOB'`), or fall through to real `grep`, rather than hand a grep-only flag
to ripgrep.

### Workaround (found)
Spell it in ripgrep's dialect directly:
```
rg -n 'pat' -g '*.py' -g '*.md' .     # works
```

### Why it matters
Less dangerous than the BRE case (fails loudly, no silent corruption), but it
breaks a *correct, extremely common* `grep` invocation — scanning a tree for a
substring filtered by extension. The `--include`/`--exclude` → `-g`/`-g '!'`
translation belongs in the same flag-mapping layer that should be handling the
regex-dialect issue above.

## Occurrence 2026-06-25 (rtk 0.42.0-algolia.2)

Command issued (plain `grep` with BRE alternation):
```
grep -n "onMapClick\|onClick\|onMove\|interactiveLayer\|Props = {\|onReady\|onSelectPlan" components/MapCanvas.tsx
```
RTK rewrote `grep` → `rg`, which parses the pattern as RE2. The literal `{` in
`Props = {` (valid/literal in grep BRE) became an invalid repetition quantifier:
```
rg: regex parse error:
    (?:onMapClick|onClick|...|Props = {|onReady|onSelectPlan)
                                       ^
error: repetition quantifier expects a valid decimal
0 matches ...
```
Expected: BRE semantics (literal `{`, `\|` alternation) since the user typed `grep`.
Workaround: re-issued with `grep -E` and no brace (`type Props`), which RTK maps to
`rg` cleanly. Same root cause as this report: silently swapping `grep`→`rg` changes
the regex dialect, so BRE-valid patterns (`\|`, bare `{`, `\{n\}`) error or mismatch.

---

## CONFIRMED 2026-06-28 — the mirror hazard fires: genuine `rg '\|'` over-matches (rtk 0.42.0-algolia.4)

The header note warned that the `\|`→`|` alternation rewrite "shares this hazard
for `rg 'a\|b'`". It is no longer hypothetical — **measured against a fresh
`main` build** (`target/release/rtk`), bypassing the hook for the control with
`RTK_DISABLED=1`:

```
$ printf 'foo\nbar\nfoo|bar\n' > disc.txt
$ RTK_DISABLED=1 rg -c 'foo\|bar' disc.txt     # genuine ripgrep
1                                              # \| is a LITERAL pipe → matches only "foo|bar"
$ rtk grep -c 'foo\|bar' disc.txt              # hook target
3                                              # \| rewritten to | → ALTERNATION → matches all 3
```

This is **silent** (exit 0, plausible count) and **direction-symmetric** with
the grep BRE bug above: the handler (`src/cmds/system/grep_cmd.rs:83`) rewrites
`\|`→`|` on the positional pattern unconditionally — correct for a `grep` caller
(BRE: `\|` = alternation) and **wrong for an `rg` caller** (RE2: `\|` = literal
pipe). One handler cannot be right for both because it has lost the source-tool
identity: the hook collapses `grep` and `rg` to `rtk grep`
(`src/discover/rules.rs:92-94`, **identical upstream**). Same keystone as
`rtk-strips-genuine-rg-replace-encoding-short-flags.md`.

**Proper fix:** route by source-tool identity (the deferred fix this whole
cluster points to). With identity known, `rtk grep` translates BRE and `rtk rg`
forwards RE2 verbatim — both correct, no guessing.
