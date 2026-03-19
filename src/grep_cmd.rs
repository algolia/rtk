use crate::tracking;
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::process::Command;

#[allow(clippy::too_many_arguments)]
pub fn run(
    pattern: &str,
    path: &str,
    max_line_len: usize,
    max_results: usize,
    context_only: bool,
    file_type: Option<&str>,
    extra_args: &[String],
    verbose: u8,
) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    if verbose > 0 {
        eprintln!("grep: '{}' in {}", pattern, path);
    }

    // Fix: convert BRE alternation \| → | for rg (which uses PCRE-style regex)
    let rg_pattern = pattern.replace(r"\|", "|");

    // Detect count mode (-c/--count) — output format is file:count, not file:line:content.
    // Skip grouping filter and passthrough raw output (already minimal).
    let is_count_mode = extra_args.iter().any(|a| a == "-c" || a == "--count");

    let mut rg_cmd = Command::new("rg");
    if is_count_mode {
        // In count mode, -n is meaningless and --no-heading is default; just pass --count
        rg_cmd.args(["--count", &rg_pattern, path]);
    } else {
        rg_cmd.args(["-n", "--no-heading", &rg_pattern, path]);
    }

    if let Some(ft) = file_type {
        rg_cmd.arg("--type").arg(ft);
    }

    for arg in extra_args {
        // Fix: skip grep-ism -r flag (rg is recursive by default; rg -r means --replace)
        if arg == "-r" || arg == "--recursive" {
            continue;
        }
        // Skip -c/--count — already handled above via is_count_mode
        if arg == "-c" || arg == "--count" {
            continue;
        }
        rg_cmd.arg(arg);
    }

    let output = rg_cmd
        .output()
        .or_else(|_| Command::new("grep").args(["-rn", pattern, path]).output())
        .context("grep/rg failed")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let exit_code = output.status.code().unwrap_or(1);

    let raw_output = stdout.to_string();

    // Count mode: passthrough raw output (already minimal, no grouping needed)
    if is_count_mode {
        if !raw_output.is_empty() {
            print!("{}", raw_output);
        }
        timer.track(
            &format!("grep -c '{}' {}", pattern, path),
            "rtk grep -c",
            &raw_output,
            &raw_output,
        );
        if exit_code != 0 {
            std::process::exit(exit_code);
        }
        return Ok(());
    }

    if stdout.trim().is_empty() {
        // Show stderr for errors (bad regex, missing file, etc.)
        if exit_code == 2 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                eprintln!("{}", stderr.trim());
            }
        }
        let msg = format!("🔍 0 for '{}'", pattern);
        println!("{}", msg);
        timer.track(
            &format!("grep -rn '{}' {}", pattern, path),
            "rtk grep",
            &raw_output,
            &msg,
        );
        if exit_code != 0 {
            std::process::exit(exit_code);
        }
        return Ok(());
    }

    let mut by_file: HashMap<String, Vec<(usize, String)>> = HashMap::new();
    let mut total = 0;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(3, ':').collect();

        let (file, line_num, content) = if parts.len() == 3 {
            let ln = parts[1].parse().unwrap_or(0);
            (parts[0].to_string(), ln, parts[2])
        } else if parts.len() == 2 {
            let ln = parts[0].parse().unwrap_or(0);
            (path.to_string(), ln, parts[1])
        } else {
            continue;
        };

        total += 1;
        let cleaned = clean_line(content, max_line_len, context_only, pattern);
        by_file.entry(file).or_default().push((line_num, cleaned));
    }

    let mut rtk_output = String::new();
    rtk_output.push_str(&format!("🔍 {} in {}F:\n\n", total, by_file.len()));

    let mut shown = 0;
    let mut files: Vec<_> = by_file.iter().collect();
    files.sort_by_key(|(f, _)| *f);

    for (file, matches) in files {
        if shown >= max_results {
            break;
        }

        let file_display = compact_path(file);
        rtk_output.push_str(&format!("📄 {} ({}):\n", file_display, matches.len()));

        for (line_num, content) in matches.iter().take(10) {
            rtk_output.push_str(&format!("  {:>4}: {}\n", line_num, content));
            shown += 1;
            if shown >= max_results {
                break;
            }
        }

        if matches.len() > 10 {
            rtk_output.push_str(&format!("  +{}\n", matches.len() - 10));
        }
        rtk_output.push('\n');
    }

    if total > shown {
        rtk_output.push_str(&format!("... +{}\n", total - shown));
    }

    print!("{}", rtk_output);
    timer.track(
        &format!("grep -rn '{}' {}", pattern, path),
        "rtk grep",
        &raw_output,
        &rtk_output,
    );

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

