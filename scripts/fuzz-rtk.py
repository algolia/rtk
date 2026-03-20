#!/usr/bin/env python3
"""RTK Agentic Fuzzer — uses MiniMax M2.5 to discover format-changing flag bugs.

Generates diverse command invocations via LLM, runs them raw vs through RTK,
and compares outputs to detect mangling, data loss, and format corruption.

Usage:
    python scripts/fuzz-rtk.py --rounds 3 --per-round 10
    python scripts/fuzz-rtk.py --family git-log,grep --rounds 1
    python scripts/fuzz-rtk.py --dry-run --family git-status
    python scripts/fuzz-rtk.py --output /tmp/fuzz-report.json
"""

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field, asdict
from datetime import datetime, timezone
from difflib import SequenceMatcher
from pathlib import Path
from typing import Optional

try:
    import requests
except ImportError:
    print("Missing dependency: requests\n  pip install requests", file=sys.stderr)
    sys.exit(1)

# ──────────────────────────────────────────────────────────────────────
# Constants
# ──────────────────────────────────────────────────────────────────────

API_URL = "https://inference.api.enablers.algolia.net/v1/chat/completions"
API_MODEL = "qwen3.5-35b-fp8"
API_MAX_TOKENS = 4000
COMMAND_TIMEOUT = 30  # seconds


def _set_timeout(val: int):
    global COMMAND_TIMEOUT
    COMMAND_TIMEOUT = val

# Commands that must NEVER be generated or executed
BLOCKLIST_WORDS = {
    "push", "rm", "delete", "drop", "reset", "clean", "force", "destroy",
    "prune", "rebase", "cherry-pick", "merge", "commit", "tag", "stash",
    "checkout", "switch", "restore", "mv", "rename", "kill", "stop",
    "remove", "purge", "truncate", "write", "create", "edit", "close",
}

# RTK formatting markers that indicate filter was applied
RTK_MARKERS = {"📄", "🔍", "🏃", "✅", "❌", "🐳", "⚠️", "📜", "ok ✓", "ok ✗"}

# Flags that signal machine-readable output (should be preserved verbatim)
MACHINE_FLAGS = {
    "--porcelain", "--json", "-c", "--count", "--quiet", "-q",
    "--name-only", "--name-status", "--format", "--pretty",
    "--message-format", "--output-format", "--out-format",
    "--template", "--jq", "--vimgrep",
}

