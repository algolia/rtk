# RTK failure audit — 7-day conversation-log sweep

**Date:** 2026-06-03
**Scope:** All Claude Code conversation logs modified in the trailing 7 days (`~/.claude/projects/*/*.jsonl`), across 5 projects.
**rtk version:** 0.42.0-algolia.2
**Method:** Automated scan for RTK command-rewrite errors, output-mangling, crashes, and user complaints; each candidate verified against the source line before inclusion; false positives explicitly ruled out (see below).

## Resolution pass — 2026-06-25 (verified against a fresh `main` build)

Every report's **exact** repro was re-run against a from-source `main` binary (not the
installed `algolia.2` the sessions logged). Result: **the fork is *ahead* of upstream on
this cluster, not behind** — so a clean upstream catchup was the wrong reflex (it would
*regress* our fixes; our grep PR (upstream #2254) is still OPEN upstream, and the live
bugs are upstream's own design too). We refixed locally instead.

| Report | Verdict | Where |
|--------|---------|-------|
| rg→grep dropping flags | ✅ fixed | algolia.3 (`73d8e14`) — deploy gap |
| truncates rg through redirect | ✅ fixed | algolia.3 (`80d7a68`) — deploy gap |
| permission-denied exit 127 | ✅ fixed | algolia.3 (`a96bee8`) — deploy gap |
| mangles grep output identifiers | ✅ fixed | algolia.3 (`d775f56`) — deploy gap |
| condenses head output | ✅ not reproducible | algolia.3 — deploy gap |
| grep `-rhoE` dumps rg help | ✅ fixed | **algolia.4** — strip short `-h` (rg `--help`) |
| grep `-c`/format filename prefix | ✅ fixed | **algolia.4** — no forced `-n`/`-H` in format mode |
| git diff → non-applicable summary | ✅ fixed | **algolia.4** — verbatim passthrough (patch applies) |
| grep BRE literal paren | 🔶 known limitation | dialect-blind proxy; documented + workaround; identity-routing tracked for catchup |

**Two takeaways for the maintainer:** (1) 5 of 9 were a *deploy gap* — fixed in `algolia.3`
but the sessions ran `algolia.2`; reinstalling is the cheapest win. (2) The grep dialect
bug (BRE vs ERE) cannot be fixed inside the output filter because the hook collapses both
`grep` and `rg` to `rtk grep` — the correct fix is source-identity routing, deferred to the
next catchup so it lands once, alongside upstream's permission-layer hardening.

## Headline

**59 distinct command-level failures → 2 live root causes.** The dominant pain is the `rg`→`grep` rewrite (still live, new flag variants found); a second, higher-severity crash (`Permission denied`, exit 127) surfaces on basic file reads. No new output-identifier mangling was seen this window.

## Findings

| # | Root cause | Severity | Occurrences | Report |
|---|---|---|---|---|
| 1 | `rg` executed as `grep`; ripgrep-only flags rejected or silently misread | Medium | 64 (2026-06-03 alone) | [rtk-rewrites-rg-to-grep-dropping-flags.md](./rtk-rewrites-rg-to-grep-dropping-flags.md) |
| 2 | `[rtk: Permission denied (os error 13)]`, exit 127 on `cat`/`ls`/`head` — child never runs | High | 10 | [rtk-permission-denied-exit-127-on-cat-ls.md](./rtk-permission-denied-exit-127-on-cat-ls.md) |
| — | Output identifier mangling (grep/rg/sed of source) | High | 0 this window | [rtk-mangles-grep-output-identifiers.md](./rtk-mangles-grep-output-identifiers.md) (prior; no new occurrences) |

### #1 — `rg`→`grep` rewrite (dominant)

The rewrite passes ripgrep flags to GNU `grep` verbatim. Confirmed broken flags: `--glob`/`-g`, `--type-add`, and **new this sweep** `--type py` / `-t py`. The short `-t` case is the worst: `-t` is a *valid* grep flag (`--text`), so it's silently misinterpreted rather than erroring — a correctness hazard, not just a usability one. Distribution: Python monorepo A (26), Python service C (27), evals monorepo (16), TS SPA (5), other (2). Workarounds that worked: absolute-path `/usr/bin/rg`, native `grep --include='*.py'`. `rtk proxy` was declined by the user.

### #2 — `Permission denied` crash on file reads (highest severity)

RTK aborts with an internal `EACCES` *before* exec on certain `cat`/`ls`/`head` invocations, returning exit 127 on world-readable files. Suspected failed write to RTK's own `.rtk/` cache/state (intermittent → likely a lazy-init/race path). 9 occurrences in one TS SPA project + 1 in RTK's own `hook claude` path (where a wrapper `exit=0` masked it). Recovery was always the editor Read tool.

## Ruled out (not RTK — logged so they aren't re-reported)

- `.env:19: command not found` — malformed `.env` (bare token, no `KEY=` prefix). User error.
- `Monitor pgrep|head SIGPIPE` — caller's own pipeline, not an RTK rewrite.
- `/usr/bin/grep: binary file matches` — expected GNU grep behavior; a *symptom* of #1 (grep lacks rg's binary defaults), not a separate bug.
- `n.encode(` match — verified legitimate `hmac.new(...).encode()`; not mangling.

## Recommended priority for the maintainer

1. **#2 first** — it silently breaks basic file reads with a confusing exit 127; highest user impact and likely a small fix (degrade the `.rtk/` state write to a warning instead of fatal).
2. **#1 next** — either exempt `rg` from the `grep` rewrite, or translate ripgrep flags properly. The silent `-t` misread makes this a correctness issue, not just ergonomics.

## Provenance

Anonymized per PII policy: project names generalized, paths reduced to `<repo>/...`, App IDs → `<APP_ID>`, no credential values transcribed. Source conversation files intentionally omitted from the per-bug reports; available on request for repro.
