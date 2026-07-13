---
name: rtk mangling/error report
about: rtk rewrote a command wrong, mangled output, or crashed. Report it so we can fix by root cause.
title: "rtk <verb> ... → <symptom>"
labels: mangling-report
---

<!--
Before filing: search existing issues for the same symptom
(https://github.com/algolia/rtk/issues?q=is%3Aissue+<symptom>).
If one matches, comment there with your case instead of opening a duplicate.

PII SAFETY — keep this report anonymized:
- Replace real project names, repo slugs, and file paths with placeholders (e.g. <project>, /abs/path/).
- No hostnames, usernames, tokens, or secrets. Use user@host, <token>, etc.
- Paste only the minimal command + output needed to reproduce.
-->

- **Date**:
- **Severity**: <!-- low / medium / high — with a one-line rationale (what did it cost you?) -->
- **Affected component**: <!-- e.g. command-rewrite on `git branch` / output-mangling on `git diff` / crash -->
- **rtk --version**: <!-- output of `rtk --version` -->

## Summary (root cause)

<!--
One paragraph. Classify the failure:
- command-rewrite  — flags dropped / args mangled before the tool runs
- output-mangling  — tool ran fine, but rtk's compaction corrupted the output
- crash            — rtk panicked / errored instead of running the command
-->

## Observed

<!-- The exact command(s) you ran and the wrong output you got. -->

```
$ <command>
<mangled output>
```

## Expected

<!-- What the raw command would have produced. -->

```
<expected output>
```

## Workaround

<!-- If you found one. Usually: bypass the rewrite with raw passthrough. -->

```
rtk proxy <command>
```

## Minimal anonymized reproduction

1.
2.
3.
