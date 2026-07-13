# RTK - Rust Token Killer (Codex CLI)

**Usage**: Token-optimized CLI proxy for shell commands.

## Rule

Always prefix shell commands with `rtk`.

Examples:

```bash
rtk git status
rtk cargo test
rtk npm run build
rtk pytest -q
```

## Meta Commands

```bash
rtk gain            # Token savings analytics
rtk gain --history  # Recent command savings history
rtk proxy <cmd>     # Run raw command without filtering
```

## Verification

```bash
rtk --version
rtk gain
which rtk
```

## Report mangling/errors

If rtk mangles a command or output: work around it with `rtk proxy <cmd>`, then file it immediately (don't ask). Search first: `gh issue list -R algolia/rtk -S "<symptom>"` — comment on a match instead of duplicating; else `gh issue create -R algolia/rtk -l mangling-report -t "rtk <verb> → <symptom>" -b "<body>"` (body: `rtk --version`, root-cause class command-rewrite/output-mangling/crash, observed vs expected, workaround, minimal repro). No `gh`? https://github.com/algolia/rtk/issues/new/choose. Keep it PII-safe/anonymized.
