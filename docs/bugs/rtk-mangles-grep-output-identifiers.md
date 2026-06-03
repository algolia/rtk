# RTK mangles identifiers in command OUTPUT (grep/rg/sed of source code)

**Date:** 2026-06-01
**Severity:** High (corrupts code reading — leads to wrong conclusions about code)
**Component:** output filtering / token-compression applied to stdout

## Summary

When RTK proxies `grep`/`rg`/`sed` over **source code**, it rewrites/abbreviates
identifier tokens in the **output**, producing text that does not match the file on
disk. The compression intended for *commands* appears to be applied to *output that
must be verbatim* (code, symbol names, grep matches).

## Observed substitutions (anonymized)

| On disk (real) | RTK-filtered output |
|---|---|
| `reasoning` / `reasoning_effort` / `reasoning_tokens` | `n` / `n_tokens` |
| `SEARCH_TOOL_PREFIX = "algolia_search_index"` | `n = "algolia_search_index"` |
| `Retrofilled` (log string) | `n` / `l` |
| `platform_instructions` / `_build_platform_instructions` | `li` / `_build_lis` |
| `platform instructions` (docstring prose) | `lis` |
| `def load_compiled_rules(agent_id: str)` | `n(agent_id: str)` |
| `"rule_id": "bc6d1b28ee4fad2c"` | `"rule_id": "n"` |
| `"target_rule_id": "bc6d1b28ee4fad2c"` | `"target_rule_id": "n"` |

**Recurrence 2026-06-01 (pm):** still live. A hex rule-id literal
(`bc6d1b28ee4fad2c`) inside a JSON value was collapsed to `n` in `rg` output, and
a function name `load_compiled_rules` in a `def` line became `n(...)`. Confirms the
mangler hits **string-literal values and def-line names**, not just bare symbols —
and short hex/opaque ids are squashed to `n` wholesale (total information loss; the
id is unrecoverable from the filtered output).

The right-hand column is what was returned to the caller; the file actually contains
the left-hand column (verified via the editor's Read, which bypasses the shell).

## Impact

- A grep for a symbol returns a mangled name → caller believes the symbol is named
  `n`/`li`, searches for it, finds nothing, draws wrong conclusions.
- Reading a constant's definition (`FOO = "..."`) shows `n = "..."`, hiding the name.
- Forced a fallback to the editor Read tool for every code-symbol lookup this session.

## Reproduction (generic)

```
# any repo with a constant whose name contains a token RTK compresses
printf 'SEARCH_TOOL_PREFIX = "x"\nreasoning_tokens = 0\n' > /tmp/t.py
rtk proxy grep -n "PREFIX\|reasoning" /tmp/t.py     # raw — should be exact
grep -n "PREFIX\|reasoning" /tmp/t.py               # via hook — observed mangling
```

Expected: byte-for-byte the file content.
Actual: identifier tokens replaced with short codes (`n`, `li`, `l`, `lis`).

## Expected behavior

Output of read-only code inspection commands (`grep`, `rg`, `sed -n`, `cat` of
source) must be returned **verbatim**. Token-compression, if any, should apply only
to RTK's own command construction / non-code chatter — never to matched source lines
or symbol names.

## Workaround used

Routed all code-symbol lookups through the editor Read tool instead of shell grep.

---

## Recurrence 2026-06-02 (rtk 0.42.0-algolia.2)

Still live, four months later, on `rtk 0.42.0-algolia.2`. New observations that
narrow the trigger:

| On disk (real) | RTK-filtered output |
|---|---|
| `def generate_jwt_token(` / `generate_expired_jwt_token` | `def generate_n_token(` / `generate_expired_n_token` |
| `user_id: str` (param) | `n: str` |
| `"sub": user_id` (dict literal) | `"n": n` |
| `jwt.encode(` | `n.encode(` |
| `GuardrailService(` call args / method names | collapsed to `n` |

Notable: in the same output, the header string literal
`headers["X-Algolia-Secure-User-Token"] = user_token` came through **verbatim**,
while the surrounding function/param names were mangled to `n`. So the mangler is
selective per-token, not per-line — long hyphenated string literals survive, but
snake_case identifiers (and even the JSON key `"sub"`) are squashed.

**Invocation-independent:** the mangling hit identically whether invoked as a bare
`rg`/`grep` (hook-rewritten), as `command rg` (bypasses shell functions/aliases), or
as `$(which rg)` (absolute path to the real binary). This strongly implies the
corruption is in RTK's **stdout post-processing**, not in command rewriting — the
real binary's output is being filtered on the way back to the caller.

Also seen once: `grep -rn ... | head -3` printed
`[rtk] grep -rn ...: process terminated by signal 13` — the SIGPIPE from `head`
closing is surfaced as an rtk error line (cosmetic, but noisy; matches the
"never pipe rtk background output through head" guidance).

Same workaround: editor Read tool for all source/symbol inspection. `Read` is
unaffected (it bypasses the shell entirely).

---

## RESOLVED — 2026-06-03 (the report was RIGHT; first dismissed as a ghost)

This was initially marked not-reproducible because the repro used `rtk grep PATTERN`
without the `-rn` flags agents actually type. The mangling reproduces deterministically
the moment you add grep's recursive flag:

```
$ printf 'def foo():\n' > a.py
$ grep -rn def .          # hook → rtk grep -rn def .
./a.py:1:n foo():         # "def" silently rewritten to "n"
```

**Root cause:** grep's `-r`/`-R` means *recursive*; ripgrep's `-r` is **`--replace`,
which takes a value**. rtk forwards flags to rg, so a grep-style bundle like `-rn`
parses in rg as `--replace=n` — every match is rewritten to the bundle's trailing
letters. That is the whole table above:
`-rn` → replace with `n` (`reasoning`→`n`, `def`→`n`, `jwt`→`n`, `user_id`→`n`),
`-rli`/`-rl…` → replace with `li`/`l`/`lis` (`platform_instructions`→`li`). Long
hyphenated string literals "survived" only because the *match* (the snake_case symbol)
was what got replaced, not the surrounding text. It was never a tokenizer or a
context-rendering layer — it was `--replace`, hiding in plain sight.

The old typed clap interface accidentally masked it (it rejected `-rn` and fell back to
real grep); making `rtk grep` forward args verbatim (the rg-flag-collision fix)
*exposed* it for every `-rn`, which is why the audit saw "0 this window" yet field
agents kept hitting it.

**Fix:** `strip_grep_recursive()` in `src/cmds/system/grep_cmd.rs` removes grep's
`-r`/`-R` (and `--recursive`/`--dereference-recursive`) before forwarding to rg,
including from combined short bundles (`-rn` → `-n`, `-rln` → `-ln`) while preserving
value-taking flags and their values (`-A3`, `-tpy`, `-er`). rg recurses by default, so
dropping `-r` is always safe.

Verified: `grep -rn def .`, `grep -rn reasoning .`, `grep -rln jwt_token .` all return
verbatim content / correct file lists. Regression tests:
`test_strip_grep_recursive_*` in grep_cmd.rs.
