use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use abnegate_notify::{Sender, SmtpConfig, SmtpSecurity};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

pub const SENDER: &str = "licenses@zone.test";

const GREETING: &str = "220 sink ready\r\n";
const OK: &str = "250 OK\r\n";
const START_DATA: &str = "354 end data with <CR><LF>.<CR><LF>\r\n";
const CLOSING: &str = "221 bye\r\n";
const UNKNOWN: &str = "502 command not implemented\r\n";
const END_OF_DATA: &str = ".\r\n";
const BOUNDARY: &str = "boundary=\"";
const BLANK_LINE: &str = "\r\n\r\n";
const CONTENT_TYPE: &str = "content-type:";
const BASE64: &str = "content-transfer-encoding: base64";

/// One message the sink accepted: who it was for and the DATA exactly as
/// sent, dot-unstuffed.
#[derive(Clone, Debug)]
pub struct Delivered {
    pub recipients: Vec<String>,
    pub data: String,
}

/// One leaf of a MIME message, its body decoded.
#[derive(Clone, Debug)]
pub struct Part {
    pub content_type: String,
    pub body: String,
}

impl Delivered {
    pub fn parts(&self) -> Vec<Part> {
        let Some(boundary) = self
            .data
            .split_once(BOUNDARY)
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(boundary, _)| format!("--{boundary}"))
        else {
            return vec![part(&self.data)];
        };
        self.data
            .split(&boundary)
            .skip(1)
            .take_while(|section| !section.starts_with("--"))
            .map(part)
            .collect()
    }

    pub fn part(&self, content_type: &str) -> Option<String> {
        self.parts()
            .into_iter()
            .find(|part| part.content_type.starts_with(content_type))
            .map(|part| part.body)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        let prefix = format!("{}:", name.to_ascii_lowercase());
        self.data
            .split(BLANK_LINE)
            .next()?
            .split("\r\n")
            .find(|line| line.to_ascii_lowercase().starts_with(&prefix))
            .map(|line| line[prefix.len()..].trim())
    }
}

fn part(section: &str) -> Part {
    let (headers, body) = section.split_once(BLANK_LINE).unwrap_or((section, ""));
    let headers = headers.to_ascii_lowercase();
    let content_type = headers
        .lines()
        .find_map(|line| line.trim().strip_prefix(CONTENT_TYPE))
        .unwrap_or_default()
        .trim()
        .to_string();
    let body = if headers.lines().any(|line| line.trim() == BASE64) {
        let encoded: String = body.split_whitespace().collect();
        String::from_utf8(STANDARD.decode(encoded).expect("base64 body")).expect("utf-8 body")
    } else {
        body.trim_end_matches("\r\n").to_string()
    };
    Part { content_type, body }
}

/// A local SMTP server that accepts every message, or refuses every
/// recipient with one reply, and records what it accepted.
pub struct SmtpSink {
    address: SocketAddr,
    messages: Arc<Mutex<Vec<Delivered>>>,
    task: JoinHandle<()>,
}

impl SmtpSink {
    pub async fn start() -> Self {
        Self::listen(None).await
    }

    /// Answers every `RCPT` with `reply`, such as `"550 5.1.1 no such user"`
    /// or `"451 4.3.0 try again later"`.
    pub async fn rejecting(reply: &'static str) -> Self {
        Self::listen(Some(reply)).await
    }

    async fn listen(rejection: Option<&'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind sink");
        let address = listener.local_addr().expect("sink address");
        let messages = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&messages);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(converse(stream, rejection, Arc::clone(&recorded)));
            }
        });
        Self {
            address,
            messages,
            task,
        }
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// An unauthenticated plaintext relay config pointing at this sink.
    pub fn config(&self) -> SmtpConfig {
        SmtpConfig::unauthenticated(
            self.address.ip().to_string(),
            self.address.port(),
            Sender::new(SENDER).with_name("Zone"),
        )
        .with_security(SmtpSecurity::Plaintext)
    }

    pub fn messages(&self) -> Vec<Delivered> {
        self.messages.lock().expect("sink messages").clone()
    }
}

impl Drop for SmtpSink {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn converse(
    stream: TcpStream,
    rejection: Option<&'static str>,
    messages: Arc<Mutex<Vec<Delivered>>>,
) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut recipients = Vec::new();
    if writer.write_all(GREETING.as_bytes()).await.is_err() {
        return;
    }
    while let Some(line) = read_line(&mut reader).await {
        let command = line.trim_end().to_ascii_uppercase();
        let verb = command.split([' ', ':']).next().unwrap_or_default();
        let reply = match verb {
            "EHLO" | "HELO" | "MAIL" | "NOOP" => OK.to_string(),
            "RSET" => {
                recipients.clear();
                OK.to_string()
            }
            "RCPT" => match rejection {
                Some(reply) => format!("{reply}\r\n"),
                None => {
                    recipients.push(recipient(line.trim_end()));
                    OK.to_string()
                }
            },
            "DATA" => {
                if writer.write_all(START_DATA.as_bytes()).await.is_err() {
                    return;
                }
                let Some(data) = read_data(&mut reader).await else {
                    return;
                };
                messages.lock().expect("sink messages").push(Delivered {
                    recipients: std::mem::take(&mut recipients),
                    data,
                });
                OK.to_string()
            }
            "QUIT" => {
                let _ = writer.write_all(CLOSING.as_bytes()).await;
                return;
            }
            _ => UNKNOWN.to_string(),
        };
        if writer.write_all(reply.as_bytes()).await.is_err() {
            return;
        }
    }
}

async fn read_line(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Option<String> {
    let mut line = Vec::new();
    match reader.read_until(b'\n', &mut line).await {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(String::from_utf8_lossy(&line).into_owned()),
    }
}

async fn read_data(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Option<String> {
    let mut data = String::new();
    loop {
        let line = read_line(reader).await?;
        if line == END_OF_DATA {
            return Some(data);
        }
        data.push_str(line.strip_prefix('.').unwrap_or(&line));
    }
}

fn recipient(command: &str) -> String {
    let address = command
        .split_once(':')
        .map_or(command, |(_, address)| address)
        .trim();
    address
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}