COMMAND_FAMILIES = {
    "git-log": {
        "rtk_cmd": "rtk git log",
        "raw_cmd": "git log",
        "focus_flags": [
            "--format", "--pretty", "--oneline", "--raw", "--patch",
            "--stat", "--numstat", "--shortstat", "--name-only",
            "--name-status", "--graph", "--all", "--no-merges",
            "--abbrev-commit", "--decorate=no",
        ],
        "context": "git repository with 50+ commits",
        "limit_hint": "-10",
    },
    "git-status": {
        "rtk_cmd": "rtk git status",
        "raw_cmd": "git status",
        "focus_flags": [
            "--porcelain", "--porcelain=v2", "-s", "--short",
            "-b", "--branch", "--long", "--no-ahead-behind",
        ],
        "context": "git repository",
    },
    "git-diff": {
        "rtk_cmd": "rtk git diff",
        "raw_cmd": "git diff",
        "focus_flags": [
            "--stat", "--numstat", "--shortstat", "--name-only",
            "--name-status", "--cached", "--staged", "--raw",
            "--no-color", "--word-diff",
        ],
        "context": "git repository (diff may be empty if clean, that's ok)",
    },
    "git-show": {
        "rtk_cmd": "rtk git show",
        "raw_cmd": "git show",
        "focus_flags": [
            "--format", "--pretty", "--stat", "--raw",
            "--name-only", "--name-status", "--no-patch",
        ],
        "context": "git repository with commits",
    },
    "grep": {
        "rtk_cmd": "rtk grep",
        "raw_cmd": "rg",
        "focus_flags": [
            "-c", "--count", "-l", "--files-with-matches",
            "-h", "--no-filename", "--vimgrep",
            "-A", "-B", "-C", "--json",
        ],
        "context": "Rust project directory with .rs files",
        "note": "RTK grep wraps ripgrep. Use patterns that will match (e.g. 'fn ', 'use ', 'pub ').",
    },
    "gh-pr": {
        "rtk_cmd": "rtk gh pr",
        "raw_cmd": "gh pr",
        "focus_flags": ["--json", "--json=", "--template", "--jq", "--web"],
        "context": "GitHub repository with pull requests",
        "note": "Use 'list' subcommand. --web opens browser, skip it.",
    },
    "gh-run": {
        "rtk_cmd": "rtk gh run",
        "raw_cmd": "gh run",
        "focus_flags": ["--json", "--json=", "--template", "--jq", "--log-failed", "--log"],
        "context": "GitHub repository with workflow runs",
        "note": "Use 'list' subcommand. --log/--log-failed require a run ID.",
    },
    "cargo-build": {
        "rtk_cmd": "rtk cargo build",
        "raw_cmd": "cargo build",
        "focus_flags": ["--message-format=json", "--message-format=short", "-q", "--quiet"],
        "context": "Rust project with Cargo.toml",
    },
    "ls": {
        "rtk_cmd": "rtk ls",
        "raw_cmd": "ls",
        "focus_flags": ["-l", "-1", "-a", "-R", "-S", "-t", "-h", "--color=never"],
        "context": "any directory",
    },
    "find": {
        "rtk_cmd": "rtk find",
        "raw_cmd": "find . -maxdepth 3",
        "focus_flags": [
            "-name", "-type", "-iname", "-path",
            "-printf", "-print0", "-ls",
        ],
        "context": "project directory with source files",
        "note": "RTK find uses its own glob-based finder. Compare with system find. Keep -maxdepth 3 for speed.",
    },
    "cat": {
        "rtk_cmd": "rtk read",
        "raw_cmd": "cat",
        "focus_flags": ["-n", "-b", "-s", "-v", "-e", "-t"],
        "context": "source code files in a project",
        "note": "RTK 'read' replaces cat. Generate commands like 'cat src/main.rs' (raw) vs 'rtk read src/main.rs'. Use real filenames from a Rust project (src/main.rs, Cargo.toml, README.md).",
    },
    "tree": {
        "rtk_cmd": "rtk tree",
        "raw_cmd": "tree",
        "focus_flags": [
            "-L", "-d", "-f", "-i", "-p", "-s", "--charset=ascii",
            "-I", "--prune", "-a", "--dirsfirst",
        ],
        "context": "project directory",
        "note": "Always use -L 2 or -L 3 to limit depth.",
    },
    "cargo-test": {
        "rtk_cmd": "rtk cargo test",
        "raw_cmd": "cargo test",
        "focus_flags": [
            "--message-format=json", "--message-format=short",
            "-q", "--quiet", "--no-fail-fast", "-- --nocapture",
        ],
        "context": "Rust project with tests",
        "note": "Tests may take a few seconds. Use --no-run to skip execution if needed.",
    },
    "cargo-clippy": {
        "rtk_cmd": "rtk cargo clippy",
        "raw_cmd": "cargo clippy",
        "focus_flags": [
            "--message-format=json", "--message-format=short",
            "-q", "--quiet", "--all-targets",
        ],
        "context": "Rust project",
    },
    "git-branch": {
        "rtk_cmd": "rtk git branch",
        "raw_cmd": "git branch",
        "focus_flags": [
            "-a", "--all", "-r", "--remotes", "-v", "--verbose",
            "--sort=-committerdate", "--format", "--list",
        ],
        "context": "git repository with branches",
    },
    "git-stash": {
        "rtk_cmd": "rtk git stash",
        "raw_cmd": "git stash",
        "focus_flags": ["list", "show", "show -p"],
        "context": "git repository (may have no stashes, that's ok)",
        "note": "Only use 'list' and 'show' subcommands. NEVER 'drop', 'pop', 'apply', 'push'.",
    },
    "curl": {
        "rtk_cmd": "rtk curl",
        "raw_cmd": "curl",
        "focus_flags": [
            "-s", "-v", "-I", "--head", "-o /dev/null", "-w",
            "--write-out", "-L", "--max-time 5",
        ],
        "context": "network access available",
        "note": "Use https://httpbin.org/ endpoints. Always include --max-time 5. Only GET requests.",
    },
    "wc": {
        "rtk_cmd": "rtk wc",
        "raw_cmd": "wc",
        "focus_flags": ["-l", "-w", "-c", "-m", "-L"],
        "context": "source files in project",
        "note": "Generate 'wc -l src/*.rs' style commands.",
    },
    "env": {
        "rtk_cmd": "rtk env",
        "raw_cmd": "env",
        "focus_flags": [],
        "context": "shell environment",
        "note": "Just compare 'env' vs 'rtk env'. RTK filters sensitive vars.",
    },
    "diff": {
        "rtk_cmd": "rtk diff",
        "raw_cmd": "diff",
        "focus_flags": [
            "-u", "--unified", "-y", "--side-by-side",
            "-q", "--brief", "--color=never",
        ],
        "context": "two files that exist in the project",
        "note": "Compare two real files e.g. 'diff src/main.rs Cargo.toml'. Files should exist.",
    },
    # ── New families (fuzzer v2) ──────────────────────────────────────
    "separator": {
        "rtk_cmd": "rtk",
        "raw_cmd": "",
        "focus_flags": ["--"],
        "context": "Commands using -- separator to delimit flags from args",
        "note": "Test that -- separator is preserved. Generate commands like 'cargo test -- --test-threads=1' or 'git log -- src/main.rs'. The -- must survive RTK's arg parsing.",
    },
    "gh-issue": {
        "rtk_cmd": "rtk gh issue",
        "raw_cmd": "gh issue",
        "focus_flags": ["--json", "--jq", "--template", "--comments", "--state"],
        "context": "GitHub repository with issues",
        "note": "Use 'list' subcommand. Test --json with different field sets.",
    },
    "gh-api": {
        "rtk_cmd": "rtk gh api",
        "raw_cmd": "gh api",
        "focus_flags": ["--jq", "-q", "--template", "-H", "--method GET"],
        "context": "GitHub API access",
        "note": "Use read-only API endpoints like 'repos/algolia/rtk' or 'repos/algolia/rtk/pulls?per_page=2'. Always GET.",
    },
    "empty-output": {
        "rtk_cmd": "rtk",
        "raw_cmd": "",
        "focus_flags": [],
        "context": "Commands that produce empty or minimal output",
        "note": "Test edge cases: no matches, clean repos, empty results. RTK must not panic or produce errors on empty input.",
    },
    "large-output": {
        "rtk_cmd": "rtk",
        "raw_cmd": "",
        "focus_flags": [],
        "context": "Commands producing large output — test truncation behavior",
        "note": "Generate commands that produce 100+ lines. RTK should compress but not lose critical data. E.g. 'git log -100', 'rg pattern . --no-heading', 'ls -laR src/'.",
    },
    "stderr-commands": {
        "rtk_cmd": "rtk",
        "raw_cmd": "",
        "focus_flags": [],
        "context": "Commands that produce output on stderr (not stdout)",
        "note": "Test that stderr content survives RTK filtering. E.g. 'cargo clippy' (warnings on stderr), 'cargo check' (errors on stderr).",
    },
}

