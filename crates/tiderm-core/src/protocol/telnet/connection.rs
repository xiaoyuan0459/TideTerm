//! Telnet connection — TCP + RFC 854 NVT negotiation.

use super::config::TelnetConfig;
use crate::error::{Error, Result};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const IAC: u8 = 0xFF;
const DONT: u8 = 0xFE;
const DO: u8 = 0xFD;
const WONT: u8 = 0xFC;
const WILL: u8 = 0xFB;
const SB: u8 = 0xFA;
const SE: u8 = 0xF0;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_TTYPE: u8 = 24;
const OPT_NAWS: u8 = 31;
const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;
/// Upper bound for Telnet sub-negotiation buffers; prevents a malicious peer
/// from growing memory without limit.
const MAX_SB_BUF: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    Normal,
    Iac,
    Will,
    Wont,
    Do,
    Dont,
    Sb,
    SbIac,
}

/// Live telnet connection with background read task.
pub struct TelnetInstance {
    pub(crate) writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    pub(crate) output_rx: tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>,
    pub(crate) alive: Arc<AtomicBool>,
    pub(crate) cols: AtomicU16,
    pub(crate) rows: AtomicU16,
    pub(crate) connected_at: Mutex<Instant>,
    pub(crate) read_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    #[allow(dead_code)]
    config: TelnetConfig,
    #[allow(dead_code)]
    terminal_type: String,
}

impl TelnetInstance {
    pub async fn connect(config: TelnetConfig, cols: u16, rows: u16) -> Result<Self> {
        config.validate()?;
        let addr = format!("{}:{}", config.host, config.port);
        let timeout = Duration::from_secs(config.connect_timeout_secs);
        let stream = tokio::time::timeout(timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| Error::Telnet(format!("connection timed out: {addr}")))?
            .map_err(|e| Error::Telnet(format!("connect {addr}: {e}")))?;
        let (mut reader, writer) = stream.into_split();
        let writer = Arc::new(tokio::sync::Mutex::new(writer));
        let terminal_type = config.terminal_type.clone();
        let (tx, rx) = mpsc::channel(256);
        let ttype_clone = terminal_type.clone();
        let negotiation_writer = writer.clone();
        // P2-4: shared liveness flag — read_task flips it on any exit path
        // (EOF/error/send failure) so `is_alive()` reflects reality.
        let alive = Arc::new(AtomicBool::new(true));
        let alive_clone = alive.clone();
        let read_task = tokio::spawn(async move {
            let mut buf = [0u8; 8192];
            let mut state = ParserState::Normal;
            let mut sb_buf: Vec<u8> = Vec::new();
            loop {
                let n = match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let mut output = Vec::new();
                for &byte in &buf[..n] {
                    match state {
                        ParserState::Normal => {
                            if byte == IAC {
                                state = ParserState::Iac;
                            } else {
                                output.push(byte);
                            }
                        }
                        ParserState::Iac => match byte {
                            WILL => state = ParserState::Will,
                            WONT => state = ParserState::Wont,
                            DO => state = ParserState::Do,
                            DONT => state = ParserState::Dont,
                            SB => {
                                state = ParserState::Sb;
                                sb_buf.clear();
                            }
                            IAC => {
                                output.push(0xFF);
                                state = ParserState::Normal;
                            }
                            _ => state = ParserState::Normal,
                        },
                        ParserState::Will => {
                            let _ =
                                write_negotiation(&negotiation_writer, &response_for_will(byte))
                                    .await;
                            state = ParserState::Normal;
                        }
                        ParserState::Wont => state = ParserState::Normal,
                        ParserState::Do => {
                            let _ = write_negotiation(&negotiation_writer, &response_for_do(byte))
                                .await;
                            state = ParserState::Normal;
                        }
                        ParserState::Dont => state = ParserState::Normal,
                        ParserState::Sb => {
                            if byte == IAC {
                                state = ParserState::SbIac;
                            } else if sb_buf.len() < MAX_SB_BUF {
                                sb_buf.push(byte);
                            } else {
                                // Sub-negotiation exceeded the safety limit; drop the
                                // connection rather than buffering unbounded data.
                                break;
                            }
                        }
                        ParserState::SbIac => {
                            if byte == SE {
                                // NOTE: `cols`/`rows` here are connect-time
                                // copies; a server-initiated NAWS request
                                // after a resize replies with the stale size.
                                // The proactive resize path (Protocol::resize)
                                // sends NAWS directly, so this only affects
                                // passive requests.
                                handle_subneg(
                                    &sb_buf,
                                    &ttype_clone,
                                    cols,
                                    rows,
                                    &negotiation_writer,
                                )
                                .await;
                                sb_buf.clear();
                                state = ParserState::Normal;
                            } else {
                                sb_buf.push(IAC);
                                sb_buf.push(byte);
                                state = ParserState::Sb;
                            }
                        }
                    }
                }
                if !output.is_empty() && tx.send(output).await.is_err() {
                    break;
                }
            }
            // P2-4: connection is gone — mark the session dead.
            alive_clone.store(false, Ordering::SeqCst);
        });
        Ok(Self {
            writer,
            output_rx: tokio::sync::Mutex::new(rx),
            alive,
            cols: AtomicU16::new(cols),
            rows: AtomicU16::new(rows),
            connected_at: Mutex::new(Instant::now()),
            read_task: tokio::sync::Mutex::new(Some(read_task)),
            config,
            terminal_type,
        })
    }
}

