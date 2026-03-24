use crate::tracking;
use crate::utils::{resolved_command, truncate};
use anyhow::{Context, Result};

const MAX_JSON_LINES: usize = 80;
const MAX_TEXT_LINES: usize = 30;

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();
    let mut cmd = resolved_command("curl");
    cmd.arg("-s"); // Silent mode (no progress bar)

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: curl -s {}", args.join(" "));
    }

    let output = cmd.output().context("Failed to run curl")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let msg = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        eprintln!("FAILED: curl {}", msg);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    let raw = stdout.to_string();

    // Auto-detect JSON and pipe through filter
    let filtered = filter_curl_output(&stdout);
    println!("{}", filtered);

    timer.track(
        &format!("curl {}", args.join(" ")),
        &format!("rtk curl {}", args.join(" ")),
        &raw,
        &filtered,
    );

    Ok(())
}

fn filter_curl_output(output: &str) -> String {
    let trimmed = output.trim();

    // Try JSON detection: pretty-print with value preservation
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && (trimmed.ends_with('}') || trimmed.ends_with(']'))
    {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Ok(pretty) = serde_json::to_string_pretty(&val) {
                return truncate_lines(&pretty, MAX_JSON_LINES);
            }
        }
    }

    // Not JSON: truncate long output
    truncate_lines(trimmed, MAX_TEXT_LINES)
}

/// Truncate output to `max` lines, appending a summary if exceeded.
fn truncate_lines(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max {
        return lines
            .iter()
            .map(|l| truncate(l, 200))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut result: Vec<&str> = lines[..max].to_vec();
    result.push("");
    let msg = format!(
        "... ({} more lines, {} bytes total)",
        lines.len() - max,
        text.len()
    );
    format!("{}\n{}", result.join("\n"), msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_curl_json_preserves_values() {
        let output = r#"{"name": "Alice", "count": 42, "items": [1, 2, 3]}"#;
        let result = filter_curl_output(output);
        // Values must be preserved (not replaced with type names)
        assert!(result.contains("Alice"), "actual string value preserved");
        assert!(result.contains("42"), "actual number value preserved");
        assert!(!result.contains(": string"), "no schema types");
        assert!(!result.contains(": int"), "no schema types");
    }

    #[test]
    fn test_filter_curl_json_array_preserves_values() {
        let output = r#"[{"id": 1, "name": "foo"}, {"id": 2, "name": "bar"}]"#;
        let result = filter_curl_output(output);
        assert!(result.contains("foo"), "array item values preserved");
        assert!(result.contains("bar"), "array item values preserved");
    }

    #[test]
    fn test_filter_curl_non_json() {
        let output = "Hello, World!\nThis is plain text.";
        let result = filter_curl_output(output);
        assert!(result.contains("Hello, World!"));
        assert!(result.contains("plain text"));
    }

    #[test]
    fn test_filter_curl_json_small_pretty_printed() {
        let output = r#"{"r2Ready":true,"status":"ok"}"#;
        let result = filter_curl_output(output);
        assert!(result.contains("r2Ready"));
        assert!(result.contains("true"));
        assert!(result.contains("ok"));
    }

    #[test]
    fn test_filter_curl_long_output() {
        let lines: Vec<String> = (0..50).map(|i| format!("Line {}", i)).collect();
        let output = lines.join("\n");
        let result = filter_curl_output(&output);
        assert!(result.contains("Line 0"));
        assert!(result.contains("Line 29"));
        assert!(result.contains("more lines"));
    }

    #[test]
    fn test_filter_curl_long_json_truncated() {
        // Build JSON with many lines when pretty-printed
        let items: Vec<String> = (0..100)
            .map(|i| format!(r#"{{"id": {}, "name": "item_{}"}}"#, i, i))
            .collect();
        let output = format!("[{}]", items.join(","));
        let result = filter_curl_output(&output);
        assert!(result.contains("item_0"), "first items preserved");
        assert!(result.contains("more lines"), "long JSON truncated");
    }
}
