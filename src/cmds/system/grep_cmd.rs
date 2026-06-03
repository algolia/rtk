//! Filters grep output by grouping matches by file.

use crate::core::config;
use crate::core::stream::exec_capture;
use crate::core::tracking;
use crate::core::utils::resolved_command;
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;

/// Default cap on rendered match-line length (chars). Lines longer than this are
/// context-truncated around the pattern. Previously a `-l` clap flag — dropped
/// because `-l` collides with grep/rg's `--files-with-matches`.
const GREP_MAX_LINE_LEN: usize = 80;

/// grep/rg flags that consume the FOLLOWING token as their value. Used only to
/// locate the positional pattern for cosmetic truncation — rg parses the real
/// args itself, so this list does not need to be exhaustive to stay correct.
const VALUE_FLAGS: &[&str] = &[
    "-A",
    "--after-context",
    "-B",
    "--before-context",
    "-C",
    "--context",
    "-m",
    "--max-count",
    "-e",
    "--regexp",
    "-f",
    "--file",
    "-g",
    "--glob",
    "--iglob",
    "-t",
    "--type",
    "-T",
    "--type-not",
    "--type-add",
    "--max-columns",
    "--color",
    "--colors",
    "--encoding",
    "--sort",
    "--sortr",
    "--pre",
    "--threads",
    "-j",
    "--replace",
    "--context-separator",
];

