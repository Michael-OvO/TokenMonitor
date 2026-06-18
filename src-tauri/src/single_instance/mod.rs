//! Single-instance guard built on a loopback lock port.
//!
//! On launch we try to bind `127.0.0.1:LOCK_PORT` (loopback ONLY — never
//! `0.0.0.0`, so no firewall prompt and no LAN exposure). The outcome drives a
//! small state machine, all of it BEFORE `tauri::Builder` so a declined launch
//! exits without ever creating a tray icon or window:
//!
//! * bind succeeds: we are the sole instance; keep the socket.
//! * bind fails + PROBE returns our magic: another TokenMonitor (REQ-001) — ask
//!   the user, then QUIT it and take over, or exit.
//! * bind fails + foreign/garbled reply: a foreign process holds it (REQ-002) —
//!   ask the user, then kill it and take over, or exit.
//!
//! `run()` is synchronous (Tauri owns the async runtime), so the entire startup
//! path is blocking `std` code — no tokio. The owner instance's accept loop is
//! spawned later from `setup()` (where the `AppHandle` exists) on a dedicated
//! `std::thread`.

mod dialog;
mod port;
pub mod protocol;

use protocol::{InstanceDecision, Request};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Holds the bound lock socket for the process lifetime so it stays occupied.
static LOCK_LISTENER: OnceLock<TcpListener> = OnceLock::new();
/// Windows-only fallback when the fixed lock port is owned by an unkillable
/// foreign process. The handle is intentionally held for the process lifetime.
#[cfg(target_os = "windows")]
static FALLBACK_MUTEX_HANDLE: OnceLock<usize> = OnceLock::new();

const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const REBIND_BUDGET: Duration = Duration::from_secs(5);
const REBIND_STEP: Duration = Duration::from_millis(100);

/// Result of [`acquire_or_exit`]: continue launching, or unwind out of `run()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acquire {
    Continue,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
// Only the Windows fallback path constructs `Acquired`/`AlreadyRunning`; on
// other platforms the lock is always `Unavailable`, so suppress dead-code there.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
enum FallbackLock {
    Acquired,
    AlreadyRunning,
    Unavailable(String),
}

fn should_bypass() -> bool {
    std::env::var("TM_DISABLE_SINGLE_INSTANCE").is_ok()
}

/// Effective lock port: `TM_LOCK_PORT` override if valid, else the default.
fn lock_port() -> u16 {
    std::env::var("TM_LOCK_PORT")
        .ok()
        .and_then(|v| protocol::parse_lock_port_override(&v))
        .unwrap_or(protocol::LOCK_PORT)
}

