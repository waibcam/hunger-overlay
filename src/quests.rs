//! The player's quest progression (docs/DESIGN_QUESTS.md §2): which quests are done, tracked (in progress),
//! available or still locked, from the prerequisites published by the website (`requires` of each quest).
//!
//! Only two things are stored per person: the done quests (D) and the tracked ones (T); every other state is derived.
//! Invariant kept by every planned change: the ancestors of a quest in D ∪ T are in D, and D ∩ T is empty.
//! - Track / Done: the ancestors not yet done become done (cascade up).
//! - Reopen: every descendant that was done or tracked is reset (cascade down, across characters).
//! - "I am here": up as Track, plus down — before = done, this = in progress, after = not started.
//! Every action is an explicit list of changes (what the preview lists and what is written to the website in one
//! atomic write); `inverse` gives the exact undo. Repeatable quests (daily, weekly, monthly) are never part of a
//! cascade and never proposed as "next". Pure computation: no network, no window.

use std::collections::{HashMap, HashSet};

/// What a person has done: done quests and tracked quests.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// Finished quests.
    pub done: HashSet<String>,
    /// Quests in progress (tracked).
    pub tracked: HashSet<String>,
}

/// State of one quest for a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Finished.
    Done,
    /// In progress (tracked).
    Tracked,
    /// Every prerequisite is done: it can be started.
    Available,
    /// At least one prerequisite is not done yet.
    Locked,
}

/// State written for one quest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Write {
    /// Finished.
    Done,
    /// In progress.
    Tracked,
    /// Neither done nor tracked.
    None,
}

impl Write {
    /// Value of the `state` field of `POST /me/quests`.
    pub fn as_str(self) -> &'static str {
        match self {
            Write::Done => "done",
            Write::Tracked => "tracked",
            Write::None => "none",
        }
    }
}

/// One change of a batch: (quest id, new state).
pub type Change = (String, Write);

/// The first quest to do for a character ("Up next").
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Already in progress.
    Tracked(String),
    /// Can be started now.
    Available(String),
    /// Waits for a quest of another character (the blocking quest).
    Waiting(String, String),
}

/// Prerequisite graph of the quests.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    /// Direct prerequisites of each quest.
    requires: HashMap<String, Vec<String>>,
    /// Direct dependents of each quest (the reverse of `requires`).
    dependents: HashMap<String, Vec<String>>,
    /// Quest giver of each quest.
    giver: HashMap<String, String>,
    /// Quest line and position in it, for ordering.
    line: HashMap<String, (String, u32)>,
    /// Givers in display order (the website's order, then the others by name).
    givers: Vec<String>,
    /// Repeatable quests (daily, weekly, monthly): never in a cascade, never proposed as "next".
    repeatable: HashSet<String>,
}

impl Graph {
    /// Builds the graph from (quest id, giver, line and position, direct prerequisites, repeatable) and the
    /// website's giver order.
    pub fn new(quests: impl IntoIterator<Item = (String, String, Option<(String, u32)>, Vec<String>, bool)>, giver_order: &[String]) -> Graph {
        let mut g = Graph::default();
        for (id, giver, line, req, rep) in quests {
            for r in &req {
                g.dependents.entry(r.clone()).or_default().push(id.clone());
            }
            g.requires.insert(id.clone(), req);
            if let Some(l) = line {
                g.line.insert(id.clone(), l);
            }
            if rep {
                g.repeatable.insert(id.clone());
            }
            g.giver.insert(id, giver);
        }
        for d in g.dependents.values_mut() {
            d.sort();
            d.dedup();
        }
        g.givers = giver_order.iter().filter(|x| g.giver.values().any(|v| v == *x)).cloned().collect();
        let mut others: Vec<String> = g.giver.values().filter(|v| !v.is_empty() && !giver_order.contains(v)).cloned().collect();
        others.sort();
        others.dedup();
        g.givers.extend(others);
        g
    }

    /// Builds the graph from the pack (no prerequisites until the website publishes them: contract v1.2).
    pub fn from_pack(p: &crate::model::Pack) -> Graph {
        Graph::new(
            p.quests.iter().map(|(id, q)| {
                let rep = q.repeatable || q.kind.as_deref().map(|k| k != "story" && k != "standard").unwrap_or(false);
                (id.clone(), q.giver.clone().unwrap_or_default(), q.line.clone().map(|l| (l, q.line_pos.unwrap_or(0))), q.requires.clone(), rep)
            }),
            &p.givers,
        )
    }