# Static regression test cases — always run, no LLM needed
STATIC_TESTS = {
    "git-log": [
        "git log --format='%H' -5",
        "git log --pretty=raw -3",
        "git log --stat -5",
        "git log --graph --oneline -10",
        "git log --numstat -5",
        "git log --name-only -5",
        "git log --patch -1",
        "git log --shortstat -5",
        "git log --pretty=format:'%h %an %s' -5",
    ],
    "git-status": [
        "git status --porcelain",
        "git status --porcelain=v2",
        "git status -s",
        "git status --short --branch",
    ],
    "git-diff": [
        "git diff --stat HEAD~1",
        "git diff --name-only HEAD~1",
        "git diff --name-status HEAD~1",
        "git diff --numstat HEAD~1",
        "git diff --shortstat HEAD~1",
    ],
    "git-show": [
        "git show --stat HEAD",
        "git show --format='%H %s' --no-patch HEAD",
        "git show --name-only HEAD",
        "git show --raw HEAD",
    ],
    "git-branch": [
        "git branch -a",
        "git branch -v",
        "git branch --format='%(refname:short) %(objectname:short)'",
        "git branch --sort=-committerdate",
    ],
    "grep": [
        "rg 'fn ' . -c",
        "rg 'fn ' . -l",
        "rg 'fn ' . --vimgrep",
        "rg 'fn ' . -A 2",
        "rg 'fn ' . --json",
        "rg 'fn ' . -h",
        "rg 'fn ' . -i --count",
        "rg 'fn ' . -B 1",
        "rg 'fn ' . --files-without-match",
    ],
    "gh-pr": [
        "gh pr list --json number,title,state",
        "gh pr list --json number,title --jq '.[0]'",
    ],
    "cargo-build": [
        "cargo build --message-format=short",
        "cargo check --message-format=json",
    ],
    "cargo-clippy": [
        "cargo clippy --message-format=short",
        "cargo clippy -q",
    ],
    "cat": [
        "cat src/main.rs",
        "cat -n src/main.rs",
    ],
    "ls": [
        "ls -la",
        "ls -1",
        "ls -lhS src/",
        "ls -R src/",
        "ls -la --color=never",
    ],
    "find": [
        "find . -maxdepth 2 -name '*.rs'",
        "find . -maxdepth 2 -type d",
        "find . -maxdepth 2 -name '*.toml'",
    ],
    "tree": [
        "tree -L 2",
        "tree -d -L 3",
        "tree -L 1 --charset=ascii",
        "tree -L 2 -f",
    ],
    "wc": [
        "wc -l src/main.rs",
        "wc -w src/main.rs",
        "wc -c src/main.rs",
        "wc -l src/git.rs src/grep_cmd.rs",
    ],
    "diff": [
        "diff src/main.rs Cargo.toml",
        "diff -u src/main.rs Cargo.toml",
        "diff -q src/main.rs Cargo.toml",
    ],
    "env": [
        "env",
    ],
    "curl": [
        "curl -sI --max-time 5 https://httpbin.org/get",
    ],
    # ── New families (fuzzer v2) ──────────────────────────────────────
    "separator": [
        # -- separator between tool flags and underlying tool flags
        "git log -- src/main.rs",
        "git log --oneline -- src/main.rs",
        "git diff HEAD~1 -- src/main.rs",
        "grep -- 'fn ' src/main.rs",
        # cargo -- passthrough to test binary
        "cargo test -- --test-threads=1",
    ],
    "gh-issue": [
        "gh issue list --json number,title,state --state all",
        "gh issue list --state closed --limit 3",
    ],
    "gh-api": [
        "gh api repos/algolia/rtk --jq '.name'",
        "gh api repos/algolia/rtk/pulls?per_page=2 --jq '.[].title'",
    ],
    "large-output": [
        # Large result sets — test truncation behavior
        "git log --oneline -100",
        "git log --stat -50",
        "rg 'fn ' . --no-heading",
        "rg 'use ' . --no-heading",
        "ls -laR src/",
    ],
    "stderr-commands": [
        # Commands that produce output on stderr (not just stdout)
        "cargo clippy --all-targets",
        "cargo check",
    ],
    "empty-output": [
        # No matches / clean state — must not panic
        "rg 'ZZZZNONEXISTENT_PATTERN' .",
        "git log --author=zzzzz_nobody -1",
        "git diff HEAD HEAD",
        "find . -name '*.ZZZNONEXISTENT'",
        "wc -l /dev/null",
    ],
}

# ──────────────────────────────────────────────────────────────────────
# Data classes
# ──────────────────────────────────────────────────────────────────────


@dataclass
class RunResult:
    stdout: str
    stderr: str
    exit_code: int
    duration_ms: int
    error: Optional[str] = None


@dataclass
class Issue:
    code: str
    detail: str


