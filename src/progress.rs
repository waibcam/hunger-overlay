//! Who the quest progression belongs to, and what can be undone (docs/DESIGN_QUESTS.md §2.4, §4.1):
//! - "me": the website account (the website is the reference; the app mirrors /me/tracker);
//! - website allies: read-only, delivered with the tracker (`allies[]`, at most 2, chosen on the website);
//! - local allies: friends without the app or an account, kept on this PC only (`allies.json`).
//! Every committed change is a record of exact "from → to" states (with badge slots), kept in `history.json`
//! (last 20 per person, 7 days), so Undo restores the previous state exactly, cascades included.
//! Files live in %APPDATA%\HungerOverlay; tests never write them.

use crate::quests::{Change, Progress, Write};
use serde::{Deserialize, Serialize};

/// Fixed colours of the two ally slots (never the website's): A1 mint, A2 orchid.
pub const ALLY_COLORS: [(f32, f32, f32); 2] = [(0.373, 0.878, 0.710), (0.898, 0.549, 0.949)];

/// Subject id of the player.
pub const ME: &str = "me";

/// A friend without the app or an account, whose progression is kept on this PC.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalAlly {
    /// Local id ("l_…").
    pub id: String,
    /// Name typed by the player (≤ 24 characters).
    pub name: String,
    /// Done quests.
    #[serde(default)]
    pub done: Vec<String>,
    /// Tracked quests, in tracking order.
    #[serde(default)]
    pub tracked: Vec<String>,
}

impl LocalAlly {
    /// Progression as sets.
    pub fn progress(&self) -> Progress {
        Progress { done: self.done.iter().cloned().collect(), tracked: self.tracked.iter().cloned().collect() }
    }

    /// Applies changes (tracked quests keep their order; new ones go last).
    pub fn apply(&mut self, changes: &[Change]) {
        for (q, w) in changes {
            self.done.retain(|x| x != q);
            match w {
                Write::Done => {
                    self.tracked.retain(|x| x != q);
                    self.done.push(q.clone());
                }
                Write::Tracked => {
                    if !self.tracked.contains(q) {
                        self.tracked.push(q.clone());
                    }
                }
                Write::None => self.tracked.retain(|x| x != q),
            }
        }
    }
}

/// One quest of a change record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Op {
    /// Quest id.
    pub quest: String,
    /// State before.
    pub from: Write,
    /// State after.
    pub to: Write,
    /// Badge slot before (to give an untracked quest its number and colour back on undo).
    #[serde(default)]
    pub slot: Option<usize>,
}

/// One committed change (one user action, cascades included).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    /// Unique id.
    pub id: u64,
    /// "me" or a local ally id.
    pub subject: String,
    /// Short description ("Last Embers done").
    pub label: String,
    /// When (Unix seconds).
    pub at: u64,
    /// Every quest it changed.
    pub ops: Vec<Op>,
}

impl Record {
    /// The changes that undo it.
    pub fn undo_changes(&self) -> Vec<Change> {
        self.ops.iter().map(|o| (o.quest.clone(), o.from)).collect()
    }

    /// Quests whose current state is no longer what this record wrote (changed elsewhere since).
    pub fn conflicts(&self, now: &Progress) -> Vec<String> {
        self.ops.iter().filter(|o| crate::quests::write_state(now, &o.quest) != o.to).map(|o| o.quest.clone()).collect()
    }
}

/// Current Unix time in seconds.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Undo history: the last 20 records per subject, at most 7 days old.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    /// Records, oldest first.
    pub records: Vec<Record>,
}