    /// True when the website publishes prerequisites (otherwise cascades and "Up next" are unavailable).
    pub fn has_chains(&self) -> bool {
        self.requires.values().any(|r| !r.is_empty())
    }

    /// True for a repeatable quest.
    pub fn is_repeatable(&self, q: &str) -> bool {
        self.repeatable.contains(q)
    }

    /// Direct prerequisites of a quest.
    pub fn requires(&self, q: &str) -> &[String] {
        self.requires.get(q).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Every quest that must be done before `q` (nearest first, `q` excluded).
    pub fn ancestors(&self, q: &str) -> Vec<String> {
        walk(q, &self.requires)
    }

    /// Every quest that needs `q`, directly or not (`q` excluded).
    pub fn descendants(&self, q: &str) -> Vec<String> {
        walk(q, &self.dependents)
    }

    /// Giver of a quest ("" if unknown).
    pub fn giver(&self, q: &str) -> &str {
        self.giver.get(q).map(|s| s.as_str()).unwrap_or("")
    }

    /// Givers in display order.
    pub fn givers(&self) -> &[String] {
        &self.givers
    }

    /// Story quest lines of a giver, in play order (a line that needs another one comes after it).
    pub fn lines_of(&self, giver: &str) -> Vec<Vec<String>> {
        let mut by_line: HashMap<&str, Vec<(u32, &String)>> = HashMap::new();
        for (q, (l, pos)) in &self.line {
            if self.giver(q) == giver && !self.repeatable.contains(q) {
                by_line.entry(l.as_str()).or_default().push((*pos, q));
            }
        }
        let mut lines: Vec<(usize, String, Vec<String>)> = by_line
            .into_iter()
            .map(|(l, mut v)| {
                v.sort();
                let qs: Vec<String> = v.into_iter().map(|(_, q)| q.clone()).collect();
                (qs.first().map(|f| self.ancestors(f).len()).unwrap_or(0), l.to_string(), qs)
            })
            .collect();
        lines.sort();
        lines.into_iter().map(|(_, _, q)| q).collect()
    }

    /// Story quests of a giver outside any line (bridges such as "A Touch Of Bribery"), by depth then id.
    pub fn side_steps(&self, giver: &str) -> Vec<String> {
        let mut v: Vec<&String> = self.giver.iter().filter(|(q, g)| g.as_str() == giver && !self.line.contains_key(*q) && !self.repeatable.contains(*q)).map(|(q, _)| q).collect();
        v.sort_by_key(|q| (self.ancestors(q).len(), (*q).clone()));
        v.into_iter().cloned().collect()
    }

    /// Repeatable quests of a giver, by id.
    pub fn repeatables_of(&self, giver: &str) -> Vec<String> {
        let mut v: Vec<String> = self.giver.iter().filter(|(q, g)| g.as_str() == giver && self.repeatable.contains(*q)).map(|(q, _)| q.clone()).collect();
        v.sort();
        v
    }

    /// Story quests of a giver in play order: line 1, line 2…, then side steps.
    pub fn story(&self, giver: &str) -> Vec<String> {
        let mut v: Vec<String> = self.lines_of(giver).into_iter().flatten().collect();
        v.extend(self.side_steps(giver));
        v
    }

    /// State of a quest for a person.
    pub fn state(&self, p: &Progress, q: &str) -> State {
        if p.done.contains(q) {
            State::Done
        } else if p.tracked.contains(q) {
            State::Tracked
        } else if self.requires(q).iter().all(|x| p.done.contains(x)) {
            State::Available
        } else {
            State::Locked
        }
    }

    /// True when `q` is done or tracked while one of its (non-repeatable) ancestors is not done (old data).
    pub fn inconsistent(&self, p: &Progress, q: &str) -> bool {
        (p.done.contains(q) || p.tracked.contains(q)) && self.ancestors(q).iter().any(|a| !p.done.contains(a) && !self.repeatable.contains(a))
    }

    /// Ancestors that a cascade marks done (already done and repeatable ones excluded), furthest first.
    fn ancestors_to_finish(&self, p: &Progress, q: &str) -> Vec<String> {
        let mut v: Vec<String> = self.ancestors(q).into_iter().filter(|a| !p.done.contains(a) && !self.repeatable.contains(a)).collect();
        v.reverse();
        v
    }

    /// Track: every missing ancestor becomes done, `q` becomes tracked.
    pub fn plan_track(&self, p: &Progress, q: &str) -> Vec<Change> {
        let mut v: Vec<Change> = self.ancestors_to_finish(p, q).into_iter().map(|a| (a, Write::Done)).collect();
        if !p.tracked.contains(q) || p.done.contains(q) {
            v.push((q.to_string(), Write::Tracked));
        }
        v
    }

    /// Done (or "Done up to here"): `q` and every missing ancestor become done. A repeatable quest is only untracked.
    /// As on the website, finishing a TRACKED quest also tracks the story quests it unlocks. Repeatable quests (daily,
    /// weekly, monthly) appear at random in game: they are only ever tracked by hand.
    pub fn plan_done(&self, p: &Progress, q: &str) -> Vec<Change> {
        if self.repeatable.contains(q) {
            return self.plan_untrack(p, q);
        }
        let mut v: Vec<Change> = self.ancestors_to_finish(p, q).into_iter().map(|a| (a, Write::Done)).collect();
        if !p.done.contains(q) {
            v.push((q.to_string(), Write::Done));
        }
        if p.tracked.contains(q) {
            let after = apply(p, &v);
            for d in self.follow_ups(&after, q) {
                v.push((d, Write::Tracked));
            }
        }
        v
    }

    /// Story quests unlocked by `q` (direct dependents whose prerequisites are all done in `after`), not done or
    /// tracked yet. Repeatable quests are never included.
    pub fn follow_ups(&self, after: &Progress, q: &str) -> Vec<String> {
        self.dependents
            .get(q)
            .into_iter()
            .flatten()
            .filter(|d| !self.repeatable.contains(*d) && !after.done.contains(*d) && !after.tracked.contains(*d))
            .filter(|d| self.requires(d).iter().all(|r| after.done.contains(r)))
            .cloned()
            .collect()
    }

    /// True when `changes` finish quests other than `q` (a cascade the player must see before it is written);
    /// the follow-up quests tracked automatically do not count.
    pub fn cascades(&self, changes: &[Change], q: &str) -> bool {
        changes.iter().any(|(x, w)| x != q && *w != Write::Tracked)
    }

    /// Untrack (or "Finished" for a repeatable): only `q` changes.
    pub fn plan_untrack(&self, p: &Progress, q: &str) -> Vec<Change> {
        if p.tracked.contains(q) {
            vec![(q.to_string(), Write::None)]
        } else {
            vec![]
        }
    }

    /// Reopen: `q` and every descendant that was done or tracked go back to "not started".
    pub fn plan_reopen(&self, p: &Progress, q: &str) -> Vec<Change> {
        let mut v = vec![];
        if p.done.contains(q) || p.tracked.contains(q) {
            v.push((q.to_string(), Write::None));
        }
        for d in self.descendants(q) {
            if p.done.contains(&d) || p.tracked.contains(&d) {
                v.push((d, Write::None));
            }
        }
        v
    }

    /// "I am here": before = done, `q` = in progress, after = not started (descendants reset).
    pub fn plan_here(&self, p: &Progress, q: &str) -> Vec<Change> {
        let mut v = self.plan_track(p, q);
        for d in self.descendants(q) {
            if (p.done.contains(&d) || p.tracked.contains(&d)) && !self.repeatable.contains(&d) {
                v.push((d, Write::None));
            }
        }
        v
    }

    /// First story quest to do for a giver: the first one not done in play order.
    pub fn up_next(&self, p: &Progress, giver: &str) -> Option<Next> {
        let q = self.story(giver).into_iter().find(|q| !p.done.contains(q))?;
        Some(match self.state(p, &q) {
            State::Tracked => Next::Tracked(q),
            State::Available => Next::Available(q),
            State::Locked | State::Done => {
                // The deepest missing prerequisite that belongs to another character (else the nearest missing one).
                let missing: Vec<String> = self.ancestors(&q).into_iter().filter(|a| !p.done.contains(a)).collect();
                let blocker = missing.iter().find(|a| self.giver(a) != giver).or(missing.first()).cloned().unwrap_or_default();
                Next::Waiting(q, blocker)
            }
        })
    }

    /// "Up next" for every giver, in display order (givers with nothing left are omitted).
    pub fn suggestions(&self, p: &Progress) -> Vec<(String, Next)> {
        self.givers.iter().filter_map(|g| self.up_next(p, g).map(|n| (g.clone(), n))).collect()
    }

    /// Position of a quest in its giver's story ("step n of t"), 1-based.
    pub fn step(&self, q: &str) -> Option<(usize, usize)> {
        let s = self.story(self.giver(q));
        s.iter().position(|x| x == q).map(|i| (i + 1, s.len()))
    }
}

/// Breadth-first walk through `edges` from `start` (excluded), each node once.
fn walk(start: &str, edges: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::from([start.to_string()]);
    let mut out = vec![];
    let mut queue = std::collections::VecDeque::from([start.to_string()]);
    while let Some(n) = queue.pop_front() {
        for m in edges.get(&n).into_iter().flatten() {
            if seen.insert(m.clone()) {
                out.push(m.clone());
                queue.push_back(m.clone());
            }
        }
    }
    out
}

/// State of a quest as written (done / tracked / none).
pub fn write_state(p: &Progress, q: &str) -> Write {
    if p.done.contains(q) {
        Write::Done
    } else if p.tracked.contains(q) {
        Write::Tracked
    } else {
        Write::None
    }
}

/// Applies changes to a progress (what the website will hold after the write).
pub fn apply(p: &Progress, changes: &[Change]) -> Progress {
    let mut n = p.clone();
    for (q, w) in changes {
        n.done.remove(q);
        n.tracked.remove(q);
        match w {
            Write::Done => {
                n.done.insert(q.clone());
            }
            Write::Tracked => {
                n.tracked.insert(q.clone());
            }
            Write::None => {}
        }
    }
    n
}

/// The changes that bring the quests touched by `changes` back to their state in `before` (exact undo).
/// (The app keeps these "before" states in its history records; this function states the rule for the tests.)
#[cfg_attr(not(test), allow(dead_code))]
pub fn inverse(before: &Progress, changes: &[Change]) -> Vec<Change> {
    changes.iter().map(|(q, _)| (q.clone(), write_state(before, q))).collect()
}

#[cfg(test)]
mod tests {
    //! Uses the real quest data of the datamine when available (HungerDB export), otherwise a small sample.
    use super::*;

