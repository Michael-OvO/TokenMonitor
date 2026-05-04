//! Script bodies installed as Claude Code's `statusLine` command.
//!
//! Both scripts read the JSON envelope CC writes to stdin, append a single
//! `{"ts": "...", "payload": <stdin>}` JSONL line to the events file, and exit
//! with no output. CC tolerates an empty stdout — leaving it blank means we
//! don't co-opt the user's terminal status line for our own bookkeeping.
//!
//! The events directory path is templated in at install time (`{events_dir}`)
//! so the scripts hold an absolute path and don't depend on env vars at
//! runtime. Users who relocate `app_data` re-install via the app's
//! Permissions settings.
//!
//! Both scripts are tolerant of:
//! - missing parent directory (created on first call)
//! - empty stdin (fall back to `{}` so the appender never produces a broken
//!   line — CC very occasionally fires the hook with no payload)
//! - JSON containing embedded newlines (stripped before append; the JSONL
//!   contract is one event per line)
//!
//! IMPORTANT: rendering uses `.replace()`, not `format!`, so curly braces in
//! these templates are literal — do *not* double them. The only placeholders
//! that get substituted are `{events_dir}` and `{events_file}`.

/// POSIX shell version installed on macOS / Linux.
pub const POSIX_SCRIPT: &str = r#"#!/bin/sh
# TokenMonitor statusline bridge.
# Receives Claude Code's session JSON on stdin and appends one event line.
# Prints nothing so the user's terminal status line stays untouched.
# Managed by TokenMonitor — re-installing the script overwrites this file.

set -u
events_dir="{events_dir}"
events_file="{events_file}"

mkdir -p "$events_dir" 2>/dev/null || true

ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
payload="$(cat | tr -d '\r\n')"
[ -z "$payload" ] && payload="{}"

# Append-and-fsync isn't worth the cost here — events are advisory, and the
# OS pagecache is reliable enough that a crash within milliseconds of a
# prompt is the worst case.
printf '{"ts":"%s","payload":%s}\n' "$ts" "$payload" >> "$events_file" 2>/dev/null || true

exit 0
"#;

/// PowerShell version installed on Windows.
pub const WINDOWS_SCRIPT: &str = r#"# TokenMonitor statusline bridge.
# Receives Claude Code's session JSON on stdin and appends one event line.
# Prints nothing so the user's terminal status line stays untouched.
# Managed by TokenMonitor - re-installing the script overwrites this file.

$ErrorActionPreference = 'SilentlyContinue'
$eventsDir  = '{events_dir}'
$eventsFile = '{events_file}'

if (-not (Test-Path $eventsDir)) {
    New-Item -ItemType Directory -Force -Path $eventsDir | Out-Null
}

$payload = [Console]::In.ReadToEnd()
if ([string]::IsNullOrEmpty($payload)) { $payload = '{}' }
$payload = $payload -replace "[\r\n]", ''

$ts = [DateTimeOffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
$line = '{"ts":"' + $ts + '","payload":' + $payload + '}'
Add-Content -Path $eventsFile -Value $line -Encoding UTF8
"#;

/// Render the script with the events directory + file path templated in.
pub fn render(events_dir: &str, events_file: &str) -> String {
    let template = if cfg!(target_os = "windows") {
        WINDOWS_SCRIPT
    } else {
        POSIX_SCRIPT
    };
    template
        .replace("{events_dir}", events_dir)
        .replace("{events_file}", events_file)
}

/// The exact command string TokenMonitor writes into Claude Code's
/// `settings.json` under `statusLine.command`. CC invokes the command via the
/// user's shell, so we have to give it something a plain shell will accept.
pub fn settings_command(script_path: &str) -> String {
    if cfg!(target_os = "windows") {
        // PowerShell -File takes the script path; -ExecutionPolicy Bypass keeps
        // the call working on machines where the user has not loosened the
        // default policy. -NoProfile skips $PROFILE so a slow profile doesn't
        // delay every prompt.
        format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
            script_path
        )
    } else {
        // Quote the path so spaces survive the shell parse. `sh` is universally
        // available on macOS/Linux and avoids depending on the user's $SHELL.
        format!("sh \"{}\"", script_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_script_contains_paths() {
        let script = render("/tmp/sl", "/tmp/sl/events.jsonl");
        assert!(script.contains("/tmp/sl"));
        assert!(script.contains("/tmp/sl/events.jsonl"));
    }

    #[test]
    fn rendered_script_has_no_unfilled_placeholders() {
        let script = render("/tmp/sl", "/tmp/sl/events.jsonl");
        assert!(!script.contains("{events_dir}"));
        assert!(!script.contains("{events_file}"));
    }

    #[test]
    fn rendered_script_has_no_doubled_braces() {
        // Regression guard for the `{{` / `}}` bug: we use `.replace()`,
        // not `format!`, so doubled braces would survive into the script
        // and produce malformed `{{"ts":...}}` JSONL on every prompt.
        let script = render("/tmp/sl", "/tmp/sl/events.jsonl");
        assert!(
            !script.contains("{{") && !script.contains("}}"),
            "rendered script must not contain doubled braces — they would be emitted literally to the events file"
        );
    }

    #[test]
    fn settings_command_quotes_path() {
        let cmd = settings_command("/path with space/script");
        assert!(cmd.contains("\"/path with space/script\""));
    }
}