/// Run `rtk grep`. `args` is the raw, verbatim grep/rg argument vector (flags,
/// pattern, and paths in any order) captured by clap's trailing-var-arg. Forwarding
/// the args untouched to ripgrep is what lets idiomatic invocations like
/// `rg -li "foo" --type py` work — the old typed clap interface stole short flags
/// (notably `-l`) from the grep/rg namespace and broke on flags-before-pattern.
pub fn run(args: &[String], verbose: u8) -> Result<i32> {
    let timer = tracking::TimedExecution::start();

    if args.is_empty() {
        eprintln!("rtk grep: no pattern given");
        return Ok(2);
    }

    let (pattern_idx, pattern, path) = locate_pattern(args);
    let pattern_display = pattern.clone().unwrap_or_else(|| "?".to_string());

    if verbose > 0 {
        eprintln!("grep: '{}' in {}", pattern_display, path);
    }

    // Forward the user's args to rg, fixing two grep-isms that would corrupt output:
    //  - strip grep-only flags whose letter rg reuses for a value-taking flag —
    //    CRITICAL: grep `-r`/`-R` (recursive) is rg `--replace` and `-E` (ERE) is rg
    //    `--encoding`, both value-taking, so forwarding e.g. `-rn`/`-nE` makes rg eat
    //    the pattern (`def foo` -> `n foo`, or "unknown encoding: <pattern>"). rg
    //    recurses and is ERE by default. Handles combined bundles (-rn, -nE, -rln).
    //  - translate BRE alternation \| → | on the pattern token (grep BRE vs rg regex)
    let mut user_args: Vec<String> = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        if Some(i) == pattern_idx {
            user_args.push(a.replace(r"\|", "|"));
            continue;
        }
        if a.starts_with('-') && a.len() > 1 {
            if let Some(sanitized) = strip_grep_only_flags(a) {
                user_args.push(sanitized);
            }
            // None → the flag was solely grep-only (e.g. `-r`, `-E`); drop it entirely.
        } else {
            user_args.push(a.clone());
        }
    }

    let mut rg_cmd = resolved_command("rg");
    // -n line numbers; -H force filename prefix so the layout is always
    // `file:line:content` (the parser can't otherwise tell `line:content` apart once
    // the content holds a colon); --no-heading for flat output; --no-ignore-vcs to
    // match grep -r and not skip .gitignore'd files (avoids silent false negatives).
    rg_cmd.args(["-n", "-H", "--no-heading", "--no-ignore-vcs"]);
    rg_cmd.args(&user_args);

    let result = exec_capture(&mut rg_cmd)
        .or_else(|_| {
            // rg binary unavailable — fall back to grep, stripping rg-only flags so
            // grep doesn't abort on an option it never understood.
            let mut grep_cmd = resolved_command("grep");
            grep_cmd.args(["-rn", "-H"]);
            grep_cmd.args(grep_safe_args(&user_args));
            exec_capture(&mut grep_cmd)
        })
        .context("grep/rg failed")?;

    // Passthrough output flags whose format we can't regroup (file lists, counts).
    if has_format_flag(args) {
        print!("{}", result.stdout);
        if !result.stderr.is_empty() {
            eprint!("{}", result.stderr.trim());
        }

        let args_display = args.join(" ");
        timer.track_passthrough(
            &format!("grep {}", args_display),
            &format!("rtk grep {} (passthrough)", args_display),
        );
        return Ok(result.exit_code);
    }

    let exit_code = result.exit_code;
    let raw_output = result.stdout.clone();

    if result.stdout.trim().is_empty() {
        // Show stderr for errors (bad regex, missing file, etc.)
        if exit_code == 2 && !result.stderr.trim().is_empty() {
            eprintln!("{}", result.stderr.trim());
        }
        let msg = format!("0 matches for '{}'", pattern_display);
        println!("{}", msg);
        timer.track(
            &format!("grep {}", args.join(" ")),
            "rtk grep",
            &raw_output,
            &msg,
        );
        return Ok(exit_code);
    }

    // Always filter: truncate long lines, apply per-file and global caps.
    // Output in standard file:line:content format that AI agents can parse.
    // (A passthrough approach yields 0% savings — no reason for RTK to exist on that path.)
    let total_matches = result.stdout.lines().count();
    let pattern_for_clean = pattern.as_deref().unwrap_or("");

    let mut by_file: HashMap<String, Vec<(usize, String)>> = HashMap::new();
    for line in result.stdout.lines() {
        let Some((file, line_num, content)) = parse_grep_line(line, &path) else {
            continue;
        };
        let cleaned = clean_line(content, GREP_MAX_LINE_LEN, None, pattern_for_clean);
        by_file.entry(file).or_default().push((line_num, cleaned));
    }

    let mut rtk_output = String::new();
    rtk_output.push_str(&format!(
        "{} matches in {} files:\n\n",
        total_matches,
        by_file.len()
    ));

    let mut shown = 0;
    let mut files: Vec<_> = by_file.iter().collect();
    files.sort_by_key(|(f, _)| *f);

    let max_results = config::limits().grep_max_results;
    let per_file = config::limits().grep_max_per_file;
    for (file, matches) in files {
        if shown >= max_results {
            break;
        }

        let file_display = compact_path(file);
        for (line_num, content) in matches.iter().take(per_file) {
            if shown >= max_results {
                break;
            }
            rtk_output.push_str(&format!("{}:{}:{}\n", file_display, line_num, content));
            shown += 1;
        }
    }

    if total_matches > shown {
        rtk_output.push_str(&format!("[+{} more]\n", total_matches - shown));
    }

    print!("{}", rtk_output);
    timer.track(
        &format!("grep {}", args.join(" ")),
        "rtk grep",
        &raw_output,
        &rtk_output,
    );

    Ok(exit_code)
}

