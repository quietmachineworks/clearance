//! The HTTP/1.1 subset clearance needs on 127.0.0.1 (ADR-0010): one request per connection, bodies with
//! `Content-Length`, responses that close the connection, server-sent events.

use crate::json::Json;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;

const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 32 * 1024 * 1024; // a Write of a large file comes through a hook

pub struct Request {
    pub method: String,
    pub path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

impl Request {
    pub fn read(stream: &TcpStream) -> io::Result<Request> {
        let mut reader = BufReader::new(stream);
        let mut head = Vec::new();
        let mut total = 0;
        loop {
            let mut line = String::new();
            let n = Read::by_ref(&mut reader).take((MAX_HEAD - total) as u64 + 1).read_line(&mut line)?;
            total += n;
            if n == 0 || total > MAX_HEAD {
                return Err(bad("no complete request head"));
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }
            head.push(line.to_string());
        }
        let mut first = head.first().ok_or_else(|| bad("empty request"))?.split(' ');
        let (method, target) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("/"));
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let headers: Vec<(String, String)> = head[1..].iter().filter_map(|h| h.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
        let len = headers.iter().find(|(k, _)| k == "content-length").map_or(Ok(0), |(_, v)| v.parse::<usize>().map_err(|_| bad("bad content-length")))?;
        if len > MAX_BODY {
            return Err(bad("body too large"));
        }
        let mut body = vec![0; len];
        reader.read_exact(&mut body)?;
        let query = query.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').unwrap_or((p, ""))).map(|(k, v)| (decode(k, true), decode(v, true))).collect();
        Ok(Request { method, path: decode(path, false), query, headers, body })
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.header("cookie")?.split(';').filter_map(|c| c.trim().split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v)
    }

    /// The body as JSON; an empty object when it is not.
    pub fn json(&self) -> Json {
        std::str::from_utf8(&self.body).ok().and_then(|t| Json::parse(t).ok()).unwrap_or(Json::Obj(Vec::new()))
    }
}

/// Percent-decoding; `+` is a space in a query string.
fn decode(s: &str, plus: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |at: usize| s.get(at..at + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        match b[i] {
            b'%' if hex(i + 1).is_some() => {
                out.extend(hex(i + 1));
                i += 3;
            }
            b'+' if plus => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        303 => "See Other",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        410 => "Gone",
        421 => "Misdirected Request",
        _ => "Internal Server Error",
    }
}

pub fn respond(mut stream: &TcpStream, status: u16, headers: &[(&str, &str)], body: &[u8]) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("content-length: {}\r\nconnection: close\r\n\r\n", body.len()));
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

pub fn json(stream: &TcpStream, status: u16, body: &Json) -> io::Result<()> {
    respond(stream, status, &[("content-type", "application/json")], body.to_string().as_bytes())
}

/// Starts a server-sent events stream; events follow as `data: <json>` blocks.
pub fn events(mut stream: &TcpStream) -> io::Result<()> {
    stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-store\r\nconnection: keep-alive\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn reads_a_request_and_decodes_it() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::thread::spawn(move || {
            let mut c = TcpStream::connect(addr).unwrap();
            c.write_all(
                b"POST /api/folders%20x?path=C%3A%5Cwork+now&token=a HTTP/1.1\r\nHost: 127.0.0.1:1\r\nCookie: a=1; clearance=t0k\r\nContent-Length: 11\r\n\r\n{\"id\":\"7\"}\n",
            )
            .unwrap();
            let mut answer = String::new();
            c.read_to_string(&mut answer).unwrap();
            answer
        });
        let (s, _) = listener.accept().unwrap();
        let r = Request::read(&s).unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/api/folders x"));
        assert_eq!(r.param("path"), Some("C:\\work now"));
        assert_eq!(r.cookie("clearance"), Some("t0k"));
        assert_eq!(r.header("host"), Some("127.0.0.1:1"));
        assert_eq!(r.json().get("id").str(), Some("7"));
        json(&s, 410, &crate::json::obj([("error", "gone".into())])).unwrap();
        drop(s);
        let answer = client.join().unwrap();
        assert!(answer.starts_with("HTTP/1.1 410 Gone\r\n"));
        assert!(answer.ends_with("\r\n\r\n{\"error\":\"gone\"}"));
    }
}
