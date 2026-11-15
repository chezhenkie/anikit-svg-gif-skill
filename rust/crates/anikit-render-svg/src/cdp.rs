//! A small Chrome DevTools Protocol client: one WebSocket, one request at a time.
//!
//! Messages are JSON objects with an increasing id. Events that arrive between
//! a request and its reply are discarded, because every step of the route waits
//! for its own reply before the next one is sent.

use std::net::TcpStream;
use std::time::Duration;

use anikit_core::{Error, Result};
use serde_json::{Value, json};
use tungstenite::handshake::client::{Request, generate_key};
use tungstenite::protocol::Message;
use tungstenite::{WebSocket, client};

/// How long one reply may take before the run fails.
const REPLY_TIMEOUT: Duration = Duration::from_secs(120);

/// A connection to one browser page target.
pub struct Session {
    socket: WebSocket<TcpStream>,
    next_id: u64,
}

impl Session {
    /// Connect to the WebSocket address a target reported.
    pub fn connect(url: &str) -> Result<Session> {
        let (host, port, _path) = split_url(url)?;
        let stream = TcpStream::connect((host.as_str(), port))
            .map_err(|err| Error::io(format!("cannot reach the browser page: {err}")))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(REPLY_TIMEOUT))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;

        let request = Request::builder()
            .uri(url)
            .header("Host", format!("{host}:{port}"))
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .body(())
            .map_err(|err| Error::invalid(format!("cannot build the browser request: {err}")))?;
        let (socket, _reply) = client::client(request, stream)
            .map_err(|err| Error::io(format!("browser handshake failed: {err}")))?;
        Ok(Session { socket, next_id: 1 })
    }

    /// Send one method call and return its result.
    pub fn call(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut message = json!({ "id": id, "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        self.socket
            .send(Message::Text(message.to_string().into()))
            .map_err(|err| Error::io(format!("cannot send {method}: {err}")))?;

        loop {
            let reply = match self.socket.read() {
                Ok(reply) => reply,
                Err(tungstenite::Error::Io(err))
                    if err.kind() == std::io::ErrorKind::WouldBlock
                        || err.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return Err(Error::invalid(format!(
                        "browser did not answer {method} within {} s",
                        REPLY_TIMEOUT.as_secs()
                    )));
                }
                Err(err) => {
                    return Err(Error::io(format!("browser connection failed: {err}")));
                }
            };
            match reply {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(text.as_str())
                        .map_err(|err| Error::io(format!("browser sent invalid json: {err}")))?;
                    if value.get("id").and_then(Value::as_u64) != Some(id) {
                        continue;
                    }
                    if let Some(error) = value.get("error") {
                        let message = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("browser error");
                        return Err(Error::invalid(format!("{method}: {message}")));
                    }
                    return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                }
                Message::Ping(payload) => {
                    self.socket
                        .send(Message::Pong(payload))
                        .map_err(|err| Error::io(format!("cannot answer the browser: {err}")))?;
                }
                Message::Close(_) => {
                    return Err(Error::io("browser closed the page connection"));
                }
                Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    }
}

/// Split `ws://host:port/path` into its parts.
fn split_url(url: &str) -> Result<(String, u16, String)> {
    let rest = url
        .strip_prefix("ws://")
        .ok_or_else(|| Error::invalid(format!("not a websocket address: {url}")))?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| Error::invalid(format!("websocket address has no port: {url}")))?;
    let port: u16 = port
        .parse()
        .map_err(|_| Error::invalid(format!("websocket address has a bad port: {url}")))?;
    Ok((host.to_string(), port, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_address_is_split_into_parts() {
        let (host, port, path) = split_url("ws://127.0.0.1:9222/devtools/page/AB").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 9222);
        assert_eq!(path, "/devtools/page/AB");
        assert!(split_url("http://127.0.0.1:1/x").is_err());
        assert!(split_url("ws://127.0.0.1/x").is_err());
    }
}