fn try_bind_lock(port: u16) -> std::io::Result<TcpListener> {
    // Rust's std does not set SO_REUSEADDR on TcpListener, so a second bind
    // fails while another instance holds the port — exactly the mutual
    // exclusion we want, on every platform.
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

#[cfg(target_os = "windows")]
fn try_acquire_fallback_lock() -> FallbackLock {
    const FALLBACK_MUTEX_NAME: &str = "Local\\TokenMonitor.SingleInstanceFallback";

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    if FALLBACK_MUTEX_HANDLE.get().is_some() {
        return FallbackLock::Acquired;
    }

    let name: Vec<u16> = FALLBACK_MUTEX_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let handle = match unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) } {
        Ok(handle) => handle,
        Err(e) => return FallbackLock::Unavailable(format!("无法创建备用锁：{e}")),
    };

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let _ = unsafe { CloseHandle(handle) };
        return FallbackLock::AlreadyRunning;
    }

    match FALLBACK_MUTEX_HANDLE.set(handle.0 as usize) {
        Ok(()) => FallbackLock::Acquired,
        Err(_) => {
            let _ = unsafe { CloseHandle(handle) };
            FallbackLock::Acquired
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn try_acquire_fallback_lock() -> FallbackLock {
    FallbackLock::Unavailable("当前平台没有可用的备用锁。".into())
}

fn continue_with_fallback_lock(reason: &str) -> Acquire {
    match try_acquire_fallback_lock() {
        FallbackLock::Acquired => {
            tracing::warn!("Continuing with fallback single-instance lock because {reason}");
            Acquire::Continue
        }
        FallbackLock::AlreadyRunning => {
            dialog::show_error(
                "检测到已有一个 TokenMonitor 正在运行（备用单实例锁）。\n\n\
                 请先关闭旧实例后再重试。",
            );
            Acquire::Exit
        }
        FallbackLock::Unavailable(e) => {
            dialog::show_error(&format!("{reason}\n\n备用单实例锁不可用：{e}"));
            Acquire::Exit
        }
    }
}

/// The synchronous startup state machine. Runs at the top of `run()`.
pub fn acquire_or_exit() -> Acquire {
    if should_bypass() {
        tracing::info!("Single-instance guard bypassed (TM_DISABLE_SINGLE_INSTANCE set)");
        return Acquire::Continue;
    }

    let port = lock_port();

    match try_bind_lock(port) {
        Ok(listener) => {
            let _ = LOCK_LISTENER.set(listener);
            tracing::info!("Single-instance lock acquired on 127.0.0.1:{port}");
            return Acquire::Continue;
        }
        Err(e) => {
            tracing::info!("Lock port {port} is busy ({e}); probing the holder");
        }
    }

    let probe = probe_holder(port);
    match protocol::decide(false, probe.as_deref()) {
        InstanceDecision::SoleInstance => {
            // decide(false, _) never yields this; bind already failed above.
            unreachable!("SoleInstance is impossible after a failed bind");
        }
        InstanceDecision::OwnInstanceRunning(reply) => {
            tracing::info!(
                "Another TokenMonitor is running (pid {}, v{})",
                reply.pid,
                reply.version
            );
            if dialog::confirm_replace_old_instance() {
                let _ = send_line(port, Request::Quit);
                match poll_rebind(port) {
                    Some(listener) => {
                        let _ = LOCK_LISTENER.set(listener);
                        tracing::info!("Old instance exited; lock taken over");
                        Acquire::Continue
                    }
                    None => {
                        dialog::show_error("旧实例未能在限定时间内退出，请手动关闭后重试。");
                        Acquire::Exit
                    }
                }
            } else {
                // Best-effort: bring the running instance forward, then exit.
                let _ = send_line(port, Request::Focus);
                tracing::info!("User kept the existing instance; exiting new launch");
                Acquire::Exit
            }
        }
        InstanceDecision::ForeignProcess => {
            let holder = port::find_port_holder(port);
            if let Some(ref h) = holder {
                tracing::info!(
                    "Lock port held by foreign process {} (pid {})",
                    h.name,
                    h.pid
                );
            } else {
                tracing::info!("Lock port held by an unidentified foreign process");
            }
            if dialog::confirm_free_port(holder.as_ref(), port) {
                if let Some(ref h) = holder {
                    if let Err(e) = port::kill_pid(h.pid) {
                        return continue_with_fallback_lock(&format!(
                            "无法结束占用端口的进程：{e}"
                        ));
                    }
                }
                match poll_rebind(port) {
                    Some(listener) => {
                        let _ = LOCK_LISTENER.set(listener);
                        tracing::info!("Foreign holder cleared; lock acquired");
                        Acquire::Continue
                    }
                    None => continue_with_fallback_lock("端口释放失败。"),
                }
            } else {
                Acquire::Exit
            }
        }
    }
}

/// Connect and send a single PROBE; return the raw reply line (or `None`).
fn probe_holder(port: u16) -> Option<String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT)).ok();
    stream.set_write_timeout(Some(PROBE_TIMEOUT)).ok();
    stream.write_all(b"PROBE\n").ok()?;
    stream.flush().ok();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    Some(line)
}

