//! Who may use the page: single-use login links, then a session of its own for each browser.
//! The hook secret never opens the page; a browser's cookie is a random id of which clearance keeps only
//! the SHA-256, so reading `browsers.json` gives no working cookie.

use crate::json::{Json, obj};
use crate::sys;
use std::collections::HashMap;

pub const LINK_MS: u64 = 5 * 60_000;
const IDLE_MS: u64 = 30 * 24 * 3_600_000; // a session unused this long ends
const SEEN_MS: u64 = 3_600_000; // "last used" is written at most once an hour
const MAX_BROWSERS: usize = 50; // beyond, the least recently used are signed out
const MAX_AGENT: usize = 200; // characters of User-Agent kept

#[derive(Default)]
pub struct Auth {
    links: HashMap<String, u64>,      // nonce -> expiry, in memory only
    browsers: HashMap<String, Entry>, // sha256(session id) -> browser
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    created: u64,
    seen: u64,
    agent: String,
}

/// What the page shows to sign a browser out: part of the digest, never the session id.
fn public_id(digest: &str) -> &str {
    &digest[..16]
}

impl Auth {
    /// The browsers kept in `browsers.json`; anything malformed is dropped.
    pub fn load(saved: &Json) -> Auth {
        let browsers = saved
            .entries()
            .iter()
            .filter(|(k, _)| k.len() == 64 && k.bytes().all(|b| b.is_ascii_hexdigit()))
            .filter_map(|(k, e)| {
                Some((k.clone(), Entry { created: e.get("created").num()? as u64, seen: e.get("seen").num()? as u64, agent: e.get("agent").str().unwrap_or("").to_string() }))
            })
            .collect();
        Auth { links: HashMap::new(), browsers }
    }

