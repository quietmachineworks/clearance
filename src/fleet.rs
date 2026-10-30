//! What the user arranges and what clearance keeps (ADR-0015): groups made by hand, sessions kept once
//! they stop until the user deletes them, a name for each session that has none. Pure: the server
//! saves `to_json` in `~/.clearance/fleet.json` and sends it to the page.

use crate::json::{Json, obj};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
}

/// A session that no longer runs, with what it takes to resume it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stopped {
    pub id: String,
    pub name: String,
    pub cwd: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub at: u64,
}

#[derive(Default)]
pub struct Fleet {
    groups: Vec<Group>,
    members: HashMap<String, String>, // session -> group id
    stopped: Vec<Stopped>,            // newest first
    names: HashMap<String, String>,   // session -> name clearance gave it
}

const MAX_NAME: usize = 40;

fn id_of(s: &Json) -> &str {
    s.get("sessionId").str().unwrap_or("")
}

fn text(v: &Json) -> Option<String> {
    v.str().filter(|s| !s.is_empty()).map(String::from)
}

fn folder_name(cwd: &str) -> &str {
    cwd.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or("session")
}

fn checked(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.contains(['\r', '\n']) {
        return Err("A group needs a name of 1 to 40 characters on one line.");
    }
    Ok(name.to_string())
}

impl Fleet {
    pub fn load(saved: &Json) -> Fleet {
        let groups = saved.get("groups").items().iter().filter_map(|g| Some(Group { id: text(g.get("id"))?, name: text(g.get("name"))? })).collect::<Vec<_>>();
        let known: HashSet<&str> = groups.iter().map(|g| g.id.as_str()).collect();
        let members = saved.get("members").entries().iter().filter_map(|(s, g)| g.str().filter(|g| known.contains(g)).map(|g| (s.clone(), g.to_string()))).collect();
        let stopped = saved
            .get("stopped")
            .items()
            .iter()
            .filter_map(|x| {
                Some(Stopped {
                    id: text(x.get("id"))?,
                    name: text(x.get("name"))?,
                    cwd: text(x.get("cwd"))?,
                    title: text(x.get("title")),
                    model: text(x.get("model")),
                    mode: text(x.get("mode")),
                    at: x.get("at").num().unwrap_or(0.0) as u64,
                })
            })
            .collect();
        let names = saved.get("names").entries().iter().filter_map(|(s, n)| n.str().map(|n| (s.clone(), n.to_string()))).collect();
        Fleet { groups, members, stopped, names }
    }