@dataclass
class TestResult:
    command: str
    family: str
    raw: Optional[RunResult]
    rtk: Optional[RunResult]
    verdict: str  # PASS, WARN, FAIL, ERROR, SKIP
    issues: list[Issue] = field(default_factory=list)
    skipped_reason: Optional[str] = None


@dataclass
class Report:
    timestamp: str
    rtk_version: str
    test_repo: str
    rounds: int
    per_round: int
    families_tested: list[str]
    results: list[TestResult] = field(default_factory=list)

    @property
    def summary(self) -> dict:
        counts = {"total": 0, "pass": 0, "warn": 0, "fail": 0, "error": 0, "skip": 0}
        for r in self.results:
            counts["total"] += 1
            counts[r.verdict.lower()] += 1
        return counts

    def to_dict(self) -> dict:
        d = asdict(self)
        d["summary"] = self.summary
        # Separate failures for quick scanning
        d["failures"] = [asdict(r) for r in self.results if r.verdict == "FAIL"]
        d["warnings"] = [asdict(r) for r in self.results if r.verdict == "WARN"]
        return d


# ──────────────────────────────────────────────────────────────────────
# Vault token
# ──────────────────────────────────────────────────────────────────────


def fetch_vault_token() -> str:
    try:
        result = subprocess.run(
            ["vault", "read", "--field=token", "identity/oidc/token/enablers"],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode != 0:
            print(f"vault failed: {result.stderr.strip()}", file=sys.stderr)
            sys.exit(1)
        return result.stdout.strip()
    except FileNotFoundError:
        print("vault CLI not found", file=sys.stderr)
        sys.exit(1)


# ──────────────────────────────────────────────────────────────────────
# MiniMax API client
# ──────────────────────────────────────────────────────────────────────

SYSTEM_PROMPT = """You are a CLI fuzzer targeting RTK, a tool that proxies and compresses command output.
RTK intercepts commands (git, grep, docker, cargo, gh) and reformats their output for LLM token savings.

Your job: generate commands that use FORMAT-CHANGING FLAGS — flags that alter output structure
in ways that might confuse RTK's parser. The goal is to find cases where RTK mangles output.

RULES:
- ONLY read-only commands. NEVER: push, delete, rm, reset, commit, merge, rebase, checkout, write, create, close, edit
- Commands must be valid and runnable
- Output ONLY a JSON array of command strings, nothing else
- Each command should test a DIFFERENT flag combination
- Focus on flags that change output FORMAT, not content
- Keep commands short and focused on one or two format flags each"""


def generate_test_cases(token: str, family_name: str, family: dict, count: int) -> list[str]:
    prompt = f"""Generate {count} diverse commands for `{family['raw_cmd']}` that test format-changing flags.

Target flags: {', '.join(family['focus_flags'])}
Context: {family['context']}
{f"Note: {family['note']}" if 'note' in family else ""}
{f"Always include a limit like {family['limit_hint']} to keep output small." if 'limit_hint' in family else ""}

Strategies to try:
1. Single format flag (e.g., --format="%H")
2. Combined format flags (e.g., --oneline --graph)
3. Machine-readable modes (e.g., --porcelain, --json, -c)
4. Flags that suppress normal output (e.g., --quiet, --name-only)
5. Custom format strings with unusual placeholders

Output as JSON array of command strings. Example for git-log:
["git log --format='%H %s' -5", "git log --oneline --all --graph -10"]"""

    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
    }
    payload = {
        "model": API_MODEL,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": prompt},
        ],
        "max_tokens": API_MAX_TOKENS,
        "temperature": 0.9,  # High creativity for diverse commands
    }

    try:
        resp = requests.post(API_URL, headers=headers, json=payload, timeout=120)
        resp.raise_for_status()
        data = resp.json()
        content = data["choices"][0]["message"].get("content") or ""

        if not content:
            # Fallback: some reasoning models put output in reasoning field
            content = data["choices"][0]["message"].get("reasoning") or ""

        # Extract JSON array from response (may have markdown fences)
        return _extract_json_array(content)
    except Exception as e:
        print(f"  LLM API error: {e}", file=sys.stderr)
        return []


def _extract_json_array(text: str) -> list[str]:
    """Extract a JSON array of strings from LLM response, handling markdown fences."""
    # Try direct parse first
    text = text.strip()
    try:
        result = json.loads(text)
        if isinstance(result, list):
            return [str(x) for x in result]
    except (json.JSONDecodeError, ValueError):
        pass

    # Strip markdown fences
    for fence in ("```json", "```"):
        if fence in text:
            start = text.index(fence) + len(fence)
            end = text.index("```", start) if "```" in text[start:] else len(text)
            text = text[start:end].strip()
            try:
                result = json.loads(text)
                if isinstance(result, list):
                    return [str(x) for x in result]
            except (json.JSONDecodeError, ValueError):
                pass

    # Last resort: find first [ ... ] block
    bracket_start = text.find("[")
    bracket_end = text.rfind("]")
    if bracket_start >= 0 and bracket_end > bracket_start:
        try:
            result = json.loads(text[bracket_start:bracket_end + 1])
            if isinstance(result, list):
                return [str(x) for x in result]
        except (json.JSONDecodeError, ValueError):
            pass

    return []


# ──────────────────────────────────────────────────────────────────────
# Safety validation
# ──────────────────────────────────────────────────────────────────────