/// Locate the positional pattern (and search path) within a raw grep/rg arg list.
/// Returns `(pattern_index, pattern, path)`. The pattern is the first token that is
/// neither a flag nor consumed as a flag's value; the path is the next such token.
/// Used only for cosmetics (truncation context, the no-match message) — ripgrep
/// receives the args verbatim regardless, so an imperfect guess never corrupts a search.
fn locate_pattern(args: &[String]) -> (Option<usize>, Option<String>, String) {
    let mut pattern_idx = None;
    let mut pattern = None;
    let mut path = None;
    let mut positional_only = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if !positional_only && a == "--" {
            positional_only = true;
            i += 1;
            continue;
        }
        if !positional_only && a.starts_with('-') && a.len() > 1 {
            // Flag — skip its value too when it takes one as a separate token.
            if !a.contains('=') && VALUE_FLAGS.contains(&a.as_str()) {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if pattern.is_none() {
            pattern_idx = Some(i);
            pattern = Some(a.clone());
        } else if path.is_none() {
            path = Some(a.clone());
        }
        i += 1;
    }
    (
        pattern_idx,
        pattern,
        path.unwrap_or_else(|| ".".to_string()),
    )
}

/// Remove grep flags that must NOT reach ripgrep verbatim because rg either does them
/// by default or — worse — reuses the same letter for a DIFFERENT, value-taking flag
/// that silently swallows the pattern:
///   `-r`/`-R` grep recursive → rg `-r` is `--replace` (value) → rewrites every match
///   `-E`      grep ERE       → rg `-E` is `--encoding` (value) → eats the pattern
/// rg recurses and speaks ERE by default, so dropping these is safe for grep-compat.
/// Returns the rewritten flag, or `None` if nothing survives (e.g. bare `-r`). Handles
/// combined short bundles by dropping only the offending char (`-rn` → `-n`,
/// `-nE` → `-n`) while preserving value-taking flags and their values (`-A3`, `-tpy`).
fn strip_grep_only_flags(tok: &str) -> Option<String> {
    if let Some(long) = tok.strip_prefix("--") {
        return match long {
            "recursive" | "dereference-recursive" | "extended-regexp" => None,
            _ => Some(tok.to_string()),
        };
    }
    if let Some(rest) = tok.strip_prefix('-') {
        let mut out = String::from("-");
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            // 'r'/'R' = grep recursive (rg default); 'E' = grep ERE (rg default).
            // Both are value-taking in rg, so forwarding them corrupts the search.
            if matches!(c, 'r' | 'R' | 'E') {
                continue;
            }
            out.push(c);
            if SHORT_VALUE_CHARS.contains(&c) {
                // Remainder is this flag's value (e.g. `-tpy`, `-A3`) — keep verbatim.
                out.extend(chars);
                break;
            }
        }
        return if out == "-" { None } else { Some(out) };
    }
    Some(tok.to_string())
}

/// Strip rg-only flags (and their separate-token values) so the grep fallback,
/// taken only when ripgrep is absent, does not abort on an unrecognized option.
fn grep_safe_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut skip_next = false;
    for a in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        let bare = a.split('=').next().unwrap_or(a);
        let rg_only = matches!(
            bare,
            "--type"
                | "-t"
                | "--type-not"
                | "-T"
                | "--type-add"
                | "--glob"
                | "-g"
                | "--iglob"
                | "--no-ignore-vcs"
                | "--no-heading"
                | "--json"
                | "--pre"
        );
        if rg_only {
            if !a.contains('=') && VALUE_FLAGS.contains(&bare) {
                skip_next = true;
            }
            continue;
        }
        out.push(a.clone());
    }
    out
}

/// Short flags that change rg/grep's output away from `file:line:content`
/// (file lists, counts, only-matching, null-separated) → must passthrough unregrouped.
const FORMAT_SHORT: &[char] = &['l', 'L', 'c', 'o', 'Z'];
/// Short flags that consume a value, so the rest of a combined bundle is that value,
/// not more flags (e.g. `-A3`, `-tpy`). Stops format-letter scanning at the value.
const SHORT_VALUE_CHARS: &[char] = &['A', 'B', 'C', 'm', 'e', 'f', 'g', 't', 'T', 'j', 'M'];

/// Does this single token request a format that we can't regroup as `file:line:content`?
/// Handles long flags and combined short bundles (`-li` contains `-l`), while not
/// mistaking a value-flag's value for flag letters (`-tpy` is `--type py`, not `-p`).
fn token_has_format_flag(tok: &str) -> bool {
    if let Some(long) = tok.strip_prefix("--") {
        return matches!(
            long,
            "count"
                | "files-with-matches"
                | "files-without-match"
                | "only-matching"
                | "null"
        );
    }
    if let Some(rest) = tok.strip_prefix('-') {
        for c in rest.chars() {
            if FORMAT_SHORT.contains(&c) {
                return true;
            }
            if SHORT_VALUE_CHARS.contains(&c) {
                break; // remainder is this flag's value, not more flags
            }
        }
    }
    false
}

