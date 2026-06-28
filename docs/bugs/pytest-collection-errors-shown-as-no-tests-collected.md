# rtk pytest filter reports collection errors as "No tests collected"

- **Date**: 2026-06-25
- **Severity**: medium (misleading output — masks real failure cause)
- **Affected component**: output filter for `pytest`
- **rtk --version**: rtk 0.42.0-algolia.4

## Summary (root cause)

Output-mangling. When `pytest` fails during **collection** (e.g. an
`ImportError`/`ModuleNotFoundError` in a test module), the rtk pytest filter
summarizes the run as:

```
Pytest: No tests collected
[full output: ~/.local/share/rtk/tee/<id>_pytest.log]
```

This is misleading: there *were* errors during collection, not merely an empty
suite. The "No tests collected" wording reads as a benign "nothing to run",
when in reality multiple modules failed to import and the run aborted. The real
signal (the ERROR collecting … / `ModuleNotFoundError: No module named 'apispec'`
tracebacks) is only visible by opening the tee log.

## Observed

Command:

```
python -m pytest -q
```

Filtered output:

```
Pytest: No tests collected
[full output: ~/.local/share/rtk/tee/1782416519_pytest.log]
```

The tee log contained the truth:

```
==================================== ERRORS ====================================
___________________ ERROR collecting tests/test_api_docs.py ____________________
E   ModuleNotFoundError: No module named 'apispec'
... (several more collection ERRORs) ...
```

## Expected

The summary should distinguish "0 tests, clean" from "collection errored". For
example:

```
Pytest: collection ERROR — N modules failed to import (0 tests run)
  - tests/test_api_docs.py: ModuleNotFoundError: No module named 'apispec'
  [full output: <tee path>]
```

i.e. surface that the run aborted on collection errors and ideally the first
error line per failing module, instead of the benign-sounding
"No tests collected".

## Workaround

Read the tee log directly (`Read ~/.local/share/rtk/tee/<id>_pytest.log`), or
re-run bypassing the filter via `rtk proxy python -m pytest -q`.

## Minimal anonymized reproduction

1. In any pytest project, introduce an import of a non-installed module at the
   top of one test file (e.g. `import a_module_that_is_not_installed`).
2. Run `python -m pytest -q` through the rtk hook.
3. Observe the filtered summary says "No tests collected" rather than reporting
   the collection ImportError.