def is_safe_command(cmd: str) -> tuple[bool, str]:
    """Validate command is read-only. Returns (safe, reason)."""
    parts = cmd.lower().split()
    for word in parts:
        # Check each token against blocklist
        clean = word.strip("-").strip("=")
        if clean in BLOCKLIST_WORDS:
            return False, f"blocked word: {word}"

    # Block shell operators (but allow | inside quotes for format strings)
    # Strip quoted sections before checking for operators
    stripped = cmd
    for q in ('"', "'"):
        while q in stripped:
            start = stripped.index(q)
            end = stripped.index(q, start + 1) if q in stripped[start + 1:] else len(stripped)
            stripped = stripped[:start] + stripped[end + 1:]
    for op in ("|", "&&", "||", ";", ">", ">>", "<", "`", "$("):
        if op in stripped:
            return False, f"shell operator: {op}"

    return True, ""


def cmd_to_rtk(cmd: str, family: dict) -> str:
    """Convert a raw command to its RTK equivalent.

    e.g., 'git log --oneline -5' → 'rtk git log --oneline -5'
    """
    raw_prefix = family["raw_cmd"]
    rtk_prefix = family["rtk_cmd"]

    # Multi-command families (separator, empty-output, etc.) have no single
    # raw_cmd prefix — detect the base command and prepend rtk
    if not raw_prefix:
        parts = cmd.split()
        if parts:
            base = parts[0]
            # Map known commands to their rtk equivalents
            RTK_MAP = {
                "git": "rtk git", "rg": "rtk grep", "grep": "rtk grep",
                "cargo": "rtk cargo", "find": "rtk find", "ls": "rtk ls",
                "wc": "rtk wc", "diff": "rtk diff", "tree": "rtk tree",
                "gh": "rtk gh", "curl": "rtk curl", "cat": "rtk read",
            }
            if base in RTK_MAP:
                return RTK_MAP[base] + cmd[len(base):]
        return f"rtk {cmd}"

    if cmd.startswith(raw_prefix):
        return rtk_prefix + cmd[len(raw_prefix):]
    return f"rtk {cmd}"


# ──────────────────────────────────────────────────────────────────────
# Command execution
# ──────────────────────────────────────────────────────────────────────


def run_command(cmd: str, cwd: str, label: str = "") -> RunResult:
    """Execute a command and capture output."""
    try:
        parts = shlex.split(cmd)
    except ValueError as e:
        return RunResult("", "", -1, 0, error=f"shlex parse error: {e}")

    # Skip RTK tracking so fuzzer runs don't pollute `rtk gain` stats
    env = {**os.environ, "RTK_SKIP_TRACKING": "1"}

    start = time.monotonic()
    try:
        result = subprocess.run(
            parts, capture_output=True, text=True,
            timeout=COMMAND_TIMEOUT, cwd=cwd, env=env,
        )
        duration = int((time.monotonic() - start) * 1000)
        return RunResult(
            stdout=result.stdout,
            stderr=result.stderr,
            exit_code=result.returncode,
            duration_ms=duration,
        )
    except subprocess.TimeoutExpired:
        duration = int((time.monotonic() - start) * 1000)
        return RunResult("", "", -1, duration, error="timeout")
    except FileNotFoundError:
        return RunResult("", "", -1, 0, error=f"command not found: {parts[0]}")
    except Exception as e:
        return RunResult("", "", -1, 0, error=str(e))


# ──────────────────────────────────────────────────────────────────────
# Output comparison heuristics
# ──────────────────────────────────────────────────────────────────────


def compare_outputs(raw: RunResult, rtk: RunResult, cmd: str) -> list[Issue]:
    """Run all 6 heuristic checks. Returns list of issues (empty = pass)."""
    issues = []

    if raw.error or rtk.error:
        if rtk.error and not raw.error:
            issues.append(Issue("RTK_ERROR", f"RTK failed but raw succeeded: {rtk.error}"))
        return issues

    raw_out = raw.stdout
    rtk_out = rtk.stdout

    # 1. JSON Integrity
    issue = _check_json_integrity(raw_out, rtk_out)
    if issue:
        issues.append(issue)

    # 2. Line Expansion
    issue = _check_line_expansion(raw_out, rtk_out)
    if issue:
        issues.append(issue)

    # 3. Emoji/Marker Injection
    issue = _check_emoji_injection(raw_out, rtk_out)
    if issue:
        issues.append(issue)

    # 4. Exit Code Match
    issue = _check_exit_code(raw, rtk)
    if issue:
        issues.append(issue)

    # 5. Data Loss
    issue = _check_data_loss(raw_out, rtk_out)
    if issue:
        issues.append(issue)

    # 6. Format Preservation (machine-readable flags)
    issue = _check_format_preservation(raw_out, rtk_out, cmd)
    if issue:
        issues.append(issue)

    # 7. Stderr Loss (for commands that produce output on stderr)
    issue = _check_stderr_loss(raw, rtk)
    if issue:
        issues.append(issue)

    return issues


def _check_json_integrity(raw: str, rtk: str) -> Optional[Issue]:
    """If raw is valid JSON, RTK must also be valid JSON."""
    raw_s = raw.strip()
    if not raw_s:
        return None
    try:
        json.loads(raw_s)
    except (json.JSONDecodeError, ValueError):
        return None  # Raw isn't JSON, nothing to check

    try:
        json.loads(rtk.strip())
    except (json.JSONDecodeError, ValueError):
        return Issue("JSON_MANGLED", "Raw output is valid JSON but RTK output is not")
    return None


