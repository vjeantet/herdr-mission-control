//! Direct client for herdr's newline-delimited JSON socket API
//! (`HERDR_SOCKET_PATH`). The server handles exactly one request per
//! connection (herdr src/api/server.rs, handle_connection), so every call
//! reconnects; a Unix socket connect costs microseconds, cheap enough for
//! the 4 Hz preview refresh.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

#[derive(Deserialize)]
struct Envelope<T> {
    result: Option<T>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct SnapshotResult {
    snapshot: Snapshot,
}

#[derive(Deserialize)]
pub struct Snapshot {
    pub focused_workspace_id: Option<String>,
    pub tabs: Vec<TabInfo>,
    pub panes: Vec<PaneInfo>,
    pub layouts: Vec<LayoutInfo>,
}

#[derive(Deserialize)]
pub struct LayoutInfo {
    pub tab_id: String,
    pub zoomed: bool,
}

#[derive(Deserialize)]
pub struct TabInfo {
    pub tab_id: String,
    pub workspace_id: String,
    pub number: usize,
    pub label: String,
    pub focused: bool,
}

#[derive(Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
    pub tab_id: String,
    pub focused: bool,
    pub title: Option<String>,
    pub terminal_title_stripped: Option<String>,
    pub agent: Option<String>,
    pub display_agent: Option<String>,
    pub agent_status: String,
}

#[derive(Deserialize)]
struct ReadResultBody {
    read: ReadResult,
}

#[derive(Deserialize)]
struct ReadResult {
    text: String,
}

pub struct HerdrClient {
    socket: String,
}

impl HerdrClient {
    pub fn from_env() -> Result<Self, String> {
        std::env::var("HERDR_SOCKET_PATH")
            .map(|socket| Self { socket })
            .map_err(|_| "HERDR_SOCKET_PATH not set (not launched by herdr?)".to_string())
    }

    fn call<T: for<'de> Deserialize<'de>>(
        &self,
        request: &serde_json::Value,
    ) -> Result<T, String> {
        let mut stream = UnixStream::connect(&self.socket)
            .map_err(|err| format!("connect {}: {err}", self.socket))?;
        let mut line = request.to_string();
        line.push('\n');
        stream
            .write_all(line.as_bytes())
            .map_err(|err| format!("write: {err}"))?;
        let mut response = String::new();
        BufReader::new(stream)
            .read_line(&mut response)
            .map_err(|err| format!("read: {err}"))?;
        let envelope: Envelope<T> =
            serde_json::from_str(&response).map_err(|err| format!("bad response: {err}"))?;
        if let Some(error) = envelope.error {
            return Err(format!("{} ({})", error.message, error.code));
        }
        envelope.result.ok_or_else(|| "empty response".to_string())
    }

    pub fn snapshot(&self) -> Result<Snapshot, String> {
        self.call::<SnapshotResult>(&json!({
            "id": "expose:snapshot",
            "method": "session.snapshot",
            "params": {},
        }))
        .map(|r| r.snapshot)
    }

    pub fn pane_read(&self, pane_id: &str, lines: u32) -> Result<String, String> {
        self.call::<ReadResultBody>(&json!({
            "id": "expose:read",
            "method": "pane.read",
            "params": {
                "pane_id": pane_id,
                "source": "recent_unwrapped",
                "lines": lines,
                "format": "ansi",
            },
        }))
        .map(|r| r.read.text)
    }

    pub fn pane_close(&self, pane_id: &str) -> Result<(), String> {
        self.call::<serde_json::Value>(&json!({
            "id": "expose:close",
            "method": "pane.close",
            "params": { "pane_id": pane_id },
        }))
        .map(|_| ())
    }

    /// Focus a pane by id. There is no direct "focus by id" API; a zoom
    /// request matching the tab's current zoom state is a no-op that still
    /// moves focus (herdr `handle_pane_zoom` calls `focus_pane_in_workspace`
    /// before checking the mode).
    pub fn focus_pane(&self, pane_id: &str, tab_zoomed: bool) -> Result<(), String> {
        self.call::<serde_json::Value>(&json!({
            "id": "expose:focus",
            "method": "pane.zoom",
            "params": {
                "pane_id": pane_id,
                "mode": if tab_zoomed { "on" } else { "off" },
            },
        }))
        .map(|_| ())
    }
}