impl History {
    /// Reads history.json (empty if missing).
    pub fn load() -> History {
        let mut h: History = std::fs::read_to_string(crate::config::data_dir().join("history.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        h.prune();
        h
    }

    /// Writes history.json (never during tests).
    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let _ = std::fs::write(crate::config::data_dir().join("history.json"), serde_json::to_string(self).unwrap_or_default());
    }

    /// Drops records older than 7 days and keeps the last 20 per subject.
    fn prune(&mut self) {
        let limit = now_secs().saturating_sub(7 * 24 * 3600);
        self.records.retain(|r| r.at >= limit);
        let mut count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut keep = vec![false; self.records.len()];
        for (i, r) in self.records.iter().enumerate().rev() {
            let c = count.entry(r.subject.clone()).or_default();
            *c += 1;
            keep[i] = *c <= 20;
        }
        let mut k = keep.into_iter();
        self.records.retain(|_| k.next().unwrap_or(false));
    }

    /// Adds a record and saves.
    pub fn push(&mut self, r: Record) {
        self.records.push(r);
        self.prune();
        self.save();
    }

    /// Removes a record (after it was undone) and saves.
    pub fn remove(&mut self, id: u64) {
        self.records.retain(|r| r.id != id);
        self.save();
    }

    /// Records of a subject, newest first.
    pub fn of(&self, subject: &str) -> Vec<&Record> {
        self.records.iter().rev().filter(|r| r.subject == subject).collect()
    }

    /// Next free record id.
    pub fn next_id(&self) -> u64 {
        self.records.iter().map(|r| r.id).max().unwrap_or(0).max(now_secs() * 1000) + 1
    }
}

/// Local allies file (allies.json).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalAllies {
    /// Allies, in adding order.
    pub allies: Vec<LocalAlly>,
}

impl LocalAllies {
    /// Reads allies.json (empty if missing).
    pub fn load() -> LocalAllies {
        std::fs::read_to_string(crate::config::data_dir().join("allies.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Writes allies.json (never during tests).
    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let _ = std::fs::write(crate::config::data_dir().join("allies.json"), serde_json::to_string_pretty(self).unwrap_or_default());
    }

    /// Ally by id.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut LocalAlly> {
        self.allies.iter_mut().find(|a| a.id == id)
    }
}

/// The player's progression from the tracker: done = `completed` (v1.2) plus quests flagged completed; tracked = the
/// others of `tracked[]`.
pub fn me_progress(t: &crate::model::Tracker) -> Progress {
    let mut p = Progress::default();
    if let Some(c) = &t.completed {
        p.done.extend(c.iter().cloned());
    }
    for q in &t.tracked {
        if q.completed {
            p.done.insert(q.quest_id.clone());
        } else if !p.done.contains(&q.quest_id) {
            p.tracked.insert(q.quest_id.clone());
        }
    }
    p
}

/// Initial(s) of an ally: the first letter, or two letters when both allies start with the same one.
pub fn initials(name: &str, other: Option<&str>) -> String {
    let first: String = name.chars().take(1).collect::<String>().to_uppercase();
    let same = other.map(|o| o.chars().next().map(|c| c.to_uppercase().to_string()) == Some(first.clone())).unwrap_or(false);
    if same {
        let mut c = name.chars();
        let a = c.next().map(|x| x.to_uppercase().to_string()).unwrap_or_default();
        let b = c.next().map(|x| x.to_lowercase().to_string()).unwrap_or_default();
        format!("{a}{b}")
    } else {
        first
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record's undo restores the exact previous states; history keeps 20 per subject.
    #[test]
    fn record_and_history() {
        let mut a = LocalAlly { id: "l_1".into(), name: "Lou".into(), done: vec![], tracked: vec!["Q1".into()] };
        let before = a.progress();
        let ch = vec![("Q0".to_string(), Write::Done), ("Q1".to_string(), Write::Done)];
        let r = Record {
            id: 1,
            subject: "l_1".into(),
            label: "x".into(),
            at: now_secs(),
            ops: ch.iter().map(|(q, w)| Op { quest: q.clone(), from: crate::quests::write_state(&before, q), to: *w, slot: None }).collect(),
        };
        a.apply(&ch);
        assert!(r.conflicts(&a.progress()).is_empty());
        a.apply(&r.undo_changes());
        assert_eq!(a.progress(), before);
        let mut h = History::default();
        for i in 0..25 {
            h.push(Record { id: i, subject: "me".into(), label: String::new(), at: now_secs(), ops: vec![] });
        }
        assert_eq!(h.of("me").len(), 20);
        assert_eq!(initials("Marius", Some("Mila")), "Ma");
        assert_eq!(initials("lou", Some("Marius")), "L");
    }
}
