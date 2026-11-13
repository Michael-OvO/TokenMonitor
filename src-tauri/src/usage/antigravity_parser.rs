//! Parser for Google Antigravity conversations.
//!
//! Antigravity stores each conversation as an SQLite database,
//! `~/.gemini/antigravity/conversations/<uuid>.db`. Two tables matter:
//!
//! - `gen_metadata.data`: one protobuf per model call. Field 1 is the chat
//!   model metadata; inside it field 4 is the usage message and field 19 the
//!   model name (`gemini-4-hydrogen`).
//! - `steps.metadata`: one protobuf per step. Field 1 is its creation time
//!   (`{1: seconds}`) and, on a model-response step, field 9 repeats the
//!   usage message.
//!
//! Usage message: 2 = input tokens (cache reads excluded), 3 = output tokens
//! (thinking included), 4 = cache-write tokens, 5 = cache-read tokens,
//! 7 = message id. The message id joins a call to its step's timestamp.
//!
//! The schema is reverse-engineered, so everything is read leniently: a row
//! that does not decode is skipped.
//!
//! ponytail: older `<uuid>.pb` conversations are encrypted and not read.

use chrono::{DateTime, Local, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;

use super::parser::{path_to_string, ParsedEntry, SessionParseResult};

enum Wire<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// The first value of `field` in a protobuf message, schema-free. Stops at
/// the first byte that does not decode.
fn field(buf: &[u8], field: u64) -> Option<Wire<'_>> {
    let mut pos = 0;
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        let value = match tag & 7 {
            0 => Some(Wire::Varint(read_varint(buf, &mut pos)?)),
            1 => {
                pos += 8;
                None
            }
            2 => {
                let len = usize::try_from(read_varint(buf, &mut pos)?).ok()?;
                let end = pos.checked_add(len)?;
                let bytes = buf.get(pos..end)?;
                pos = end;
                Some(Wire::Bytes(bytes))
            }
            5 => {
                pos += 4;
                None
            }
            _ => return None,
        };
        if tag >> 3 == field {
            return value;
        }
    }
    None
}

fn bytes(buf: &[u8], number: u64) -> Option<&[u8]> {
    match field(buf, number)? {
        Wire::Bytes(bytes) => Some(bytes),
        Wire::Varint(_) => None,
    }
}

fn varint(buf: &[u8], number: u64) -> u64 {
    match field(buf, number) {
        Some(Wire::Varint(value)) => value,
        _ => 0,
    }
}

fn text(buf: &[u8], number: u64) -> Option<&str> {
    std::str::from_utf8(bytes(buf, number)?).ok()
}

/// `steps.metadata` → (usage message id, step creation time).
fn step_stamp(metadata: &[u8]) -> Option<(String, DateTime<Local>)> {
    let message_id = text(bytes(metadata, 9)?, 7)?;
    let seconds = i64::try_from(varint(bytes(metadata, 1)?, 1)).ok()?;
    let created = Utc.timestamp_opt(seconds, 0).single()?;
    Some((message_id.to_string(), created.with_timezone(&Local)))
}

/// `gen_metadata.data` → an entry stamped from `stamps`, else `fallback`.
fn generation_entry(
    data: &[u8],
    stamps: &HashMap<String, DateTime<Local>>,
    fallback: DateTime<Local>,
    session_key: &str,
) -> Option<ParsedEntry> {
    let chat = bytes(data, 1)?;
    let usage = bytes(chat, 4)?;
    let input_tokens = varint(usage, 2);
    let output_tokens = varint(usage, 3);
    let cache_write = varint(usage, 4);
    let cache_read = varint(usage, 5);
    if input_tokens + output_tokens + cache_write + cache_read == 0 {
        return None;
    }
    let model = text(chat, 19).filter(|model| !model.is_empty())?;
    let timestamp = text(usage, 7)
        .and_then(|id| stamps.get(id).copied())
        .unwrap_or(fallback);

    Some(ParsedEntry {
        timestamp,
        // `gemini-4-argon-trial` is the same model on a trial quota.
        model: model.strip_suffix("-trial").unwrap_or(model).to_string(),
        input_tokens,
        output_tokens,
        cache_creation_5m_tokens: cache_write,
        cache_creation_1h_tokens: 0,
        cache_read_tokens: cache_read,
        web_search_requests: 0,
        billed_cost: None,
        unique_hash: None,
        session_key: session_key.to_string(),
        agent_scope: crate::stats::subagent::AgentScope::Main,
        // ponytail: no project attribution; the workspace URI is in
        // `trajectory_metadata_blob` (field 1.1) if per-project stats matter.
        session_meta: None,
    })
}

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let wal_pending = std::fs::metadata(wal).is_ok_and(|meta| meta.len() > 0);
    // With nothing in the WAL the file is opened immutable, which takes no
    // locks and creates no `-shm` beside a database that is not ours. A read
    // that races a checkpoint fails or comes up short, and is redone when the
    // file's stamp next changes.
    let uri = format!(
        "file:{}{}?mode=ro{}",
        if cfg!(windows) { "/" } else { "" },
        path.to_string_lossy()
            .replace('\\', "/")
            .replace('%', "%25")
            .replace('?', "%3f")
            .replace('#', "%23"),
        if wal_pending { "" } else { "&immutable=1" },
    );
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;
    Ok(conn)
}