def _check_line_expansion(raw: str, rtk: str) -> Optional[Issue]:
    """RTK should compress, not expand."""
    raw_lines = len(raw.strip().splitlines()) if raw.strip() else 0
    rtk_lines = len(rtk.strip().splitlines()) if rtk.strip() else 0
    if raw_lines > 0 and rtk_lines > raw_lines + 3:
        return Issue("LINE_EXPANSION", f"{raw_lines} → {rtk_lines} lines")
    return None


def _check_emoji_injection(raw: str, rtk: str) -> Optional[Issue]:
    """If raw has no RTK markers but RTK does, filter was applied on format-changing output."""
    raw_has = any(m in raw for m in RTK_MARKERS)
    rtk_has = any(m in rtk for m in RTK_MARKERS)
    if not raw_has and rtk_has:
        found = [m for m in RTK_MARKERS if m in rtk]
        return Issue("MARKER_INJECTION", f"RTK injected markers: {found}")
    return None


def _check_exit_code(raw: RunResult, rtk: RunResult) -> Optional[Issue]:
    if raw.exit_code != rtk.exit_code:
        return Issue("EXIT_CODE_MISMATCH", f"raw={raw.exit_code} rtk={rtk.exit_code}")
    return None


def _check_data_loss(raw: str, rtk: str) -> Optional[Issue]:
    """Sample anchor tokens from raw, check >=50% appear in RTK output."""
    raw_lines = [l for l in raw.strip().splitlines() if l.strip()]
    if len(raw_lines) < 2:
        return None

    anchors = []
    for line in raw_lines[:8]:
        tokens = line.split()
        if tokens:
            # First token is often the most identifying (hash, filename, count)
            anchor = tokens[0].strip(",:;()[]{}\"'")
            if len(anchor) >= 3:  # Skip trivial tokens
                anchors.append(anchor)

    if not anchors:
        return None

    missing = [a for a in anchors if a not in rtk]
    if len(missing) > len(anchors) * 0.5:
        return Issue("DATA_LOSS", f"{len(missing)}/{len(anchors)} anchor tokens missing: {missing[:5]}")
    return None


def _check_format_preservation(raw: str, rtk: str, cmd: str) -> Optional[Issue]:
    """When machine-readable flags are used, output should be near-identical."""
    cmd_lower = cmd.lower()
    has_machine_flag = any(f in cmd_lower for f in MACHINE_FLAGS)
    if not has_machine_flag:
        return None

    # wc intentionally strips filenames for compression — not a format bug
    if cmd_lower.startswith("wc "):
        return None

    raw_s = raw.strip()
    rtk_s = rtk.strip()
    if not raw_s or not rtk_s:
        return None

    if raw_s == rtk_s:
        return None

    # For grep/rg with -c/--count/--json/--vimgrep: rg uses threads so line
    # order is non-deterministic. Sort lines before comparing to avoid false
    # positives when data is identical but order differs.
    cmd_base = cmd.split()[0] if cmd.split() else ""
    is_grep = cmd_base in ("rg", "grep", "rtk") and any(
        w in cmd_lower for w in ("grep", "rg ")
    ) or cmd_base in ("rg", "grep")
    if is_grep:
        raw_sorted = "\n".join(sorted(raw_s.splitlines()))
        rtk_sorted = "\n".join(sorted(rtk_s.splitlines()))
        if raw_sorted == rtk_sorted:
            return None
        ratio = SequenceMatcher(None, raw_sorted[:2000], rtk_sorted[:2000]).ratio()
    else:
        ratio = SequenceMatcher(None, raw_s[:2000], rtk_s[:2000]).ratio()

    if ratio < 0.90:
        return Issue("FORMAT_ALTERED", f"Machine-readable output altered (similarity: {ratio:.0%})")
    return None


def _check_stderr_loss(raw: RunResult, rtk: RunResult) -> Optional[Issue]:
    """If raw has significant stderr content but RTK has none (stdout+stderr), flag it."""
    raw_stderr = raw.stderr.strip() if raw.stderr else ""
    rtk_stderr = rtk.stderr.strip() if rtk.stderr else ""
    rtk_stdout = rtk.stdout.strip() if rtk.stdout else ""

    # Only flag if raw has substantial stderr (>50 chars, not just warnings)
    if len(raw_stderr) < 50:
        return None

    # RTK may legitimately move stderr content to stdout (e.g., cargo clippy filter)
    # So check if the content appears in EITHER stdout or stderr
    if rtk_stderr or rtk_stdout:
        # Sample a few tokens from raw stderr to check they survived somewhere
        raw_lines = [l for l in raw_stderr.splitlines() if l.strip()][:5]
        anchors = []
        for line in raw_lines:
            tokens = line.split()
            for tok in tokens[:2]:
                clean = tok.strip(",:;()[]{}\"'")
                if len(clean) >= 4:
                    anchors.append(clean)
                    break
        if not anchors:
            return None
        combined_rtk = rtk_stdout + "\n" + rtk_stderr
        missing = [a for a in anchors if a not in combined_rtk]
        if len(missing) > len(anchors) * 0.5:
            return Issue("STDERR_LOSS", f"Raw stderr content lost: {len(missing)}/{len(anchors)} anchors missing")
    elif raw_stderr and not rtk_stderr and not rtk_stdout:
        return Issue("STDERR_LOSS", "Raw has stderr output but RTK produced nothing")

    return None


# ──────────────────────────────────────────────────────────────────────
# Verdict
# ──────────────────────────────────────────────────────────────────────