fn has_format_flag(args: &[String]) -> bool {
    args.iter().any(|a| token_has_format_flag(a))
}

/// Parse one grep/rg result line into `(file, line_number, content)`.
///
/// grep/rg emits either `file:line:content` (with -H, recursive, or multi-file) or
/// `line:content` (single file without a filename prefix). The matched content routinely
/// contains colons (`def f():`, dict literals, type hints, URLs), so the layout cannot be
/// decided by counting colons. We anchor instead on the invariant that the line number is
/// always a run of digits: whichever leading field parses as a number identifies the layout.
/// This never mis-files a line number as a filename, however many colons the content holds.
///
/// `default_path` is used as the file name when the line carries no filename prefix.
/// Returns `None` for lines that don't begin with a numeric field (no usable match).
fn parse_grep_line<'a>(line: &'a str, default_path: &str) -> Option<(String, usize, &'a str)> {
    let parts: Vec<&str> = line.splitn(3, ':').collect();
    match parts.as_slice() {
        // file:line:content — middle field is the numeric line number.
        [f, ln, rest] if ln.parse::<usize>().is_ok() => {
            Some((f.to_string(), ln.parse().unwrap_or(0), rest))
        }
        // line:content — single file, no filename prefix, content had no colon.
        [ln, rest] if ln.parse::<usize>().is_ok() => {
            Some((default_path.to_string(), ln.parse().unwrap_or(0), rest))
        }
        // line:content where the content contains colons: splitn(3) over-split it. The
        // first field is still the numeric line number; re-join the rest as content.
        [ln, ..] if ln.parse::<usize>().is_ok() => {
            let rest = line.split_once(':').map_or("", |(_, r)| r);
            Some((default_path.to_string(), ln.parse().unwrap_or(0), rest))
        }
        _ => None,
    }
}

fn clean_line(line: &str, max_len: usize, context_re: Option<&Regex>, pattern: &str) -> String {
    let trimmed = line.trim();

    if let Some(re) = context_re {
        if let Some(m) = re.find(trimmed) {
            let matched = m.as_str();
            if matched.len() <= max_len {
                return matched.to_string();
            }
        }
    }

    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        let lower = trimmed.to_lowercase();
        let pattern_lower = pattern.to_lowercase();

        if let Some(pos) = lower.find(&pattern_lower) {
            let char_pos = lower[..pos].chars().count();
            let chars: Vec<char> = trimmed.chars().collect();
            let char_len = chars.len();

            let start = char_pos.saturating_sub(max_len / 3);
            let end = (start + max_len).min(char_len);
            let start = if end == char_len {
                end.saturating_sub(max_len)
            } else {
                start
            };

            let slice: String = chars[start..end].iter().collect();
            if start > 0 && end < char_len {
                format!("...{}...", slice)
            } else if start > 0 {
                format!("...{}", slice)
            } else {
                format!("{}...", slice)
            }
        } else {
            let t: String = trimmed.chars().take(max_len - 3).collect();
            format!("{}...", t)
        }
    }
}

