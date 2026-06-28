# RTK rewrites `next start` / `next dev` runtime output into a fake "Next.js Build" summary

- **Date:** 2026-06-26
- **Severity:** Medium (output-mangling — hides real runtime errors, sends debugging down the wrong path)
- **Affected component:** output filter / summarizer for Next.js dev-server commands
- **rtk --version:** 0.42.0-algolia.4

## Summary (root cause)

When a long-running `npx next start` / `npx next dev` is launched (here as a
background task), RTK replaces the server's real stdout/stderr with a synthesized
build-style summary:

```
Next.js Build
═══════════════════════════════════════
Errors: 1 | Warnings: 0
```

This is **output-mangling**, not a command rewrite. `next start` is a long-running
server, not a build — it has no "Errors/Warnings" build summary. The synthesized
summary both (a) invents a build phase that didn't run and (b) reports "Errors: 1"
for what was actually a *successful start with a warning*. The real, useful line
was suppressed.

## Observed

Launched (background): `npx next start -p 4488`
RTK-surfaced output: the fake "Next.js Build / Errors: 1 | Warnings: 0" block, and
the background task was marked `failed (exit code 1)`.

Re-running the SAME command raw (sandbox disabled, no RTK summarization) showed the
truth — the server started fine and the "error" was a benign warning:

```
▲ Next.js 16.2.9
- Local:        http://localhost:4499
✓ Ready in 133ms
⚠ "next start" does not work with "output: standalone" configuration.
  Use "node .next/standalone/server.js" instead.
```

So: server READY, exit was from the eventual kill, and the only real signal was a
*warning* about standalone config — which the fake summary recast as "Errors: 1".

## Expected

Pass through `next start` / `next dev` output verbatim (or near-verbatim). These are
dev servers; their `✓ Ready`, `Local:` URL, and `⚠` warning lines are exactly what
the caller needs. Do not coerce a long-running server's output into a build summary,
and never recast a `⚠ warning` line as `Errors: N`.

## Impact

Cost ~20 min of misdirected debugging: the fake "Errors: 1" implied a build/code
failure, when the code built clean (typecheck + unit tests + `next build` all green)
and the only issue was a serving-config warning. The mangled summary actively
pointed away from the real cause.

## Workaround

Re-run the server command with the sandbox/RTK bypass to read raw output
(`dangerouslyDisableSandbox`, or `rtk proxy <cmd>`), then read the genuine
`✓ Ready` / `⚠` lines.

## Minimal anonymized repro

1. In any Next.js 16 project with `output: "standalone"` in next.config.
2. Launch `npx next start -p <port>` as a background task through RTK.
3. Observe the surfaced output is a fake "Next.js Build / Errors: 1" block, not the
   server's real `✓ Ready` + `⚠ standalone` warning.
