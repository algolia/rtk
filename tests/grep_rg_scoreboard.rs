//! Differential correctness + savings scoreboard for `rtk grep` / `rtk rg`.
//!
//! WHY THIS EXISTS
//! ripgrep is, by its author's own FAQ, *not* a drop-in replacement for grep and
//! "never will be POSIX compatible" — it deliberately diverges (RE2 regex instead of
//! POSIX BRE/ERE, recurse-by-default, `-r`=replace, `-E`=encoding). rtk rewrites BOTH
//! `grep` and `rg` onto the ripgrep engine, so it must bridge that gap faithfully *in
//! both directions*. This harness measures whether it does, by treating the **real
//! tool as the oracle**: for each case we run genuine `grep`/`rg` (ground truth) and
//! compare rtk's routed output to it.
//!
//! WHAT IT CHECKS (two dimensions, per the agreed design)
//!   1. Correctness — rtk must be faithful to ground truth *modulo declared compaction*:
//!      exact counts (`-c`), exact file lists (`-l`/`--files`), no fabricated or silently
//!      dropped matches (drops must be accounted for by a `[+N more]` marker), and never
//!      an error/crash where the real tool succeeds.
//!   2. Savings — default match-line mode must still hit the >=60% token-savings floor.
//!
//! HOW IT ROUTES
//! It asks the binary under test `rtk rewrite "<tool> ..."` to learn the real routing
//! (`rtk grep` vs `rtk rg`), so the board automatically tracks the registry split as we
//! land it — no hard-coded mapping.
//!
//! KNOWN-BUG GATING (keeps `cargo test` green while the board is honest)
//! Cases tagged `known_bug` are asserted to be *currently broken*. When a fix lands and
//! such a case starts passing, the gate FAILS on purpose ("appears FIXED — lock it in"),
//! forcing us to drop the tag and make the green permanent. New regressions on untagged
//! cases fail immediately. The report test prints the full table regardless.
//!
//! Run the shareable table:  cargo test --test grep_rg_scoreboard -- --nocapture report
//! With the upstream column:  RTK_UPSTREAM_BIN=/path/to/upstream/rtk cargo test ... report

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Default,
    Count,
    List,
}

#[derive(Clone, Copy)]
struct Case {
    tool: &'static str,            // "grep" | "rg" — what the user typed
    args: &'static [&'static str], // exact user args after the tool (flags, pattern, path)
    pattern: &'static str,         // the regex pattern, in the tool's native dialect
    path: &'static str,            // search path (relative to repo root)
    mode: Mode,
    desc: &'static str,
    compacts: bool, // default-mode case eligible for the savings floor
    known_bug: Option<&'static str>, // Some(doc) while currently broken; None once fixed
}