    fn graph() -> Graph {
        let base = "D:/Users/Camille/Downloads/HungerDB/export/quests/";
        if let (Ok(pre), Ok(ch)) = (std::fs::read_to_string(format!("{base}quest_preconditions.json")), std::fs::read_to_string(format!("{base}quest_chains.json"))) {
            let pre: serde_json::Value = serde_json::from_str(&pre).unwrap();
            let ch: serde_json::Value = serde_json::from_str(&ch).unwrap();
            let mut line: HashMap<String, (String, u32)> = HashMap::new();
            for l in ch.as_array().unwrap() {
                for (i, q) in l["order"].as_array().unwrap().iter().enumerate() {
                    line.insert(q.as_str().unwrap().into(), (l["questline_id"].as_str().unwrap().into(), i as u32));
                }
            }
            let mut ids: HashSet<String> = pre.as_object().unwrap().keys().cloned().collect();
            for v in pre.as_object().unwrap().values() {
                ids.extend(v["requires"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()));
            }
            ids.extend(line.keys().cloned());
            let quests = ids.into_iter().map(|id| {
                let req: Vec<String> = pre.get(&id).and_then(|q| q["requires"].as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                let short = id.split('.').nth(2).unwrap_or("");
                let rep = short.starts_with("DQ") || short.starts_with("WQ") || short.starts_with("MQ");
                (id.clone(), id.split('.').nth(1).unwrap_or("").to_string(), line.get(&id).cloned(), req, rep)
            });
            return Graph::new(quests.collect::<Vec<_>>(), &["Eloise".into(), "Louis".into(), "MrSweet".into()]);
        }
        let q = |id: &str, pos: u32, req: &[&str]| {
            let g = id.split('.').nth(1).unwrap().to_string();
            (id.to_string(), g.clone(), Some((g, pos)), req.iter().map(|s| s.to_string()).collect(), false)
        };
        Graph::new(
            vec![
                q("Quest.Eloise.Q01", 0, &[]),
                q("Quest.Eloise.Q04", 1, &["Quest.Eloise.Q01"]),
                q("Quest.Eloise.Q05", 2, &["Quest.Eloise.Q04"]),
                q("Quest.Louis.Q01", 0, &["Quest.Eloise.Q01"]),
                q("Quest.Louis.Q02", 1, &["Quest.Louis.Q01"]),
            ],
            &["Eloise".into(), "Louis".into()],
        )
    }

    /// Cascades both ways, "I am here", exact undo, and "Up next".
    #[test]
    fn cascades_and_undo() {
        let g = graph();
        let empty = Progress::default();
        // Tracking a quest further down finishes every quest before it, across characters; undo restores everything.
        let ch = g.plan_track(&empty, "Quest.Louis.Q02");
        let after = apply(&empty, &ch);
        assert!(after.done.contains("Quest.Louis.Q01") && after.done.contains("Quest.Eloise.Q01"), "{ch:?}");
        assert!(after.tracked.contains("Quest.Louis.Q02"));
        assert_eq!(apply(&after, &inverse(&empty, &ch)), empty);
        assert!(!g.inconsistent(&after, "Quest.Louis.Q02"));
        // Reopening Eloise's first quest reopens everything that depends on it, Louis included.
        let r = g.plan_reopen(&after, "Quest.Eloise.Q01");
        let reset = apply(&after, &r);
        assert!(reset.done.is_empty() && reset.tracked.is_empty(), "{r:?}");
        // "I am here" on Louis Q01 after being on Q02: Q01 tracked, Q02 reset.
        let here = apply(&after, &g.plan_here(&after, "Quest.Louis.Q01"));
        assert!(here.tracked.contains("Quest.Louis.Q01") && !here.tracked.contains("Quest.Louis.Q02") && !here.done.contains("Quest.Louis.Q01"));
        // Up next: Eloise's first quest at the start, Louis waits for it; once done, Louis becomes available.
        let s = g.suggestions(&empty);
        assert!(matches!(s.iter().find(|x| x.0 == "Eloise").map(|x| &x.1), Some(Next::Available(q)) if q == "Quest.Eloise.Q01"), "{s:?}");
        assert!(matches!(s.iter().find(|x| x.0 == "Louis").map(|x| &x.1), Some(Next::Waiting(_, b)) if b == "Quest.Eloise.Q01"), "{s:?}");
        let s2 = g.suggestions(&apply(&empty, &g.plan_done(&empty, "Quest.Eloise.Q01")));
        assert!(matches!(s2.iter().find(|x| x.0 == "Louis").map(|x| &x.1), Some(Next::Available(_))), "{s2:?}");
        // Finishing a tracked quest tracks the story quests it unlocks (Eloise Q01 → Q04, Louis Q01…), never a repeatable.
        let on1 = apply(&empty, &g.plan_track(&empty, "Quest.Eloise.Q01"));
        let fin = g.plan_done(&on1, "Quest.Eloise.Q01");
        assert!(fin.iter().any(|(q, w)| q == "Quest.Eloise.Q04" && *w == Write::Tracked), "{fin:?}");
        assert!(!g.cascades(&fin, "Quest.Eloise.Q01"));
        assert!(fin.iter().all(|(q, _)| !g.is_repeatable(q)), "{fin:?}");
        // Play order is the line order, not the id order (Eloise: Q01, Q04, Q05, Q02…).
        let story = g.story("Eloise");
        assert_eq!(&story[..3], &["Quest.Eloise.Q01", "Quest.Eloise.Q04", "Quest.Eloise.Q05"]);
    }

    /// Repeatable quests are never cascaded (neither up nor down) and never proposed as "next".
    #[test]
    fn repeatables_stay_out() {
        let g = graph();
        let Some(rep) = g.repeatable.iter().next().cloned() else { return };
        let empty = Progress::default();
        let ch = g.plan_track(&empty, &rep);
        assert!(ch.iter().all(|(q, _)| !g.is_repeatable(q) || *q == rep));
        for (_, n) in g.suggestions(&empty) {
            let q = match n {
                Next::Tracked(q) | Next::Available(q) | Next::Waiting(q, _) => q,
            };
            assert!(!g.is_repeatable(&q));
        }
    }
}
