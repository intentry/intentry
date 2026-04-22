use owo_colors::OwoColorize;
use serde::Serialize;

// ---------------------------------------------------------------------------
// TTY detection
// ---------------------------------------------------------------------------

/// Returns `true` if stdout is a real terminal (not piped / redirected).
pub fn is_tty() -> bool {
    // Simple heuristic: check if stdout is a tty via the `std` approach.
    // On Windows we check TERM env var fallback.
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::isatty(std::io::stdout().as_raw_fd()) != 0 }
    }
    #[cfg(not(unix))]
    {
        // On Windows, assume tty unless NO_COLOR or CI is set.
        std::env::var("NO_COLOR").is_err() && std::env::var("CI").is_err()
    }
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

/// Print a success line (green bullet when TTY, plain "ok" otherwise).
pub fn print_success(msg: &str) {
    if is_tty() {
        println!("{} {msg}", "✓".green().bold());
    } else {
        println!("ok: {msg}");
    }
}

/// Print an info line (dim when TTY).
pub fn print_info(msg: &str) {
    if is_tty() {
        println!("{}", msg.dimmed());
    } else {
        println!("{msg}");
    }
}

/// Print a warning to stderr (yellow when TTY).
pub fn print_warn(msg: &str) {
    if is_tty() {
        eprintln!("{} {msg}", "⚠".yellow().bold());
    } else {
        eprintln!("warn: {msg}");
    }
}

/// Print an error to stderr (red when TTY).
pub fn print_error(msg: &str) {
    if is_tty() {
        eprintln!("{} {msg}", "✗".red().bold());
    } else {
        eprintln!("error: {msg}");
    }
}

// ---------------------------------------------------------------------------
// JSON output
// ---------------------------------------------------------------------------

/// Print a successful JSON result to stdout.
pub fn print_json_ok<T: Serialize>(data: &T) {
    let envelope = serde_json::json!({ "ok": true, "data": data });
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap_or_default());
}

/// Print a JSON error to stdout.
pub fn print_json_error(code: &str, message: &str) {
    let envelope = serde_json::json!({
        "ok": false,
        "error": { "code": code, "message": message }
    });
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap_or_default());
}

// ---------------------------------------------------------------------------
// Table printing (TTY only)
// ---------------------------------------------------------------------------

/// Print a simple two-column key/value table.
pub fn print_kv_table(rows: &[(&str, String)]) {
    let max_key = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (key, val) in rows {
        if is_tty() {
            println!("  {:width$}  {}", key.bold(), val, width = max_key);
        } else {
            println!("{key}\t{val}");
        }
    }
}
