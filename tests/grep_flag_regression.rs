//! Behavioral regression tests for `rtk grep` flag handling.
//!
//! These drive the built `rtk` binary end-to-end (not just internal helpers), so they
//! catch grep→ripgrep flag-translation regressions that helper-level unit tests can
//! miss — notably the class where rg reuses a grep flag letter for a *value-taking*
//! flag and silently swallows the pattern. Each case names the production bug it guards.
//!
//! Verified red on the pre-fix binary (v0.42.0-algolia.2) and green after the fixes:
//!   - `grep <pat> -rn`      old: match rewritten to "n"  (rg -r = --replace)
//!   - `grep -nE <pat>`      old: clap-fell-back to grep; broke once forwarded to rg
//!   - `grep -l <pat> --type old: "/usr/bin/grep: unrecognized option '--type'"

use std::path::Path;
use std::process::Command;

fn rg_available() -> bool {
    Command::new("rg")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `rtk grep <args>` in `dir`, returning (stdout, stderr, exit_code).
fn rtk_grep(args: &[&str], dir: &Path) -> (String, String, i32) {
    let exe = env!("CARGO_BIN_EXE_rtk");
    let out = Command::new(exe)
        .arg("grep")
        .args(args)
        .current_dir(dir)
        .env("RTK_DB_PATH", dir.join("rtk-test.db")) // isolate tracking from the real DB
        .output()
        .expect("failed to spawn rtk");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// Create an isolated temp dir containing `x.py` with a `def`/identifier to match.
fn fixture(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rtk_grep_regression_{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    std::fs::write(dir.join("x.py"), "def foo():\n    reasoning = 1\n").expect("write fixture");
    dir
}

#[test]
fn grep_recursive_flags_first_do_not_mangle_matches() {
    if !rg_available() {
        eprintln!("rg not installed — skipping");
        return;
    }
    let dir = fixture("rn_first");
    let (stdout, _stderr, _code) = rtk_grep(&["-rn", "def", "."], &dir);
    // `def foo():` must come through verbatim, NOT rewritten to `n foo():`.
    assert!(
        stdout.contains("def foo():"),
        "expected verbatim match, got:\n{stdout}"
    );
    assert!(
        !stdout.contains(":n foo():"),
        "match was mangled (rg --replace leak):\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn grep_recursive_flags_after_pattern_do_not_mangle_matches() {
    // The pre-fix mangling path: pattern first, `-rn` lands in trailing args.
    if !rg_available() {
        eprintln!("rg not installed — skipping");
        return;
    }
    let dir = fixture("rn_after");
    let (stdout, _stderr, _code) = rtk_grep(&["def", ".", "-rn"], &dir);
    assert!(
        stdout.contains("def foo():"),
        "expected verbatim match, got:\n{stdout}"
    );
    assert!(
        !stdout.contains(":n foo():"),
        "match was mangled (rg --replace leak):\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn grep_extended_regex_ere_flag_is_handled() {
    // grep `-E` (ERE) must not be forwarded to rg as `--encoding` (which eats the pattern).
    if !rg_available() {
        eprintln!("rg not installed — skipping");
        return;
    }
    let dir = fixture("ere");
    let (stdout, stderr, _code) = rtk_grep(&["-nE", "def", "x.py"], &dir);
    assert!(
        !stderr.contains("error parsing flag -E") && !stderr.contains("unknown encoding"),
        "rg choked on -E:\n{stderr}"
    );
    assert!(
        stdout.contains("def foo():"),
        "expected match for -E pattern, got:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn grep_redirect_to_file_keeps_all_matches() {
    // Silent data loss: truncating to ~25 rows into a redirected file drops matches the
    // caller treats as complete. A real `> out.txt` redirect must get every match.
    if !rg_available() {
        eprintln!("rg not installed — skipping");
        return;
    }
    let dir = std::env::temp_dir().join("rtk_grep_regression_redirect");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    // 40 matching lines — well over the per-file cap (25).
    let body: String = (0..40).map(|i| format!("needle line {i}\n")).collect();
    std::fs::write(dir.join("many.txt"), body).expect("write fixture");

    let outpath = dir.join("out.txt");
    let exe = env!("CARGO_BIN_EXE_rtk");
    let outfile = std::fs::File::create(&outpath).expect("create redirect target");
    let status = Command::new(exe)
        .arg("grep")
        .args(["needle", "many.txt"])
        .current_dir(&dir)
        .env("RTK_DB_PATH", dir.join("rtk-test.db"))
        .stdout(std::process::Stdio::from(outfile)) // real-file redirect
        .status()
        .expect("spawn rtk");
    assert!(status.success() || status.code() == Some(0));

    let content = std::fs::read_to_string(&outpath).expect("read redirect output");
    let matches = content.lines().filter(|l| l.contains("needle")).count();
    assert_eq!(
        matches, 40,
        "redirect to a file must contain ALL matches, got {matches}:\n{content}"
    );
    assert!(
        !content.contains("[+"),
        "redirect must not contain a truncation marker:\n{content}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn grep_rg_type_and_l_flags_do_not_collide() {
    // `-l` (files-with-matches) + `--type` must work, not error from /usr/bin/grep.
    if !rg_available() {
        eprintln!("rg not installed — skipping");
        return;
    }
    let dir = fixture("type_l");
    let (stdout, stderr, _code) = rtk_grep(&["-l", "reasoning", "--type", "py"], &dir);
    assert!(
        !stderr.contains("unrecognized option") && !stderr.contains("invalid option"),
        "grep flag collision leaked:\n{stderr}"
    );
    assert!(
        stdout.contains("x.py"),
        "expected file-list output, got:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