/// Send a request line to the running owner and read its (ignored) reply.
fn send_line(port: u16, req: Request) -> Option<String> {
    let word = match req {
        Request::Probe => "PROBE",
        Request::Quit => "QUIT",
        Request::Focus => "FOCUS",
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT)).ok();
    stream.set_write_timeout(Some(PROBE_TIMEOUT)).ok();
    stream.write_all(format!("{word}\n").as_bytes()).ok()?;
    stream.flush().ok();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let _ = reader.read_line(&mut line);
    Some(line)
}

/// Poll-bind the lock port until it frees up or the budget elapses.
fn poll_rebind(port: u16) -> Option<TcpListener> {
    let deadline = Instant::now() + REBIND_BUDGET;
    loop {
        if let Ok(listener) = try_bind_lock(port) {
            return Some(listener);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(REBIND_STEP);
    }
}

/// Spawn the owner instance's accept loop. Called from `setup()` once the
/// `AppHandle` exists. No-op when bypassed or when we never acquired the lock.
pub fn spawn_accept_loop(app: tauri::AppHandle) {
    if should_bypass() {
        return;
    }
    let Some(listener) = LOCK_LISTENER.get() else {
        return;
    };
    let listener = match listener.try_clone() {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("Could not clone lock listener for accept loop: {e}");
            return;
        }
    };

    let builder = std::thread::Builder::new().name("single-instance-accept".into());
    let spawned = builder.spawn(move || {
        for incoming in listener.incoming() {
            match incoming {
                Ok(stream) => handle_conn(&app, stream),
                Err(e) => tracing::debug!("Lock listener accept error: {e}"),
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!("Failed to spawn single-instance accept loop: {e}");
    }
}

/// Handle one inbound connection from a launching instance.
fn handle_conn(app: &tauri::AppHandle, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(read_stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }

    match protocol::parse_request(&line) {
        Some(Request::Probe) => {
            let reply = protocol::format_probe_reply(env!("CARGO_PKG_VERSION"), std::process::id());
            let _ = stream.write_all(reply.as_bytes());
            let _ = stream.flush();
        }
        Some(Request::Quit) => {
            // Reply + flush BEFORE exiting so the requester reliably gets the ack.
            let _ = stream.write_all(b"OK\n");
            let _ = stream.flush();
            tracing::info!("Received QUIT from a new instance; exiting");
            app.exit(0);
        }
        Some(Request::Focus) => {
            let _ = stream.write_all(b"OK\n");
            let _ = stream.flush();
            crate::show_main_window(app);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_port_default_when_no_override() {
        // We don't mutate process env here (racy across tests); just assert the
        // default constant is what lock_port() falls back to.
        assert_eq!(protocol::LOCK_PORT, 53217);
    }

    #[test]
    fn bind_then_second_bind_fails() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // A second bind on the same loopback port must fail while held.
        assert!(try_bind_lock(port).is_err());
        drop(listener);
    }

    #[test]
    fn probe_and_quit_roundtrip_over_loopback() {
        // Stand up a minimal owner-side responder and exercise the wire format
        // end-to-end (PROBE -> magic reply; QUIT -> OK), without a Tauri app.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = std::thread::spawn(move || {
            // Handle exactly two connections: a PROBE then a QUIT.
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let read_stream = stream.try_clone().unwrap();
                let mut reader = BufReader::new(read_stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                match protocol::parse_request(&line) {
                    Some(Request::Probe) => {
                        let reply = protocol::format_probe_reply("9.9.9", 4242);
                        stream.write_all(reply.as_bytes()).unwrap();
                    }
                    Some(Request::Quit) => {
                        stream.write_all(b"OK\n").unwrap();
                    }
                    _ => {}
                }
                stream.flush().unwrap();
            }
        });

        let probe = probe_holder(port).expect("probe reply");
        match protocol::decide(false, Some(&probe)) {
            InstanceDecision::OwnInstanceRunning(r) => {
                assert_eq!(r.version, "9.9.9");
                assert_eq!(r.pid, 4242);
            }
            other => panic!("expected OwnInstanceRunning, got {other:?}"),
        }

        let ack = send_line(port, Request::Quit).expect("quit ack");
        assert_eq!(ack.trim(), "OK");

        server.join().unwrap();
    }
}