    pub fn to_json(&self) -> Json {
        Json::Obj(self.browsers.iter().map(|(k, e)| (k.clone(), obj([("created", e.created.into()), ("seen", e.seen.into()), ("agent", e.agent.as_str().into())]))).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.browsers.is_empty()
    }

    /// A fresh single-use link, valid for `LINK_MS`.
    pub fn link(&mut self, now: u64) -> std::io::Result<String> {
        self.links.retain(|_, expiry| *expiry > now);
        let nonce = sys::random_hex(16)?;
        self.links.insert(nonce.clone(), now + LINK_MS);
        Ok(nonce)
    }

    /// Whether the link is valid; it is spent either way.
    pub fn redeem(&mut self, nonce: &str, now: u64) -> bool {
        self.links.remove(nonce).is_some_and(|expiry| expiry > now)
    }

    /// A new session for a browser that just signed in: its id, the cookie value.
    pub fn sign_in(&mut self, agent: &str, now: u64) -> std::io::Result<String> {
        let id = sys::random_hex(32)?;
        self.browsers.insert(sys::sha256(id.as_bytes()), Entry { created: now, seen: now, agent: agent.chars().take(MAX_AGENT).collect() });
        self.prune(now);
        Ok(id)
    }

    /// The session's digest when the id is valid, and whether the browsers changed (to save them).
    pub fn check(&mut self, id: &str, now: u64) -> (Option<String>, bool) {
        let digest = sys::sha256(id.as_bytes());
        let Some(entry) = self.browsers.get_mut(&digest) else { return (None, false) };
        if now.saturating_sub(entry.seen) > IDLE_MS {
            self.browsers.remove(&digest);
            return (None, true);
        }
        let stale = now.saturating_sub(entry.seen) >= SEEN_MS;
        if stale {
            entry.seen = now;
        }
        (Some(digest), stale)
    }

    /// Signed-in browsers, most recently used first; `current` is the asking browser's digest.
    pub fn list(&self, current: &str) -> Json {
        let mut all: Vec<(&String, &Entry)> = self.browsers.iter().collect();
        all.sort_by_key(|a| std::cmp::Reverse(a.1.seen));
        Json::Arr(
            all.into_iter()
                .map(|(k, e)| {
                    obj([
                        ("id", public_id(k).into()),
                        ("created", e.created.into()),
                        ("seen", e.seen.into()),
                        ("agent", e.agent.as_str().into()),
                        ("current", (k == current).into()),
                    ])
                })
                .collect(),
        )
    }

    /// Ends the sessions shown under `public`, or every one but `keep`; the digests that ended.
    pub fn sign_out(&mut self, public: Option<&str>, keep: &str) -> Vec<String> {
        let gone: Vec<String> = self.browsers.keys().filter(|k| public.map_or(k.as_str() != keep, |p| public_id(k) == p)).cloned().collect();
        for k in &gone {
            self.browsers.remove(k);
        }
        gone
    }

    fn prune(&mut self, now: u64) {
        self.browsers.retain(|_, e| now.saturating_sub(e.seen) <= IDLE_MS);
        if self.browsers.len() > MAX_BROWSERS {
            let mut by_use: Vec<(String, u64)> = self.browsers.iter().map(|(k, e)| (k.clone(), e.seen)).collect();
            by_use.sort_by_key(|a| std::cmp::Reverse(a.1));
            for (k, _) in by_use.into_iter().skip(MAX_BROWSERS) {
                self.browsers.remove(&k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_800_000_000_000;

    #[test]
    fn a_link_works_once_and_not_after_five_minutes() {
        let mut a = Auth::default();
        let link = a.link(T).unwrap();
        assert!(a.redeem(&link, T + 1000));
        assert!(!a.redeem(&link, T + 2000), "used twice");
        let late = a.link(T).unwrap();
        assert!(!a.redeem(&late, T + LINK_MS), "expired");
        assert!(!a.redeem(&late, T), "an expired link is spent too");
        assert!(!a.redeem("", T));
    }

    #[test]
    fn a_session_is_kept_as_a_digest_only() {
        let mut a = Auth::default();
        let id = a.sign_in("Firefox", T).unwrap();
        let saved = a.to_json().to_string();
        assert!(!saved.contains(&id), "the cookie value is never written");
        let mut back = Auth::load(&Json::parse(&saved).unwrap());
        let (digest, changed) = back.check(&id, T + 60_000);
        assert_eq!(digest.as_deref(), Some(sys::sha256(id.as_bytes()).as_str()));
        assert!(!changed, "last use is noted at most hourly");
        assert!(back.check(&id, T + SEEN_MS).1);
        assert_eq!(back.check(&saved, T).0, None, "the saved file is not a cookie");
    }

    #[test]
    fn a_session_unused_for_thirty_days_ends() {
        let mut a = Auth::default();
        let id = a.sign_in("", T).unwrap();
        assert_eq!(a.check(&id, T + IDLE_MS + 1), (None, true));
        assert!(a.is_empty());
    }

    #[test]
    fn signing_out_one_browser_or_all_the_others() {
        let mut a = Auth::default();
        let (one, two, three) = (a.sign_in("a", T).unwrap(), a.sign_in("b", T + 1).unwrap(), a.sign_in("c", T + 2).unwrap());
        let mine = a.check(&one, T).0.unwrap();
        let list = a.list(&mine);
        assert_eq!(list.items().len(), 3);
        assert_eq!(list.items()[0].get("agent").str(), Some("c"), "most recent first");
        let two_public = list.items()[1].get("id").str().unwrap().to_string();
        assert_eq!(a.sign_out(Some(&two_public), &mine).len(), 1);
        assert_eq!(a.check(&two, T).0, None);
        assert_eq!(a.sign_out(None, &mine).len(), 1);
        assert_eq!(a.check(&three, T).0, None);
        assert!(a.check(&one, T).0.is_some());
    }

    #[test]
    fn a_broken_file_gives_no_browser() {
        assert!(Auth::load(&Json::parse(r#"{"short":{"created":1,"seen":1},"x":3}"#).unwrap()).is_empty());
    }
}