fn blobs(conn: &Connection, sql: &str) -> rusqlite::Result<Vec<Vec<u8>>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map([], |row| row.get::<_, Option<Vec<u8>>>(0))?;
    Ok(rows.flatten().flatten().collect())
}

fn read_conversation(path: &Path) -> rusqlite::Result<(Vec<ParsedEntry>, usize)> {
    let conn = open_read_only(path)?;
    let stamps: HashMap<String, DateTime<Local>> =
        blobs(&conn, "SELECT metadata FROM steps ORDER BY idx")?
            .iter()
            .filter_map(|metadata| step_stamp(metadata))
            .collect();
    let generations = blobs(&conn, "SELECT data FROM gen_metadata ORDER BY idx")?;

    let session_key = format!("antigravity-file:{}", path_to_string(path));
    // A call whose step is missing takes the previous call's time, or the
    // file's own.
    let mut fallback: DateTime<Local> = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map(DateTime::from)
        .unwrap_or_else(|_| Local::now());
    let mut entries = Vec::new();
    for data in &generations {
        if let Some(entry) = generation_entry(data, &stamps, fallback, &session_key) {
            fallback = entry.timestamp;
            entries.push(entry);
        }
    }
    entries.sort_by_key(|entry| entry.timestamp);
    Ok((entries, generations.len()))
}

pub(crate) fn parse_antigravity_conversation(path: &Path) -> SessionParseResult {
    match read_conversation(path) {
        Ok((entries, rows)) => (entries, Vec::new(), rows, true),
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "Antigravity conversation unreadable");
            (Vec::new(), Vec::new(), 0, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push(value as u8 | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn int(number: u64, value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, number << 3);
        put_varint(&mut out, value);
        out
    }

    fn msg(number: u64, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, number << 3 | 2);
        put_varint(&mut out, body.len() as u64);
        out.extend_from_slice(body);
        out
    }

    fn usage(message_id: &str) -> Vec<u8> {
        [
            int(2, 2793),
            int(3, 284),
            int(5, 9147),
            msg(7, message_id.as_bytes()),
        ]
        .concat()
    }

    #[test]
    fn reads_usage_and_joins_step_time_from_a_conversation_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c #1.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE steps(idx INTEGER PRIMARY KEY, metadata BLOB);
             CREATE TABLE gen_metadata(idx INTEGER PRIMARY KEY, data BLOB);",
        )
        .unwrap();
        let step = [msg(1, &int(1, 1_800_000_000)), msg(9, &usage("bot-1"))].concat();
        let generation = msg(
            1,
            &[msg(4, &usage("bot-1")), msg(19, b"gemini-4-argon-trial")].concat(),
        );
        conn.execute("INSERT INTO steps VALUES (0, ?1)", [&step])
            .unwrap();
        conn.execute("INSERT INTO steps VALUES (1, NULL)", [])
            .unwrap();
        conn.execute("INSERT INTO gen_metadata VALUES (0, ?1)", [&generation])
            .unwrap();
        conn.execute("INSERT INTO gen_metadata VALUES (1, x'ff')", [])
            .unwrap();
        drop(conn);

        let (entries, _, rows, _) = parse_antigravity_conversation(&path);
        assert_eq!(rows, 2);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.model, "gemini-4-argon");
        assert_eq!(
            (
                entry.input_tokens,
                entry.output_tokens,
                entry.cache_read_tokens
            ),
            (2793, 284, 9147)
        );
        assert_eq!(entry.timestamp.timestamp(), 1_800_000_000);
    }

    #[test]
    fn an_unreadable_file_yields_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.db");
        std::fs::write(&path, b"not a database").unwrap();
        assert!(parse_antigravity_conversation(&path).0.is_empty());
    }
}