// Cases search rtk's own codebase (src/) for the realistic savings rows and controlled
// fixtures for the deterministic hazard rows.
const CASES: &[Case] = &[
    // ---- realistic corpus rows (should compact correctly, hit savings floor) ----
    Case {
        tool: "rg",
        args: &["fn ", "src"],
        pattern: "fn ",
        path: "src",
        mode: Mode::Default,
        desc: "rg 'fn ' src (corpus, default match mode)",
        compacts: true,
        known_bug: None,
    },
    Case {
        tool: "grep",
        args: &["-rn", "fn run", "src"],
        pattern: "fn run",
        path: "src",
        mode: Mode::Default,
        desc: "grep -rn 'fn run' src (corpus, default)",
        compacts: false, // few matches: a correctness row, not a compression row
        known_bug: None,
    },
    Case {
        tool: "rg",
        args: &["-n", "alpha", "tests/fixtures/scoreboard/many.txt"],
        pattern: "alpha",
        path: "tests/fixtures/scoreboard/many.txt",
        mode: Mode::Default,
        desc: "rg -n alpha many.txt (single clean match)",
        compacts: false,
        known_bug: None,
    },
    // ---- count mode (resolved + hazards) ----
    Case {
        tool: "grep",
        args: &["-c", "a", "tests/fixtures/scoreboard/many.txt"],
        pattern: "a",
        path: "tests/fixtures/scoreboard/many.txt",
        mode: Mode::Count,
        desc: "grep -c a many.txt (bare count, RESOLVED)",
        compacts: false,
        known_bug: None,
    },
    // FIXED (bug alpha) — rg dialect now forwards verbatim, so `\|` stays a literal pipe
    // (RE2) instead of being rewritten to alternation. truth=1, rtk=1.
    Case {
        tool: "rg",
        args: &["-c", r"foo\|bar", "tests/fixtures/scoreboard/disc.txt"],
        pattern: r"foo\|bar",
        path: "tests/fixtures/scoreboard/disc.txt",
        mode: Mode::Count,
        desc: r"rg -c 'foo\|bar' (rg literal pipe; truth=1, was over-counted)",
        compacts: false,
        known_bug: None,
    },
    // FIXED — grep BRE literal paren now translated to RE2 `rpc\(` instead of crashing.
    Case {
        tool: "grep",
        args: &["-c", "rpc(", "tests/fixtures/scoreboard/paren.txt"],
        pattern: "rpc(",
        path: "tests/fixtures/scoreboard/paren.txt",
        mode: Mode::Count,
        desc: "grep -c 'rpc(' (grep BRE literal paren; truth=2)",
        compacts: false,
        known_bug: None,
    },
    // grep BRE alternation `\|` is the operator → must match both branches (truth=3 lines).
    Case {
        tool: "grep",
        args: &["-c", r"foo\|bar", "tests/fixtures/scoreboard/disc.txt"],
        pattern: r"foo\|bar",
        path: "tests/fixtures/scoreboard/disc.txt",
        mode: Mode::Count,
        desc: r"grep -c 'foo\|bar' (BRE alternation; truth=3)",
        compacts: false,
        known_bug: None,
    },
    // grep -F fixed-strings: paren is literal, no BRE translation, rg -F forwarded.
    Case {
        tool: "grep",
        args: &["-Fc", "rpc(", "tests/fixtures/scoreboard/paren.txt"],
        pattern: "rpc(",
        path: "tests/fixtures/scoreboard/paren.txt",
        mode: Mode::Count,
        desc: "grep -Fc 'rpc(' (fixed strings; truth=2, no translation)",
        compacts: false,
        known_bug: None,
    },
    // FIXED (bug alpha, default mode) — `\|` no longer over-matches; truth=1 line, rtk=1.
    Case {
        tool: "rg",
        args: &[r"foo\|bar", "tests/fixtures/scoreboard/disc.txt"],
        pattern: r"foo\|bar",
        path: "tests/fixtures/scoreboard/disc.txt",
        mode: Mode::Default,
        desc: r"rg 'foo\|bar' (default; literal pipe, truth=1 line)",
        compacts: false,
        known_bug: None,
    },
    // FIXED (bug beta) — rg dialect forwards `-r`/`-E` verbatim (genuine --replace/--encoding)
    // instead of stripping them as grep recursive/ERE. The replacement runs.
    Case {
        tool: "rg",
        args: &["-r", "REPL", "world", "tests/fixtures/scoreboard/rep.txt"],
        pattern: "world",
        path: "tests/fixtures/scoreboard/rep.txt",
        mode: Mode::Default,
        desc: "rg -r REPL world rep.txt (rg --replace, forwarded verbatim)",
        compacts: false,
        known_bug: None,
    },
    // FIXED — `--files` now recognized as a path-list format, passed through unmangled
    // instead of being forced through the match-regroup path.
    Case {
        tool: "rg",
        args: &["--files", "src"],
        pattern: "",
        path: "src",
        mode: Mode::List,
        desc: "rg --files src (path list, passthrough)",
        compacts: false,
        known_bug: None,
    },
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn exec(cmd: &mut Command) -> (String, String, i32) {
    let out = cmd.output().expect("failed to spawn process");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// Ask `bin rewrite "<tool> PROBE ."` what subcommand the tool routes to.
/// Returns the subcommand ("grep"/"rg") or None for passthrough (no rtk filter).
fn route(bin: &str, tool: &str) -> Option<String> {
    let raw = format!("{tool} PROBE_PATTERN .");
    let (stdout, _stderr, code) = exec(Command::new(bin).args(["rewrite", &raw]));
    // exit 1 = passthrough, exit 2 = deny → treat as passthrough for the harness
    if code == 1 || code == 2 {
        return None;
    }
    let head: Vec<&str> = stdout.split_whitespace().take(2).collect();
    match head.as_slice() {
        ["rtk", "rg"] => Some("rg".into()),
        ["rtk", "grep"] => Some("grep".into()),
        _ => None,
    }
}

/// Run the case through the rtk binary the way the hook would route it.
fn run_rtk(bin: &str, case: &Case) -> (String, String, i32) {
    let db = std::env::temp_dir().join("rtk-scoreboard.db");
    match route(bin, case.tool) {
        Some(sub) => exec(
            Command::new(bin)
                .arg(sub)
                .args(case.args)
                .current_dir(repo_root())
                .env("RTK_DB_PATH", &db),
        ),
        // Passthrough → the hook would run the real tool unchanged.
        None => exec(
            Command::new(case.tool)
                .args(case.args)
                .current_dir(repo_root())
                .env("RTK_DISABLED", "1"),
        ),
    }
}

/// Ground truth: the genuine tool with the user's exact args (the oracle + savings base).
fn ground_truth(case: &Case) -> (String, String, i32) {
    exec(
        Command::new(case.tool)
            .args(case.args)
            .current_dir(repo_root())
            .env("RTK_DISABLED", "1"),
    )
}

/// Canonical match set for default mode: run the real tool with normalizing flags so we
/// get flat `(file:)line:content`, then collect the line-number set + total count.
/// Pattern is interpreted in the tool's *native* dialect → a faithful oracle.
fn canonical_default(case: &Case) -> (usize, BTreeSet<usize>) {
    let mut cmd = Command::new(case.tool);
    cmd.arg("-n");
    if case.tool == "rg" {
        cmd.args(["--no-heading", "--no-ignore-vcs"]);
    } else {
        cmd.arg("-r"); // grep recursive to match rg's default recursion
    }
    cmd.arg(case.pattern)
        .arg(case.path)
        .current_dir(repo_root())
        .env("RTK_DISABLED", "1");
    let (stdout, _e, _c) = exec(&mut cmd);
    let lines = parse_line_numbers(&stdout);
    (lines.len(), lines.into_iter().collect())
}

/// Parse line numbers from flat grep/rg output (`file:line:content` or `line:content`).
/// Anchors on the first numeric field; ignores headers and non-match lines.
fn parse_line_numbers(out: &str) -> Vec<usize> {
    let mut v = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        let n = match parts.as_slice() {
            [_f, ln, _rest] if ln.parse::<usize>().is_ok() => ln.parse().ok(),
            [ln, _rest] if ln.parse::<usize>().is_ok() => ln.parse().ok(),
            _ => None,
        };
        if let Some(n) = n {
            v.push(n);
        }
    }
    v
}

/// rtk default output → (shown match lines, [+N more] marker value, line-number set).
fn parse_rtk_default(out: &str) -> (usize, usize, BTreeSet<usize>) {
    let mut shown = 0usize;
    let mut marker = 0usize;
    let mut lines = BTreeSet::new();
    for line in out.lines() {
        let t = line.trim();
        if t.is_empty() || t.contains("matches in") {
            continue;
        }
        if let Some(rest) = t.strip_prefix("[+") {
            if let Some(num) = rest.split_whitespace().next() {
                marker = num.parse().unwrap_or(0);
            }
            continue;
        }
        // `file:line:content` — second field numeric.
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        if let [_f, ln, _rest] = parts.as_slice() {
            if let Ok(n) = ln.parse::<usize>() {
                shown += 1;
                lines.insert(n);
            }
        }
    }
    (shown, marker, lines)
}

fn sum_ints(out: &str) -> i64 {
    let mut sum = 0i64;
    let mut cur = String::new();
    for ch in out.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            sum += cur.parse::<i64>().unwrap_or(0);
            cur.clear();
        }
    }
    if !cur.is_empty() {
        sum += cur.parse::<i64>().unwrap_or(0);
    }
    sum
}

