# RTK - Rust Token Killer

**Usage**: Token-optimized CLI proxy (60-90% savings on dev operations)

## Meta Commands (always use rtk directly)

```bash
rtk gain              # Show token savings analytics
rtk gain --history    # Show command usage history with savings
rtk discover          # Analyze Claude Code history for missed opportunities
rtk proxy <cmd>       # Execute raw command without filtering (for debugging)
```

## Installation Verification

```bash
rtk --version         # Should show: rtk X.Y.Z
rtk gain              # Should work (not "command not found")
which rtk             # Verify correct binary
```

⚠️ **Name collision**: If `rtk gain` fails, you may have reachingforthejack/rtk (Rust Type Kit) installed instead.

## Hook-Based Usage

All other commands are automatically rewritten by the Claude Code hook.
Example: `git status` → `rtk git status` (transparent, 0 tokens overhead)

## Report mangling/errors

If rtk mangles a command or output: (1) work around it with `rtk proxy <cmd>` (raw passthrough), then (2) file it immediately, don't ask permission. Search first: `gh issue list -R algolia/rtk -S "<symptom>"` — comment on a match instead of duplicating; else `gh issue create -R algolia/rtk -l mangling-report -t "rtk <verb> → <symptom>" -b "<body>"` where body has: `rtk --version`, root-cause class (command-rewrite / output-mangling / crash), observed vs expected, workaround, minimal repro. No `gh`? Point the user at https://github.com/algolia/rtk/issues/new/choose. Keep it PII-safe/anonymized (no secrets, real paths, or hostnames).

Refer to CLAUDE.md for full command reference.