    pub fn to_json(&self) -> Json {
        let mut members: Vec<(&String, &String)> = self.members.iter().collect();
        members.sort();
        let mut names: Vec<(&String, &String)> = self.names.iter().collect();
        names.sort();
        obj([
            ("groups", Json::Arr(self.groups.iter().map(|g| obj([("id", g.id.as_str().into()), ("name", g.name.as_str().into())])).collect())),
            ("members", Json::Obj(members.into_iter().map(|(s, g)| (s.clone(), g.as_str().into())).collect())),
            (
                "stopped",
                Json::Arr(
                    self.stopped
                        .iter()
                        .map(|x| {
                            obj([
                                ("id", x.id.as_str().into()),
                                ("name", x.name.as_str().into()),
                                ("cwd", x.cwd.as_str().into()),
                                ("title", x.title.as_deref().into()),
                                ("model", x.model.as_deref().into()),
                                ("mode", x.mode.as_deref().into()),
                                ("at", x.at.into()),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("names", Json::Obj(names.into_iter().map(|(s, n)| (s.clone(), n.as_str().into())).collect())),
        ])
    }

    pub fn create(&mut self, name: &str) -> Result<String, &'static str> {
        let name = checked(name)?;
        let next = self.groups.iter().filter_map(|g| g.id.strip_prefix('g')?.parse::<u64>().ok()).max().unwrap_or(0) + 1;
        let id = format!("g{next}");
        self.groups.push(Group { id: id.clone(), name });
        Ok(id)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), &'static str> {
        let name = checked(name)?;
        let g = self.groups.iter_mut().find(|g| g.id == id).ok_or("No such group.")?;
        g.name = name;
        Ok(())
    }

    /// Deletes a group; its sessions go back to "No group".
    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.groups.len();
        self.groups.retain(|g| g.id != id);
        self.members.retain(|_, g| g != id);
        self.groups.len() != before
    }

    /// Puts a session in a group, or in none.
    pub fn place(&mut self, session: &str, group: Option<&str>) -> Result<(), &'static str> {
        match group {
            Some(g) if self.groups.iter().any(|x| x.id == g) => {
                self.members.insert(session.to_string(), g.to_string());
                Ok(())
            }
            Some(_) => Err("No such group."),
            None => {
                self.members.remove(session);
                Ok(())
            }
        }
    }

    /// Forgets a stopped session in clearance; its transcript is not touched.
    pub fn forget(&mut self, session: &str) -> bool {
        let before = self.stopped.len();
        self.stopped.retain(|x| x.id != session);
        if self.stopped.len() == before {
            return false;
        }
        self.members.remove(session);
        self.names.remove(session);
        true
    }

    pub fn is_stopped(&self, session: &str) -> bool {
        self.stopped.iter().any(|x| x.id == session)
    }

    /// A new reading of `claude agents`: sessions gone are kept as stopped, unless the same process goes on
    /// under a new id (`/clear`, `/resume`), which then keeps the group and the name; sessions running
    /// again leave the stopped list; sessions without a name get one. Whether anything changed.
    /// ponytail: a session that ends while clearance is stopped is not seen leaving, so it is not kept.
    pub fn observe(&mut self, before: &[Json], now: &[Json], at: u64) -> bool {
        let mut changed = false;
        let was: HashSet<&str> = before.iter().map(id_of).collect();
        let is: HashSet<&str> = now.iter().map(id_of).collect();
        for b in before.iter().filter(|b| !is.contains(id_of(b))) {
            let (old, pid) = (id_of(b), b.get("pid").num());
            let next = now.iter().find(|n| pid.is_some() && n.get("pid").num() == pid && !was.contains(id_of(n)));
            if let Some(n) = next {
                let new = id_of(n).to_string();
                if let Some(g) = self.members.remove(old) {
                    self.members.entry(new.clone()).or_insert(g);
                }
                if let Some(name) = self.names.remove(old) {
                    self.names.entry(new).or_insert(name);
                }
            } else if !old.is_empty() && !self.is_stopped(old) {
                let name = text(b.get("name")).or_else(|| self.names.get(old).cloned()).unwrap_or_else(|| folder_name(b.get("cwd").str().unwrap_or("")).to_string());
                self.stopped.insert(
                    0,
                    Stopped {
                        id: old.to_string(),
                        name,
                        cwd: b.get("cwd").str().unwrap_or("").to_string(),
                        title: text(b.get("title")),
                        model: text(b.get("model")),
                        mode: text(b.get("permissionMode")),
                        at,
                    },
                );
            }
            changed = true;
        }
        let before_stopped = self.stopped.len();
        self.stopped.retain(|x| !is.contains(x.id.as_str()));
        changed |= self.stopped.len() != before_stopped;
        for n in now {
            let id = id_of(n);
            if id.is_empty() || text(n.get("name")).is_some() || self.names.contains_key(id) {
                continue;
            }
            let taken: HashSet<String> = self
                .names
                .values()
                .cloned()
                .chain(now.iter().filter_map(|s| text(s.get("name"))))
                .chain(self.stopped.iter().map(|x| x.name.clone()))
                .map(|n| n.to_lowercase())
                .collect();
            let base = folder_name(n.get("cwd").str().unwrap_or(""));
            let name = (1..).map(|i| if i == 1 { base.to_string() } else { format!("{base} {i}") }).find(|c| !taken.contains(&c.to_lowercase())).unwrap_or_default();
            self.names.insert(id.to_string(), name);
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: &str, pid: u32, cwd: &str, name: &str) -> Json {
        let mut j = obj([("sessionId", id.into()), ("pid", u64::from(pid).into()), ("cwd", cwd.into()), ("status", "idle".into())]);
        if !name.is_empty() {
            j.set("name", name.into());
        }
        j
    }

    #[test]
    fn groups_are_made_renamed_and_deleted_by_the_user() {
        let mut f = Fleet::default();
        let a = f.create(" Shop ").unwrap();
        let b = f.create("Docs").unwrap();
        assert_ne!(a, b);
        assert!(f.create("").is_err() && f.create(&"x".repeat(41)).is_err());
        f.place("s1", Some(&a)).unwrap();
        assert!(f.place("s1", Some("g99")).is_err());
        f.rename(&a, "Shop front").unwrap();
        assert!(f.delete(&a));
        let saved = Fleet::load(&f.to_json());
        assert_eq!(saved.groups, [Group { id: b, name: "Docs".into() }]);
        assert!(saved.members.is_empty(), "a deleted group's sessions go back to no group");
    }

    #[test]
    fn a_session_that_leaves_is_kept_stopped_until_deleted() {
        let mut f = Fleet::default();
        let g = f.create("Shop").unwrap();
        let mut one = s("a1", 10, "/work/shop", "fix-cart");
        one.set("title", "Fix the cart".into());
        f.place("a1", Some(&g)).unwrap();
        assert!(!f.observe(&[], std::slice::from_ref(&one), 1), "a named session needs nothing");
        assert!(f.observe(&[one.clone()], &[], 2));
        let stopped = &Fleet::load(&f.to_json()).stopped;
        assert_eq!(stopped[0].name, "fix-cart");
        assert_eq!(stopped[0].title.as_deref(), Some("Fix the cart"));
        assert_eq!(f.members.get("a1"), Some(&g), "it stays in its group");
        // Resumed: it runs again under the same id.
        f.observe(&[], std::slice::from_ref(&one), 3);
        assert!(!f.is_stopped("a1"));
        f.observe(&[one], &[], 4);
        assert!(f.forget("a1"));
        assert!(!f.is_stopped("a1") && !f.members.contains_key("a1"));
        assert!(!f.forget("a1"));
    }

    #[test]
    fn the_same_process_under_a_new_id_keeps_its_group_and_name() {
        let mut f = Fleet::default();
        let g = f.create("Shop").unwrap();
        let before = s("a1", 10, "/work/shop", "");
        f.observe(&[], std::slice::from_ref(&before), 1);
        f.place("a1", Some(&g)).unwrap();
        let after = s("b2", 10, "/work/shop", "");
        f.observe(&[before], std::slice::from_ref(&after), 2);
        assert!(!f.is_stopped("a1"), "a /clear is not a stop");
        assert_eq!(f.members.get("b2"), Some(&g));
        assert_eq!(f.names.get("b2").map(String::as_str), Some("shop"));
    }

    #[test]
    fn sessions_without_a_name_get_their_folder_numbered() {
        let mut f = Fleet::default();
        let list = [s("a", 1, "C:\\work\\shop", ""), s("b", 2, "/work/shop/", ""), s("c", 3, "/work/docs", "shop 3"), s("d", 4, "/work/shop", "")];
        f.observe(&[], &list, 1);
        assert_eq!(f.names.get("a").map(String::as_str), Some("shop"));
        assert_eq!(f.names.get("b").map(String::as_str), Some("shop 2"));
        assert_eq!(f.names.get("d").map(String::as_str), Some("shop 4"), "shop 3 is taken by a real name");
        assert!(!f.names.contains_key("c"));
        assert!(!f.observe(&list, &list, 2), "a name is given once");
    }
}