fn response_for_will(option: u8) -> Vec<u8> {
    match option {
        OPT_ECHO | OPT_SGA => vec![IAC, DO, option],
        _ => vec![IAC, DONT, option],
    }
}

fn response_for_do(option: u8) -> Vec<u8> {
    match option {
        OPT_TTYPE | OPT_NAWS | OPT_SGA => vec![IAC, WILL, option],
        _ => vec![IAC, WONT, option],
    }
}

fn build_ttype_response(ttype: &str) -> Vec<u8> {
    let mut r = vec![IAC, SB, OPT_TTYPE, TTYPE_IS];
    r.extend_from_slice(ttype.as_bytes());
    r.extend_from_slice(&[IAC, SE]);
    r
}

fn build_naws_response(cols: u16, rows: u16) -> Vec<u8> {
    vec![
        IAC,
        SB,
        OPT_NAWS,
        (cols >> 8) as u8,
        (cols & 0xFF) as u8,
        (rows >> 8) as u8,
        (rows & 0xFF) as u8,
        IAC,
        SE,
    ]
}

async fn handle_subneg(
    sb_buf: &[u8],
    ttype: &str,
    cols: u16,
    rows: u16,
    writer: &Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
) {
    if sb_buf.is_empty() {
        return;
    }
    let response = match sb_buf[0] {
        OPT_TTYPE if sb_buf.len() >= 2 && sb_buf[1] == TTYPE_SEND => {
            Some(build_ttype_response(ttype))
        }
        OPT_NAWS => Some(build_naws_response(cols, rows)),
        _ => None,
    };
    if let Some(response) = response {
        let _ = write_negotiation(writer, &response).await;
    }
}

async fn write_negotiation(
    writer: &Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    bytes: &[u8],
) -> Result<()> {
    let mut writer = writer.lock().await;
    writer
        .write_all(bytes)
        .await
        .map_err(|e| Error::Telnet(format!("negotiation write: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| Error::Telnet(format!("negotiation flush: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Protocol;

    #[test]
    fn parser_state_eq() {
        assert_eq!(ParserState::Normal, ParserState::Normal);
        assert_ne!(ParserState::Normal, ParserState::Iac);
    }

    #[test]
    fn telnet_constants() {
        assert_eq!(IAC, 255);
        assert_eq!(WILL, 251);
        assert_eq!(DO, 253);
    }

    #[test]
    fn builds_do_response_for_supported_will_options() {
        assert_eq!(response_for_will(OPT_ECHO), vec![IAC, DO, OPT_ECHO]);
        assert_eq!(response_for_will(OPT_SGA), vec![IAC, DO, OPT_SGA]);
    }

    #[test]
    fn builds_will_response_for_supported_do_options() {
        assert_eq!(response_for_do(OPT_TTYPE), vec![IAC, WILL, OPT_TTYPE]);
        assert_eq!(response_for_do(OPT_NAWS), vec![IAC, WILL, OPT_NAWS]);
    }

    #[test]
    fn builds_terminal_type_subnegotiation_response() {
        assert_eq!(
            build_ttype_response("xterm"),
            vec![IAC, SB, OPT_TTYPE, TTYPE_IS, b'x', b't', b'e', b'r', b'm', IAC, SE],
        );
    }

    #[test]
    fn builds_naws_subnegotiation_response() {
        assert_eq!(
            build_naws_response(80, 24),
            vec![IAC, SB, OPT_NAWS, 0, 80, 0, 24, IAC, SE],
        );
    }

    #[tokio::test]
    async fn connect_fails_unreachable() {
        let cfg = TelnetConfig {
            host: "127.0.0.1".into(),
            port: 1,
            terminal_type: "xterm".into(),
            connect_timeout_secs: 2,
        };
        assert!(TelnetInstance::connect(cfg, 80, 24).await.is_err());
    }

    /// P2-4: when the peer closes the connection, the read task must
    /// flip `alive` to false so `is_alive()` reflects reality.
    #[tokio::test]
    async fn read_task_marks_dead_after_peer_closes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");

        // Accept then immediately drop the stream: the client reads EOF.
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept");
        });

        let cfg = TelnetConfig {
            host: "127.0.0.1".into(),
            port: addr.port(),
            terminal_type: "xterm".into(),
            connect_timeout_secs: 5,
        };
        let inst = TelnetInstance::connect(cfg, 80, 24).await.expect("connect");
        server.await.expect("server task");

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !inst.is_alive() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("alive should flip to false after peer closes");
    }
}