CRITICAL_ISSUES = {"JSON_MANGLED", "EXIT_CODE_MISMATCH", "FORMAT_ALTERED", "DATA_LOSS", "RTK_ERROR", "STDERR_LOSS"}


def determine_verdict(issues: list[Issue]) -> str:
    if not issues:
        return "PASS"
    if any(i.code in CRITICAL_ISSUES for i in issues):
        return "FAIL"
    return "WARN"


# ──────────────────────────────────────────────────────────────────────
# Preflight checks
# ──────────────────────────────────────────────────────────────────────


def preflight(needs_llm: bool = True) -> tuple[str, str]:
    """Run preflight checks. Returns (rtk_version, vault_token).

    If needs_llm=False (static-only run), vault and API checks are skipped.
    """
    print("Preflight checks...", file=sys.stderr)

    # 1. RTK installed
    try:
        r = subprocess.run(["rtk", "--version"], capture_output=True, text=True, timeout=5)
        rtk_version = r.stdout.strip() or r.stderr.strip()
        print(f"  rtk: {rtk_version}", file=sys.stderr)
    except (FileNotFoundError, subprocess.TimeoutExpired):
        print("  FAIL: rtk not installed", file=sys.stderr)
        sys.exit(1)

    if not needs_llm:
        print("  vault: skipped (static-only run)", file=sys.stderr)
        print("  api: skipped (static-only run)", file=sys.stderr)
        token = ""
    else:
        # 2. Vault token
        token = fetch_vault_token()
        print(f"  vault: ok (token {len(token)} chars)", file=sys.stderr)

        # 3. API connectivity
        try:
            resp = requests.get(
                "https://inference.api.enablers.algolia.net/v1/models",
                headers={"Authorization": f"Bearer {token}"},
                timeout=10,
            )
            resp.raise_for_status()
            models = [m["id"] for m in resp.json().get("data", [])]
            print(f"  api: ok ({', '.join(models)})", file=sys.stderr)
            if API_MODEL not in models and "large" not in models:
                print(f"  WARN: {API_MODEL} not in available models", file=sys.stderr)
        except Exception as e:
            print(f"  FAIL: API connectivity: {e}", file=sys.stderr)
            sys.exit(1)

    # 4. Git
    try:
        subprocess.run(["git", "--version"], capture_output=True, timeout=5, check=True)
        print("  git: ok", file=sys.stderr)
    except (FileNotFoundError, subprocess.CalledProcessError):
        print("  FAIL: git not found", file=sys.stderr)
        sys.exit(1)

    return rtk_version, token


# ──────────────────────────────────────────────────────────────────────
# Test repo setup
# ──────────────────────────────────────────────────────────────────────


def setup_test_repo(workdir: str) -> str:
    """Clone RTK repo (shallow) for testing."""
    repo_path = os.path.join(workdir, "test-repo")
    print("Setting up test repo...", file=sys.stderr)
    result = subprocess.run(
        ["git", "clone", "--depth=50", "https://github.com/algolia/rtk.git", repo_path],
        capture_output=True, text=True, timeout=60,
    )
    if result.returncode != 0:
        # Fallback: use current directory if it's a git repo
        if os.path.isdir(".git"):
            print("  Clone failed, using current directory", file=sys.stderr)
            return os.getcwd()
        print(f"  Clone failed: {result.stderr}", file=sys.stderr)
        sys.exit(1)
    print(f"  repo: {repo_path}", file=sys.stderr)
    return repo_path


# ──────────────────────────────────────────────────────────────────────
# Fuzz session
# ──────────────────────────────────────────────────────────────────────


def run_fuzz_session(
    token: str,
    rtk_version: str,
    families: list[str],
    rounds: int,
    per_round: int,
    dry_run: bool,
    test_repo: str,
) -> Report:
    report = Report(
        timestamp=datetime.now(timezone.utc).isoformat(),
        rtk_version=rtk_version,
        test_repo=test_repo,
        rounds=rounds,
        per_round=per_round,
        families_tested=families,
    )

    # Phase 1: Run static regression tests (no LLM needed)
    for family_name in families:
        static_cmds = STATIC_TESTS.get(family_name, [])
        if not static_cmds:
            continue
        family = COMMAND_FAMILIES[family_name]
        print(
            f"\n{'='*60}\n"
            f"Static tests | Family: {family_name} ({len(static_cmds)} tests)\n"
            f"{'='*60}",
            file=sys.stderr,
        )
        for cmd in static_cmds:
            report.results.extend(
                _run_single_test(cmd, family_name, family, dry_run, test_repo)
            )

    # Phase 2: LLM-generated fuzz tests
    for round_num in range(1, rounds + 1):
        for family_name in families:
            family = COMMAND_FAMILIES[family_name]
            print(
                f"\n{'='*60}\n"
                f"Round {round_num}/{rounds} | Family: {family_name}\n"
                f"{'='*60}",
                file=sys.stderr,
            )

            print(f"  Generating {per_round} test cases via {API_MODEL}...", file=sys.stderr)
            commands = generate_test_cases(token, family_name, family, per_round)
            print(f"  Got {len(commands)} commands", file=sys.stderr)

            if not commands:
                report.results.append(TestResult(
                    command="(generation failed)",
                    family=family_name,
                    raw=None, rtk=None,
                    verdict="ERROR",
                    issues=[Issue("LLM_FAILED", "No commands generated")],
                ))
                continue

            for cmd in commands:
                report.results.extend(
                    _run_single_test(cmd, family_name, family, dry_run, test_repo)
                )

    return report


