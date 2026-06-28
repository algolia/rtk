# RTK mangles identifiers in command OUTPUT (grep/rg/sed of source code)

> ✅ **RESOLVED in 0.42.0-algolia.3** (verified 2026-06-25 against a fresh `main` build).
> Root cause was `grep -rn`/`-nE` reaching ripgrep where `-r`/`-E` are the value-taking `--replace`/`--encoding`,
> silently rewriting matches (`def foo` → `n foo`). Those flags are now stripped before rg (commit `d775f56`).
> Re-verified: `grep -n "PREFIX\|reasoning" t.py` returns `SEARCH_TOOL_PREFIX`/`reasoning_tokens` intact, no `n`/`li`.

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

---

## Recurrence 2026-06-04 — survives on `rtk 0.42.0-algolia.2` via `rg -rl -i`

The documented fix (`strip_grep_recursive` in `grep_cmd.rs`) was verified for
`grep -rn/-rln`, but the `--replace` mangling still fires when the **caller invokes
`rg` directly with `-rl`** (not `grep`). On `rtk 0.42.0-algolia.2`:

- **Command:** `rg -rl -i "external eval|pushback|enablers.*judge|judge.*controlled" <dirs>`
- **Observed (mangled):** matched lines rendered with the matched token collapsed to
  `l` — e.g. `ENABLERS_TOKEN | Judge LLM` → `l LLM`, `get_enablers_judge` → `get_l`,
  `avoids flattery.` → `avoids l.`. The leftover `l` is the `-rl` replacement value,
  same mechanism as the `-rn`→`n` case already documented.
- **Expected:** verbatim file content / file list, no token substitution.
- **Confirmed real (not display):** raw `python3` read of the same files shows the
  intact `ENABLERS_TOKEN`, `Judge LLM`, `get_enablers_judge` — so it is RTK's output
  layer replacing matched substrings, not the files.
- **Workaround:** bypass rtk for grep-output-bearing searches — `python3` line scan, or
  `rtk proxy rg ...`.

Root-cause note: the `-r` strip / `--replace` guard needs to cover the **`rg` entrypoint
with combined short bundles containing `l`** (`-rl`, `-rli`, `-rl -i`), not only the
`grep`→rg rewrite path. The match-replacement still leaks for direct `rg` calls.

---

## Recurrence 2026-06-25 — `rg -rln` collapses matches to `ln` (still unpatched for direct `rg`)

On `rtk 0.42.0-algolia.2`, a direct `rg` call with a `-rln` bundle still mangles:

- **Command:** `rg -rln 'popup-chip|mk-transit|--transit' --glob '*.css' --glob '*.scss' .`
  (intent: list CSS files containing any of those tokens)
- **Observed (mangled):** every matched token replaced by `ln` — `--transit` → `ln`,
  the `--transit:` CSS custom-property line came back as `ln:`, `.transit {` as `.ln {`,
  `var(--transit)` as `var(ln)`, and the `.popup-chip`/`.transit-lines` class names were
  dropped from the listing entirely.
- **Expected:** a list of matching file paths (that's what `-l` does), verbatim.
- **Confirmed real (not display):** the immediate re-run `grep -n -- '--transit' app/globals.css`
  (no `-r`, so no bundle collision) returned the intact `--transit: #5a3fb5;` and
  `background: var(--transit);` — so the file is fine; RTK's `--replace=ln` ate the output.
- **Workaround:** drop the `-r` (rg recurses by default) — `rg -ln 'pat' --glob '*.css'`
  is unaffected; or use `grep -n` without recursive flags; or editor Read.

Same mechanism as the 2026-06-04 entry: `strip_grep_recursive()` fixed the `grep`→rg
path, but a **caller typing `rg -rln` directly** still has `-r` parsed by rg as
`--replace`, with the bundle's trailing `ln` as the replacement value. The guard must
also strip `-r` from short bundles on the **direct `rg` entrypoint** (`-rln` → `-ln`,
`-rl` → `-l`), not just when rewriting `grep`.

---

## Recurrence 2026-06-26 — `rg -rln` still collapses identifiers to `ln` on `0.42.0-algolia.4`

The `-rln`→`-ln` short-bundle strip on the **direct `rg` entrypoint** proposed in the
2026-06-25 entry is confirmed **still un-applied** as of `rtk 0.42.0-algolia.4`.

- **Command:** `rg -rln 'outdoor|role|entity' src/domovoy --glob '*.py' | rg -i 'ha|home_assist|poll|observ'`
  (intent: list Python files mentioning those identifiers).
- **Observed (mangled):** every `role` → `ln`, `outdoor` → `ln`, `not_role` → `not_ln`,
  `home_assistant` truncated to `home_assist` — e.g. real source
  `latest_observation(metric="temperature", role="outdoor")` rendered as
  `latest_observation(metric="temperature", not_ln="ln")`. The substitution made the
  output actively misleading about the code's real parameter names (I nearly mis-traced
  the T_out role-tagging logic because of it).
- **Confirmed real (not the file):** `Read` of `observations.py:227` showed the true
  params are `role` / `not_role` matching `attributes.role == "outdoor"` — file intact;
  RTK's `--replace=ln` (from the `-rln` bundle) ate the tokens.
- **Workaround:** drop `-r` (`rg -ln 'pat' src/`) or use the Read tool.
