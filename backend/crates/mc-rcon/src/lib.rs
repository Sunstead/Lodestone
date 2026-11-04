//! Minimal async client for the Source RCON protocol as spoken by Minecraft.
//!
//! Packet layout (all integers little-endian):
//!
//! ```text
//! i32 length   (bytes that follow: id + kind + body + 2 nulls)
//! i32 id
//! i32 kind     3 = login, 2 = command, 0 = response
//! [u8] body    ASCII/UTF-8
//! u8 0, u8 0
//! ```
//!
//! Minecraft splits responses longer than 4096 bytes across several
//! packets with the same id, so [`Rcon::command`] keeps reading while
//! fragments keep arriving.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const KIND_LOGIN: i32 = 3;
const KIND_COMMAND: i32 = 2;
/// Largest response body Minecraft puts in one packet.
const MAX_FRAGMENT: usize = 4096;
/// Refuse absurd frames instead of allocating them.
const MAX_PACKET: usize = 1 << 20;

#[derive(Debug, thiserror::Error)]
pub enum RconError {
    #[error("rcon i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("rcon authentication failed")]
    AuthFailed,
    #[error("rcon timed out")]
    Timeout,
    #[error("rcon protocol error: {0}")]
    Protocol(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub id: i32,
    pub kind: i32,
    pub body: String,
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        let body = self.body.as_bytes();
        let length = (4 + 4 + body.len() + 2) as i32;
        let mut out = Vec::with_capacity(4 + length as usize);
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&self.id.to_le_bytes());
        out.extend_from_slice(&self.kind.to_le_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(&[0, 0]);
        out
    }

    /// Decodes one packet from the payload that follows the length prefix.
    pub fn decode(payload: &[u8]) -> Result<Self, RconError> {
        if payload.len() < 10 {
            return Err(RconError::Protocol(format!("packet too short ({} bytes)", payload.len())));
        }
        let id = i32::from_le_bytes(payload[0..4].try_into().expect("4 bytes"));
        let kind = i32::from_le_bytes(payload[4..8].try_into().expect("4 bytes"));
        let body = &payload[8..payload.len() - 2];
        Ok(Packet { id, kind, body: String::from_utf8_lossy(body).into_owned() })
    }
}

pub struct Rcon {
    stream: TcpStream,
    next_id: i32,
    io_timeout: Duration,
}

impl Rcon {
    /// Connects and authenticates.
    pub async fn connect(addr: &str, password: &str, io_timeout: Duration) -> Result<Self, RconError> {
        let stream = timeout(io_timeout, TcpStream::connect(addr)).await.map_err(|_| RconError::Timeout)??;
        stream.set_nodelay(true)?;
        let mut rcon = Rcon { stream, next_id: 1, io_timeout };
        let id = rcon.send(KIND_LOGIN, password).await?;
        let reply = rcon.read_packet(rcon.io_timeout).await?;
        if reply.id == -1 || reply.id != id {
            return Err(RconError::AuthFailed);
        }
        Ok(rcon)
    }

    /// Runs a command and returns the server's full reply text.
    pub async fn command(&mut self, command: &str) -> Result<String, RconError> {
        let id = self.send(KIND_COMMAND, command).await?;
        let first = self.read_packet(self.io_timeout).await?;
        if first.id != id {
            return Err(RconError::Protocol(format!("expected reply {id}, got {}", first.id)));
        }
        let mut body = first.body;
        let mut last_len = body.len();
        // A full-size fragment means more may follow; drain them briefly.
        while last_len >= MAX_FRAGMENT {
            match self.read_packet(Duration::from_millis(150)).await {
                Ok(next) if next.id == id => {
                    last_len = next.body.len();
                    body.push_str(&next.body);
                }
                Ok(_) | Err(RconError::Timeout) => break,
                Err(err) => return Err(err),
            }
        }
        Ok(body)
    }

    async fn send(&mut self, kind: i32, body: &str) -> Result<i32, RconError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let packet = Packet { id, kind, body: body.to_owned() }.encode();
        timeout(self.io_timeout, self.stream.write_all(&packet)).await.map_err(|_| RconError::Timeout)??;
        Ok(id)
    }

    async fn read_packet(&mut self, wait: Duration) -> Result<Packet, RconError> {
        let mut len = [0u8; 4];
        timeout(wait, self.stream.read_exact(&mut len)).await.map_err(|_| RconError::Timeout)??;
        let len = i32::from_le_bytes(len);
        if len < 10 || len as usize > MAX_PACKET {
            return Err(RconError::Protocol(format!("bad packet length {len}")));
        }
        let mut payload = vec![0u8; len as usize];
        timeout(self.io_timeout, self.stream.read_exact(&mut payload)).await.map_err(|_| RconError::Timeout)??;
        Packet::decode(&payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn packet_round_trips() {
        let packet = Packet { id: 7, kind: KIND_COMMAND, body: "list".into() };
        let bytes = packet.encode();
        assert_eq!(i32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize, bytes.len() - 4);
        assert_eq!(Packet::decode(&bytes[4..]).unwrap(), packet);
    }

    #[test]
    fn rejects_short_packets() {
        assert!(Packet::decode(&[0; 4]).is_err());
    }

    /// A tiny fake Minecraft RCON server: checks the password, echoes
    /// commands, and splits a long reply into 4096-byte fragments.
    async fn fake_server(password: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            loop {
                let mut len = [0u8; 4];
                if socket.read_exact(&mut len).await.is_err() {
                    return;
                }
                let mut payload = vec![0u8; i32::from_le_bytes(len) as usize];
                socket.read_exact(&mut payload).await.unwrap();
                let packet = Packet::decode(&payload).unwrap();
                let replies = match packet.kind {
                    KIND_LOGIN if packet.body == password => {
                        vec![Packet { id: packet.id, kind: 2, body: String::new() }]
                    }
                    KIND_LOGIN => vec![Packet { id: -1, kind: 2, body: String::new() }],
                    _ if packet.body == "long" => {
                        let text = "x".repeat(MAX_FRAGMENT + 100);
                        vec![
                            Packet { id: packet.id, kind: 0, body: text[..MAX_FRAGMENT].to_owned() },
                            Packet { id: packet.id, kind: 0, body: text[MAX_FRAGMENT..].to_owned() },
                        ]
                    }
                    _ => vec![Packet { id: packet.id, kind: 0, body: format!("ran {}", packet.body) }],
                };
                for reply in replies {
                    socket.write_all(&reply.encode()).await.unwrap();
                }
            }
        });
        addr
    }

    #[tokio::test]
    async fn authenticates_and_runs_commands() {
        let addr = fake_server("hunter2").await;
        let mut rcon = Rcon::connect(&addr, "hunter2", Duration::from_secs(2)).await.unwrap();
        assert_eq!(rcon.command("list").await.unwrap(), "ran list");
        assert_eq!(rcon.command("long").await.unwrap().len(), MAX_FRAGMENT + 100);
    }

    #[tokio::test]
    async fn wrong_password_is_rejected() {
        let addr = fake_server("hunter2").await;
        let result = Rcon::connect(&addr, "nope", Duration::from_secs(2)).await;
        assert!(matches!(result, Err(RconError::AuthFailed)));
    }
}
