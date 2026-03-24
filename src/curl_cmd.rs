use crate::tracking;
use crate::utils::truncate;
use anyhow::{Context, Result};
use std::process::Command;

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();
    let mut cmd = Command::new("curl");
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

/// Max lines of pretty-printed JSON to show before truncating.
const MAX_JSON_LINES: usize = 80;
/// Max lines for non-JSON output.
const MAX_TEXT_LINES: usize = 30;

fn filter_curl_output(output: &str) -> String {
    let trimmed = output.trim();

    // Try JSON detection: pretty-print with actual values preserved.
    // Previous behaviour replaced values with types (schema mode) which
    // destroyed data needed for API debugging.
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && (trimmed.ends_with('}') || trimmed.ends_with(']'))
    {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let pretty = serde_json::to_string_pretty(&parsed).unwrap_or_default();
            return truncate_lines(&pretty, MAX_JSON_LINES);
        }
    }

    // Not JSON: truncate long output
    truncate_lines(trimmed, MAX_TEXT_LINES)
}

/// Keep at most `max` lines, appending a summary when truncated.
/// Long individual lines are capped at 200 chars.
fn truncate_lines(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > max {
        let kept: Vec<String> = lines[..max].iter().map(|l| truncate(l, 200)).collect();
        format!(
            "{}\n\n... ({} more lines, {} bytes total)",
            kept.join("\n"),
            lines.len() - max,
            text.len()
        )
    } else {
        lines
            .iter()
            .map(|l| truncate(l, 200))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_curl_json_preserves_values() {
        let output = r#"{"name": "test", "count": 42, "items": [1, 2, 3]}"#;
        let result = filter_curl_output(output);
        // Must preserve actual values — not replace with types
        assert!(result.contains("\"test\""), "should keep string value");
        assert!(result.contains("42"), "should keep numeric value");
        assert!(result.contains("name"), "should keep key name");
        assert!(!result.contains(": string"), "must NOT schema-ify values");
        assert!(!result.contains(": int"), "must NOT schema-ify values");
    }

    #[test]
    fn test_filter_curl_json_array_preserves_values() {
        let output = r#"[{"id": 1, "title": "hello"}, {"id": 2, "title": "world"}]"#;
        let result = filter_curl_output(output);
        assert!(result.contains("\"hello\""), "should keep string values");
        assert!(result.contains("2"), "should keep numeric values");
    }

    #[test]
    fn test_filter_curl_json_pretty_prints() {
        let output = r#"{"a":1,"b":"two"}"#;
        let result = filter_curl_output(output);
        // Pretty-printed should have newlines
        assert!(result.contains('\n'), "should be multi-line");
        assert!(result.contains("\"a\""), "should keep key");
        assert!(result.contains("\"two\""), "should keep value");
    }

    #[test]
    fn test_filter_curl_non_json() {
        let output = "Hello, World!\nThis is plain text.";
        let result = filter_curl_output(output);
        assert!(result.contains("Hello, World!"));
        assert!(result.contains("plain text"));
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
    fn test_filter_curl_large_json_truncated() {
        // Build a JSON object with many keys to exceed MAX_JSON_LINES
        let mut obj = serde_json::Map::new();
        for i in 0..200 {
            obj.insert(
                format!("key_{}", i),
                serde_json::Value::String(format!("value_{}", i)),
            );
        }
        let output = serde_json::to_string(&obj).unwrap();
        let result = filter_curl_output(&output);
        assert!(result.contains("more lines"), "should truncate large JSON");
        assert!(result.contains("key_0"), "should keep early keys");
    }
}