fn path_basenames(out: &str) -> BTreeSet<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains("matches in") && !l.starts_with("[+"))
        .map(|l| l.rsplit('/').next().unwrap_or(l).to_string())
        .collect()
}

fn tokens(s: &str) -> usize {
    s.split_whitespace().count()
}

enum Verdict {
    Ok,        // byte-faithful or trivially equal
    Compacted, // differs from truth but faithful (token-saving)
    Corrupt(String),
}

fn looks_like_error(out: &str, err: &str, code: i32) -> Option<String> {
    let hay = format!("{out}\n{err}").to_lowercase();
    for needle in [
        "regex parse error",
        "unclosed group",
        "unrecognized",
        "no such file",
        "error parsing flag",
        "unknown encoding",
    ] {
        if hay.contains(needle) {
            return Some(needle.to_string());
        }
    }
    if code == 2 {
        return Some(format!("exit code 2: {}", err.trim()));
    }
    None
}

fn judge(case: &Case, gt: &(String, String, i32), rtk: &(String, String, i32)) -> Verdict {
    let (gt_out, _gt_err, gt_code) = gt;
    let (rk_out, rk_err, rk_code) = rtk;
    let gt_ok = *gt_code == 0 || *gt_code == 1; // 1 = no match (not an error)

    // Crash class: rtk errors where the real tool was fine.
    if gt_ok {
        if let Some(reason) = looks_like_error(rk_out, rk_err, *rk_code) {
            return Verdict::Corrupt(format!("rtk errors where truth succeeds ({reason})"));
        }
    }

    match case.mode {
        Mode::Count => {
            let g = sum_ints(gt_out);
            let r = sum_ints(rk_out);
            if g != r {
                return Verdict::Corrupt(format!("count {r} != truth {g}"));
            }
            Verdict::Ok
        }
        Mode::List => {
            if rk_out.contains("matches in") {
                return Verdict::Corrupt("emitted match-summary for a file-list mode".into());
            }
            let g = path_basenames(gt_out);
            let r = path_basenames(rk_out);
            let marker = rk_out
                .lines()
                .find_map(|l| {
                    l.trim()
                        .strip_prefix("[+")
                        .and_then(|s| s.split_whitespace().next())
                        .and_then(|n| n.parse::<usize>().ok())
                })
                .unwrap_or(0);
            if r.difference(&g).next().is_some() {
                return Verdict::Corrupt("listed paths not present in truth".into());
            }
            if r.len() + marker < g.len() {
                return Verdict::Corrupt(format!(
                    "dropped paths without marker: shown {} + {} < truth {}",
                    r.len(),
                    marker,
                    g.len()
                ));
            }
            if r.len() == g.len() {
                Verdict::Ok
            } else {
                Verdict::Compacted
            }
        }
        Mode::Default => {
            let (g_total, g_lines) = canonical_default(case);
            let (shown, marker, rk_lines) = parse_rtk_default(rk_out);
            if shown + marker != g_total {
                return Verdict::Corrupt(format!(
                    "match total {}+{} != truth {}",
                    shown, marker, g_total
                ));
            }
            for ln in &rk_lines {
                if !g_lines.contains(ln) {
                    return Verdict::Corrupt(format!("fabricated match at line {ln}"));
                }
            }
            Verdict::Compacted
        }
    }
}