def _run_single_test(
    cmd: str, family_name: str, family: dict, dry_run: bool, test_repo: str,
) -> list[TestResult]:
    """Run a single raw-vs-rtk comparison. Returns list of 1 TestResult."""
    safe, reason = is_safe_command(cmd)
    if not safe:
        print(f"  SKIP (unsafe): {cmd} — {reason}", file=sys.stderr)
        return [TestResult(
            command=cmd, family=family_name,
            raw=None, rtk=None,
            verdict="SKIP", skipped_reason=reason,
        )]

    rtk_cmd = cmd_to_rtk(cmd, family)
    print(f"\n  RAW: {cmd}", file=sys.stderr)
    print(f"  RTK: {rtk_cmd}", file=sys.stderr)

    if dry_run:
        return [TestResult(
            command=cmd, family=family_name,
            raw=None, rtk=None,
            verdict="SKIP", skipped_reason="dry-run",
        )]

    raw_result = run_command(cmd, cwd=test_repo, label="raw")
    rtk_result = run_command(rtk_cmd, cwd=test_repo, label="rtk")

    issues = compare_outputs(raw_result, rtk_result, cmd)
    verdict = determine_verdict(issues)

    raw_for_report = RunResult(
        stdout=raw_result.stdout[:1000],
        stderr=raw_result.stderr[:500],
        exit_code=raw_result.exit_code,
        duration_ms=raw_result.duration_ms,
        error=raw_result.error,
    )
    rtk_for_report = RunResult(
        stdout=rtk_result.stdout[:1000],
        stderr=rtk_result.stderr[:500],
        exit_code=rtk_result.exit_code,
        duration_ms=rtk_result.duration_ms,
        error=rtk_result.error,
    )

    result = TestResult(
        command=cmd, family=family_name,
        raw=raw_for_report, rtk=rtk_for_report,
        verdict=verdict, issues=issues,
    )

    icon = {"PASS": ".", "WARN": "W", "FAIL": "F", "ERROR": "E"}
    status = icon.get(verdict, "?")
    detail = f" — {issues[0].code}: {issues[0].detail}" if issues else ""
    print(f"  [{status}] {verdict}{detail}", file=sys.stderr)

    return [result]


# ──────────────────────────────────────────────────────────────────────
# Main
# ──────────────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(
        description="RTK Agentic Fuzzer — discover format-changing flag bugs via LLM",
    )
    parser.add_argument("--rounds", type=int, default=3, help="Number of fuzzing rounds (default: 3)")
    parser.add_argument("--per-round", type=int, default=10, help="Commands per round per family (default: 10)")
    parser.add_argument("--family", type=str, default=None,
                        help=f"Comma-separated families to test (default: all). "
                             f"Available: {','.join(COMMAND_FAMILIES.keys())}")
    parser.add_argument("--output", type=str, default=None, help="Write JSON report to file")
    parser.add_argument("--dry-run", action="store_true", help="Generate commands but don't execute")
    parser.add_argument("--timeout", type=int, default=COMMAND_TIMEOUT, help="Per-command timeout in seconds")
    parser.add_argument("--use-cwd", action="store_true", help="Use current directory instead of cloning test repo")

    args = parser.parse_args()

    _set_timeout(args.timeout)

    # Resolve families
    if args.family:
        families = [f.strip() for f in args.family.split(",")]
        for f in families:
            if f not in COMMAND_FAMILIES:
                print(f"Unknown family: {f}", file=sys.stderr)
                print(f"Available: {', '.join(COMMAND_FAMILIES.keys())}", file=sys.stderr)
                sys.exit(1)
    else:
        families = list(COMMAND_FAMILIES.keys())

    # Preflight
    needs_llm = args.rounds > 0
    rtk_version, token = preflight(needs_llm=needs_llm)

    # Test repo
    if args.use_cwd:
        test_repo = os.getcwd()
        workdir = None
    else:
        workdir = tempfile.mkdtemp(prefix="rtk-fuzz-")
        test_repo = setup_test_repo(workdir)

    try:
        # Run fuzzing
        report = run_fuzz_session(
            token=token,
            rtk_version=rtk_version,
            families=families,
            rounds=args.rounds,
            per_round=args.per_round,
            dry_run=args.dry_run,
            test_repo=test_repo,
        )

        # Print summary
        s = report.summary
        print(
            f"\n{'='*60}\n"
            f"SUMMARY: {s['total']} tests | "
            f"{s['pass']} pass | {s['warn']} warn | {s['fail']} fail | "
            f"{s['error']} error | {s['skip']} skip\n"
            f"{'='*60}",
            file=sys.stderr,
        )

        if s["fail"] > 0:
            print(f"\nFAILURES:", file=sys.stderr)
            for r in report.results:
                if r.verdict == "FAIL":
                    issue_str = "; ".join(f"{i.code}: {i.detail}" for i in r.issues)
                    print(f"  [{r.family}] {r.command}", file=sys.stderr)
                    print(f"    {issue_str}", file=sys.stderr)

        # Output report
        report_json = json.dumps(report.to_dict(), indent=2, default=str)
        if args.output:
            Path(args.output).write_text(report_json)
            print(f"\nReport written to: {args.output}", file=sys.stderr)
        else:
            print(report_json)

    finally:
        # Cleanup temp dir
        if workdir and os.path.exists(workdir):
            shutil.rmtree(workdir, ignore_errors=True)


if __name__ == "__main__":
    main()