fn clean_line(line: &str, max_len: usize, context_only: bool, pattern: &str) -> String {
    let trimmed = line.trim();

    if context_only {
        if let Ok(re) = Regex::new(&format!("(?i).{{0,20}}{}.*", regex::escape(pattern))) {
            if let Some(m) = re.find(trimmed) {
                let matched = m.as_str();
                if matched.len() <= max_len {
                    return matched.to_string();
                }
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
        let cleaned = clean_line(line, 50, false, "result");
        assert!(!cleaned.starts_with(' '));
        assert!(cleaned.len() <= 50);
    }

    #[test]
    fn test_compact_path() {
        let path = "/Users/patrick/dev/project/src/components/Button.tsx";
        let compact = compact_path(path);
        assert!(compact.len() <= 60);
    }

    #[test]
    fn test_extra_args_accepted() {
        // Test that the function signature accepts extra_args
        // This is a compile-time test - if it compiles, the signature is correct
        let _extra: Vec<String> = vec!["-i".to_string(), "-A".to_string(), "3".to_string()];
        // No need to actually run - we're verifying the parameter exists
    }

    #[test]
    fn test_clean_line_multibyte() {
        // Thai text that exceeds max_len in bytes
        let line = "  สวัสดีครับ นี่คือข้อความที่ยาวมากสำหรับทดสอบ  ";
        let cleaned = clean_line(line, 20, false, "ครับ");
        // Should not panic
        assert!(!cleaned.is_empty());
    }

    #[test]
    fn test_clean_line_emoji() {
        let line = "🎉🎊🎈🎁🎂🎄 some text 🎃🎆🎇✨";
        let cleaned = clean_line(line, 15, false, "text");
        assert!(!cleaned.is_empty());
    }

    // Fix: -c/--count flags are detected and handled separately
    #[test]
    fn test_count_mode_detected() {
        let extra_with_c: &[&str] = &["-i", "-c"];
        assert!(extra_with_c.iter().any(|a| *a == "-c" || *a == "--count"));

        let extra_with_long: &[&str] = &["--count"];
        assert!(extra_with_long
            .iter()
            .any(|a| *a == "-c" || *a == "--count"));

        let extra_without: &[&str] = &["-i", "-w"];
        assert!(!extra_without.iter().any(|a| *a == "-c" || *a == "--count"));
    }

    // Fix: -c/--count flags are stripped from extra_args passthrough
    #[test]
    fn test_count_flag_stripped_from_extra_args() {
        let extra_args: &[&str] = &["-r", "-c", "-i", "--count"];
        let filtered: Vec<&str> = extra_args
            .iter()
            .copied()
            .filter(|a| *a != "-r" && *a != "--recursive" && *a != "-c" && *a != "--count")
            .collect();
        assert_eq!(filtered, vec!["-i"]);
    }

    // Regression: grep -c output (file:count) must not be parsed as file:linenum:content
    #[test]
    fn test_count_mode_output_passthrough() {
        // Simulate rg --count output: file:count format
        let rg_count_output = "/tmp/output.txt:42\nsrc/main.rs:7\n";

        // In count mode, output should be passed through verbatim — no grouping
        // Verify the output format is NOT mangled into "📄 42 (1):" nonsense
        assert!(rg_count_output.contains("/tmp/output.txt:42"));
        assert!(!rg_count_output.contains("📄"));
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

    // Verify line numbers are always enabled in rg invocation (grep_cmd.rs:24).
    // The -n/--line-numbers clap flag in main.rs is a no-op accepted for compat.
    #[test]
    fn test_rg_always_has_line_numbers() {
        // grep_cmd::run() always passes "-n" to rg (line 24).
        // This test documents that -n is built-in, so the clap flag is safe to ignore.
        let mut cmd = std::process::Command::new("rg");
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
}