fn savings(gt: &(String, String, i32), rtk: &(String, String, i32)) -> Option<f64> {
    let g = tokens(&gt.0);
    if g < 40 {
        return None; // too small to meaningfully measure
    }
    let r = tokens(&rtk.0);
    Some(100.0 - (r as f64 / g as f64 * 100.0))
}

fn verdict_label(v: &Verdict) -> String {
    match v {
        Verdict::Ok => "OK".into(),
        Verdict::Compacted => "COMPACTED".into(),
        Verdict::Corrupt(r) => format!("CORRUPT ({r})"),
    }
}

/// GATE: known-bug cases must stay broken (fixing one trips this on purpose); untagged
/// cases must never be CORRUPT and must meet the savings floor.
#[test]
fn scoreboard_gate() {
    if !tool_available("rg") || !tool_available("grep") {
        eprintln!("skipping scoreboard_gate: rg/grep not available");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_rtk");
    let mut failures = Vec::new();

    for case in CASES {
        let gt = ground_truth(case);
        let rtk = run_rtk(bin, case);
        let v = judge(case, &gt, &rtk);
        let is_bad = matches!(v, Verdict::Corrupt(_));

        match (case.known_bug, is_bad) {
            (Some(_), true) => {} // expected broken
            (Some(bug), false) => failures.push(format!(
                "[{}] KNOWN BUG ({bug}) now appears FIXED — drop the known_bug tag to lock it in",
                case.desc
            )),
            (None, true) => {
                failures.push(format!("[{}] REGRESSION: {}", case.desc, verdict_label(&v)))
            }
            (None, false) => {}
        }

        if case.compacts && case.known_bug.is_none() {
            if let Some(s) = savings(&gt, &rtk) {
                if s < 60.0 {
                    failures.push(format!("[{}] savings {:.1}% below 60% floor", case.desc, s));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "scoreboard gate failed:\n  {}",
        failures.join("\n  ")
    );
}

/// REPORT: prints the human-readable table; never fails. The shareable artifact.
/// `cargo test --test grep_rg_scoreboard -- --nocapture report`
#[test]
fn report() {
    if !tool_available("rg") || !tool_available("grep") {
        eprintln!("skipping report: rg/grep not available");
        return;
    }
    let fork = env!("CARGO_BIN_EXE_rtk");
    let upstream = std::env::var("RTK_UPSTREAM_BIN").ok();

    eprintln!("\n=== grep/rg differential scoreboard (truth = real tool) ===");
    eprintln!(
        "upstream column: {}",
        upstream
            .as_deref()
            .unwrap_or("(set RTK_UPSTREAM_BIN to enable)")
    );
    eprintln!(
        "{:<52} {:<10} {:<26} {:<26}",
        "CASE", "SAVE", "FORK", "UPSTREAM"
    );
    eprintln!("{}", "-".repeat(118));

    for case in CASES {
        let gt = ground_truth(case);
        let rtk = run_rtk(fork, case);
        let fork_v = verdict_label(&judge(case, &gt, &rtk));
        let save = savings(&gt, &rtk)
            .map(|s| format!("{s:.0}%"))
            .unwrap_or_else(|| "-".into());
        let up_v = match &upstream {
            Some(bin) if std::path::Path::new(bin).exists() => {
                let up = run_rtk(bin, case);
                verdict_label(&judge(case, &gt, &up))
            }
            _ => "n/a".into(),
        };
        let tag = case.known_bug.map(|_| " [known]").unwrap_or("");
        eprintln!(
            "{:<52} {:<10} {:<26} {:<26}",
            truncate(&format!("{}{}", case.desc, tag), 52),
            save,
            truncate(&fork_v, 26),
            truncate(&up_v, 26)
        );
    }
    eprintln!("{}", "-".repeat(118));
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{t}…")
    }
}