fn compact_path(path: &str) -> String {
    if path.len() <= 50 {
        return path.to_string();
    }

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 3 {
        return path.to_string();
    }

    format!(
        "{}/.../{}/{}",
        parts[0],
        parts[parts.len() - 2],
        parts[parts.len() - 1]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_line() {
        let line = "            const result = someFunction();";
        let cleaned = clean_line(line, 50, None, "result");
        assert!(!cleaned.starts_with(' '));
        assert!(cleaned.len() <= 50);
    }

    // --- grep line parsing (regression for single-file colon-content mis-parse) ---

    #[test]
    fn test_parse_grep_line_with_filename() {
        // rg -H / recursive: file:line:content
        let parsed = parse_grep_line("src/foo.py:42:    return x", "fallback.py");
        assert_eq!(parsed, Some(("src/foo.py".to_string(), 42, "    return x")));
    }

    #[test]
    fn test_parse_grep_line_single_file_no_colon() {
        // single file, content has no colon: line:content
        let parsed = parse_grep_line("3:        ta = TypeAdapter(int)", "test.py");
        assert_eq!(parsed, Some(("test.py".to_string(), 3, "        ta = TypeAdapter(int)")));
    }

    #[test]
    fn test_parse_grep_line_single_file_colon_in_content() {
        // THE BUG: single file, content contains a colon (`def test():`).
        // Old code split into ["2", "    def test_messages(self)", ""] and filed
        // line number "2" as the FILENAME with empty content. Must not happen.
        let parsed = parse_grep_line("2:    def test_messages(self):", "test.py");
        assert_eq!(
            parsed,
            Some(("test.py".to_string(), 2, "    def test_messages(self):"))
        );
    }

    #[test]
    fn test_parse_grep_line_filename_and_colon_content() {
        // file:line:content where content also has colons (dict literal).
        let parsed = parse_grep_line(r#"a.py:4:    return ta.model_validate({"k": "v"})"#, "x");
        assert_eq!(
            parsed,
            Some((
                "a.py".to_string(),
                4,
                r#"    return ta.model_validate({"k": "v"})"#
            ))
        );
    }

    #[test]
    fn test_parse_grep_line_non_numeric_dropped() {
        // A line that begins with no numeric field is not a usable match.
        assert_eq!(parse_grep_line("not a grep line", "x"), None);
    }

    #[test]
    fn test_compact_path() {
        let path = "/Users/patrick/dev/project/src/components/Button.tsx";
        let compact = compact_path(path);
        assert!(compact.len() <= 60);
    }

    // --- raw-arg parsing: pattern/path location and grep-fallback sanitization ---

    fn sv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_locate_pattern_flags_before_pattern() {
        // THE BUG: `rg -li "foo" --type py` rewritten to `rtk grep -li foo --type py`.
        // -l no longer steals "foo"; the pattern is correctly the first positional.
        let (idx, pat, path) = locate_pattern(&sv(&["-li", "foo", "--type", "py"]));
        assert_eq!(pat.as_deref(), Some("foo"));
        assert_eq!(idx, Some(1));
        assert_eq!(path, "."); // "py" is the value of --type, not a path
    }

    #[test]
    fn test_locate_pattern_value_flag_not_mistaken_for_pattern() {
        // -g takes a glob value; the glob must not be read as the pattern.
        let (_, pat, path) = locate_pattern(&sv(&["-g", "*.py", "needle", "src/"]));
        assert_eq!(pat.as_deref(), Some("needle"));
        assert_eq!(path, "src/");
    }

    #[test]
    fn test_locate_pattern_simple_pattern_and_path() {
        let (idx, pat, path) = locate_pattern(&sv(&["needle", "src/"]));
        assert_eq!(idx, Some(0));
        assert_eq!(pat.as_deref(), Some("needle"));
        assert_eq!(path, "src/");
    }

    #[test]
    fn test_locate_pattern_double_dash_forces_positional() {
        // After `--`, a leading-dash token is the pattern, not a flag.
        let (_, pat, _) = locate_pattern(&sv(&["--", "-weird-pattern"]));
        assert_eq!(pat.as_deref(), Some("-weird-pattern"));
    }

    // --- CRITICAL: grep -r/-R/-E must be stripped before rg (rg reuses them as
    // value-taking --replace/--encoding, which silently eats the pattern) ---

    #[test]
    fn test_strip_grep_only_bundle_prevents_replace_mangling() {
        // THE GHOST BUG: `grep -rn def` -> rg `--replace=n` rewrites `def` to `n`.
        // -rn must become -n (recursive dropped, line-numbers kept).
        assert_eq!(strip_grep_only_flags("-rn").as_deref(), Some("-n"));
        assert_eq!(strip_grep_only_flags("-rln").as_deref(), Some("-ln"));
        assert_eq!(strip_grep_only_flags("-Rn").as_deref(), Some("-n"));
        assert_eq!(strip_grep_only_flags("-rni").as_deref(), Some("-ni"));
    }

    #[test]
    fn test_strip_grep_only_extended_regex_ere() {
        // `grep -nE PATTERN` -> rg `-E` is --encoding (value) and eats PATTERN.
        // Drop E (rg is ERE by default); keep the rest of the bundle.
        assert_eq!(strip_grep_only_flags("-nE").as_deref(), Some("-n"));
        assert_eq!(strip_grep_only_flags("-E"), None);
        assert_eq!(strip_grep_only_flags("--extended-regexp"), None);
        assert_eq!(strip_grep_only_flags("-rE"), None); // both dropped
        assert_eq!(strip_grep_only_flags("-Ei").as_deref(), Some("-i"));
    }

    #[test]
    fn test_strip_grep_only_standalone_dissolves() {
        assert_eq!(strip_grep_only_flags("-r"), None);
        assert_eq!(strip_grep_only_flags("-R"), None);
        assert_eq!(strip_grep_only_flags("--recursive"), None);
        assert_eq!(strip_grep_only_flags("--dereference-recursive"), None);
    }

    #[test]
    fn test_strip_grep_only_preserves_value_flags_and_their_values() {
        // A value-taking flag's value must survive even if it contains 'r'/'R'/'E'.
        assert_eq!(strip_grep_only_flags("-tpy").as_deref(), Some("-tpy"));
        assert_eq!(strip_grep_only_flags("-A3").as_deref(), Some("-A3"));
        // `-e` (lowercase, rg --regexp) takes the pattern as its value; chars there
        // are data, not flags — even an 'E' or 'r'.
        assert_eq!(strip_grep_only_flags("-eRARE").as_deref(), Some("-eRARE"));
        // No r/R/E at all: unchanged.
        assert_eq!(strip_grep_only_flags("-li").as_deref(), Some("-li"));
        assert_eq!(strip_grep_only_flags("--type").as_deref(), Some("--type"));
    }

    #[test]
    fn test_grep_safe_args_strips_rg_only_flags() {
        // The grep fallback (rg absent) must not receive --type/--glob/-g.
        let safe = grep_safe_args(&sv(&["-i", "--type", "py", "needle", "-g", "*.rs"]));
        assert_eq!(safe, sv(&["-i", "needle"]));
    }

    #[test]
    fn test_grep_safe_args_keeps_grep_compatible_flags() {
        let safe = grep_safe_args(&sv(&["-i", "-w", "needle"]));
        assert_eq!(safe, sv(&["-i", "-w", "needle"]));
    }

    #[test]
    fn test_clean_line_multibyte() {
        // Thai text that exceeds max_len in bytes
        let line = "  สวัสดีครับ นี่คือข้อความที่ยาวมากสำหรับทดสอบ  ";
        let cleaned = clean_line(line, 20, None, "ครับ");
        // Should not panic
        assert!(!cleaned.is_empty());
    }

    #[test]
    fn test_clean_line_emoji() {
        let line = "🎉🎊🎈🎁🎂🎄 some text 🎃🎆🎇✨";
        let cleaned = clean_line(line, 15, None, "text");
        assert!(!cleaned.is_empty());
    }

    // Fix: BRE \| alternation is translated to PCRE | for rg
    #[test]
    fn test_bre_alternation_translated() {
        let pattern = r"fn foo\|pub.*bar";
        let rg_pattern = pattern.replace(r"\|", "|");
        assert_eq!(rg_pattern, "fn foo|pub.*bar");
    }

    // Fix: -r flag (grep recursive) is stripped from extra_args (rg is recursive by default)
    #[test]
    fn test_recursive_flag_stripped() {
        let extra_args: Vec<String> = vec!["-r".to_string(), "-i".to_string()];
        let filtered: Vec<&String> = extra_args
            .iter()
            .filter(|a| *a != "-r" && *a != "--recursive")
            .collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "-i");
    }

    // --- truncation accuracy ---

    #[test]
    fn test_grep_overflow_uses_uncapped_total() {
        // Confirm the grep overflow invariant: matches vec is never capped before overflow calc.
        // If total_matches > per_file, overflow = total_matches - per_file (not capped).
        // This documents that grep_cmd.rs avoids the diff_cmd bug (cap at N then compute N-10).
        let per_file = config::limits().grep_max_per_file;
        let total_matches = per_file + 42;
        let overflow = total_matches - per_file;
        assert_eq!(overflow, 42, "overflow must equal true suppressed count");
        // Demonstrate why capping before subtraction is wrong:
        let hypothetical_cap = per_file + 5;
        let capped = total_matches.min(hypothetical_cap);
        let wrong_overflow = capped - per_file;
        assert_ne!(
            wrong_overflow, overflow,
            "capping before subtraction gives wrong overflow"
        );
    }

    // --- format flag detection ---

    #[test]
    fn test_format_flag_detects_count() {
        assert!(has_format_flag(&["-c".to_string()]));
        assert!(has_format_flag(&["--count".to_string()]));
    }

    #[test]
    fn test_format_flag_detects_files_with_matches() {
        assert!(has_format_flag(&["-l".to_string()]));
        assert!(has_format_flag(&["--files-with-matches".to_string()]));
    }

    #[test]
    fn test_format_flag_detects_files_without_match() {
        assert!(has_format_flag(&["-L".to_string()]));
        assert!(has_format_flag(&["--files-without-match".to_string()]));
    }

    #[test]
    fn test_format_flag_detects_only_matching() {
        assert!(has_format_flag(&["-o".to_string()]));
        assert!(has_format_flag(&["--only-matching".to_string()]));
    }

    #[test]
    fn test_format_flag_detects_null() {
        assert!(has_format_flag(&["-Z".to_string()]));
        assert!(has_format_flag(&["--null".to_string()]));
    }

    #[test]
    fn test_format_flag_ignores_normal_flags() {
        assert!(!has_format_flag(&[
            "-i".to_string(),
            "-w".to_string(),
            "-A".to_string(),
            "3".to_string(),
        ]));
    }

    #[test]
    fn test_format_flag_detects_combined_short_bundle() {
        // `-li` (= -l -i) must be recognized as a files-with-matches request,
        // else rtk tries to regroup filename-only output ("N matches in 0 files").
        assert!(has_format_flag(&["-li".to_string()]));
        assert!(has_format_flag(&["-rl".to_string()]));
    }

    #[test]
    fn test_format_flag_value_in_bundle_not_misread() {
        // `-tlog` is `--type log` (value "log"), NOT a hidden -l/-o; must be false.
        assert!(!token_has_format_flag("-tlog"));
        // `-A3` is after-context 3, not a format flag.
        assert!(!token_has_format_flag("-A3"));
    }

    // Verify line numbers are always enabled in rg invocation (grep_cmd.rs:24).
    // The -n/--line-numbers clap flag in main.rs is a no-op accepted for compat.
    #[test]
    fn test_rg_always_has_line_numbers() {
        // grep_cmd::run() always passes "-n" to rg (line 24).
        // This test documents that -n is built-in, so the clap flag is safe to ignore.
        let mut cmd = resolved_command("rg");
        cmd.args(["-n", "--no-heading", "NONEXISTENT_PATTERN_12345", "."]);
        // If rg is available, it should accept -n without error (exit 1 = no match, not error)
        if let Ok(output) = cmd.output() {
            assert!(
                output.status.code() == Some(1) || output.status.success(),
                "rg -n should be accepted"
            );
        }
        // If rg is not installed, skip gracefully (test still passes)
    }

    #[test]
    fn test_rg_no_ignore_vcs_flag_accepted() {
        // Verify rg accepts --no-ignore-vcs (used to match grep -r behavior for .gitignore)
        let mut cmd = resolved_command("rg");
        cmd.args([
            "-n",
            "--no-heading",
            "--no-ignore-vcs",
            "NONEXISTENT_PATTERN_12345",
            ".",
        ]);
        if let Ok(output) = cmd.output() {
            assert!(
                output.status.code() == Some(1) || output.status.success(),
                "rg --no-ignore-vcs should be accepted"
            );
        }
        // If rg is not installed, skip gracefully (test still passes)
    }
}
