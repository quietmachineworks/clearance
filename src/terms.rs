//! The sessions clearance runs in its own pseudo-terminals: what each one draws is kept (the
//! last 512 KB) and streamed to the pages that show it; keys and size come back from them.

use crate::json::{Json, obj};
use crate::pty::Pty;
use crate::sys;
use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, MutexGuard};

const KEPT: usize = 512 * 1024;

/// How a terminal was started: what it takes to start it again.
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub name: String,
    pub cwd: String,
    pub model: String,
    pub mode: String,
    pub group: Option<String>,
    pub resume: Option<String>, // the session it was started on, until it shows up with its own
}

pub struct Term {
    pub id: String,
    pub launch: Launch,
    pty: Pty,
    screen: Mutex<Screen>,
}

#[derive(Default)]
struct Screen {
    kept: VecDeque<u8>,
    pages: Vec<(String, TcpStream)>, // browser digest, open stream
}

#[derive(Default)]
pub struct Terms {
    map: Mutex<HashMap<String, Arc<Term>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One server-sent event carrying bytes.
fn event(bytes: &[u8]) -> String {
    format!("data: {}\n\n", sys::base64(bytes))
}

impl Term {
    pub fn pid(&self) -> u32 {
        self.pty.pid()
    }

    fn drawn(&self, bytes: &[u8]) {
        let mut s = lock(&self.screen);
        s.kept.extend(bytes);
        let extra = s.kept.len().saturating_sub(KEPT);
        s.kept.drain(..extra);
        let msg = event(bytes);
        s.pages.retain(|(_, p)| {
            let mut w: &TcpStream = p;
            w.write_all(msg.as_bytes()).is_ok()
        });
    }

    fn ended(&self) {
        let mut s = lock(&self.screen);
        for (_, p) in s.pages.drain(..) {
            let mut w: &TcpStream = &p;
            let _ = w.write_all(b"event: end\ndata: {}\n\n");
            let _ = p.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Terms {
    /// Starts `program args` in a new terminal; `gone` runs once it has ended.
    pub fn start(self: &Arc<Self>, program: &str, args: &[String], launch: Launch, gone: impl FnOnce(&str) + Send + 'static) -> io::Result<Arc<Term>> {
        let env = crate::terminal::session_env();
        let (pty, mut out) = Pty::spawn(program, args, &launch.cwd, &env, 120, 32)?;
        let id = sys::random_hex(8)?;
        let term = Arc::new(Term { id: id.clone(), launch, pty, screen: Mutex::default() });
        lock(&self.map).insert(id.clone(), term.clone());
        let waiter = term.clone();
        std::thread::spawn(move || waiter.pty.wait());
        let (reader, terms) = (term.clone(), self.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    break;
                }
                reader.drawn(&buf[..n]);
            }
            reader.ended();
            lock(&terms.map).remove(&reader.id);
            gone(&reader.id);
        });
        Ok(term)
    }

    pub fn get(&self, id: &str) -> Option<Arc<Term>> {
        lock(&self.map).get(id).cloned()
    }

    pub fn all(&self) -> Vec<Arc<Term>> {
        lock(&self.map).values().cloned().collect()
    }

    /// A page opens a terminal: what it kept, then every new chunk.
    pub fn attach(&self, id: &str, browser: &str, stream: &TcpStream) -> io::Result<bool> {
        let Some(term) = self.get(id) else { return Ok(false) };
        let mut s = lock(&term.screen);
        let (a, b) = s.kept.as_slices();
        let mut w = stream;
        crate::http::events(stream)?;
        w.write_all(event(&[a, b].concat()).as_bytes())?;
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
        s.pages.push((browser.to_string(), stream.try_clone()?));
        Ok(true)
    }

    pub fn input(&self, id: &str, data: &str) -> io::Result<bool> {
        let Some(term) = self.get(id) else { return Ok(false) };
        term.pty.write(data.as_bytes())?;
        Ok(true)
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> io::Result<bool> {
        let Some(term) = self.get(id) else { return Ok(false) };
        term.pty.resize(cols, rows)?;
        Ok(true)
    }

    /// Closes the streams of the browsers `gone` names: signed out, or all of them when clearance stops.
    pub fn forget_pages(&self, gone: impl Fn(&str) -> bool) {
        for term in self.all() {
            lock(&term.screen).pages.retain(|(b, p)| {
                let keep = !gone(b);
                if !keep {
                    let _ = p.shutdown(std::net::Shutdown::Both);
                }
                keep
            });
        }
    }
}

impl Launch {
    pub fn to_json(&self, session: Option<&str>) -> Json {
        obj([
            ("session", session.into()),
            ("name", self.name.as_str().into()),
            ("cwd", self.cwd.as_str().into()),
            ("model", self.model.as_str().into()),
            ("mode", self.mode.as_str().into()),
            ("group", self.group.as_deref().into()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminal_keeps_its_output_and_ends_with_its_program() {
        let terms = Arc::new(Terms::default());
        let (tx, rx) = std::sync::mpsc::channel();
        let launch = Launch { name: "t".into(), cwd: std::env::temp_dir().display().to_string(), model: String::new(), mode: String::new(), group: None, resume: None };
        let (program, args): (&str, Vec<String>) =
            if cfg!(windows) { ("cmd", vec!["/c".into(), "echo".into(), "kept-output".into()]) } else { ("sh", vec!["-c".into(), "echo kept-output".into()]) };
        let term = terms.start(program, &args, launch, move |id| tx.send(id.to_string()).unwrap()).unwrap();
        let gone = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(gone, term.id);
        assert!(terms.get(&term.id).is_none(), "an ended terminal leaves the list");
        let kept: Vec<u8> = lock(&term.screen).kept.iter().copied().collect();
        assert!(String::from_utf8_lossy(&kept).contains("kept-output"));
    }
}
