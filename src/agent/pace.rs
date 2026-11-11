//! When each tier takes its turn: the placer the moment a thought waits, the steward when the
//! talk pauses or the map strains, and neither while an instruction of the user's is being
//! carried out. Bookkeeping over a clock in seconds and nothing more: whoever owns the
//! workers does the asking, and tells this what came back.
use super::{Tier, at_limit, brief, health};
use crate::model::Session;
use std::collections::BTreeSet;

/// Quiet after the last words before the steward steps back.
pub const PAUSE: f64 = 3.0;
/// Quiet after which even one new thought is worth a look.
pub const LULL: f64 = 10.0;
/// New thoughts that make a pause worth a steward turn.
pub const WORTH: u64 = 3;
/// New thoughts after which the steward stops waiting for a pause.
pub const PRESSURE: u64 = 8;
/// Breath between steward turns once the talk has paused.
pub const REST: f64 = 8.0;
/// Breath between steward turns while the talk runs on: a map that keeps moving cannot be read.
pub const STEADY: f64 = 20.0;
/// Placer turns in a row that file nothing before the steward takes what still waits.
pub const STUCK: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Due {
    Ask(Tier),
    /// Nothing yet: look again at this time.
    At(f64),
    /// Nothing until the talk or the map changes.
    Rest,
}

#[derive(Clone, Debug, Default)]
pub struct Pace {
    started: bool,
    epoch: u64,
    /// Per tier, in its order: when the turn in flight was asked for, the earliest next ask,
    /// and failures in a row.
    asked: [Option<f64>; 3],
    after: [f64; 3],
    failures: [u32; 3],
    /// Placer turns in a row that filed nothing.
    idle: u32,
    /// `next_id` when the talk last grew, and when anything was last heard.
    heard: u64,
    heard_at: f64,
    /// `next_id` at the steward's last look and at the look in flight; when the last one
    /// ended, and the strains it left standing.
    stewarded: u64,
    briefed: u64,
    rested: f64,
    strains: usize,
    /// The placer asked for the steward; `answering` while that turn is in flight.
    called: bool,
    answering: bool,
    /// The epoch the directed turn in flight was asked in.
    directed: u64,
}

fn strains(session: &Session) -> usize {
    health::review(session).iter().filter(|f| f.strain).count()
}

impl Pace {
    /// Stamped on every request.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    /// Whether a turn asked in `epoch` still speaks for the map. One from before the user
    /// took over is dropped rather than rebased over what they did.
    pub fn current(&self, epoch: u64) -> bool {
        epoch == self.epoch
    }
    /// Whether a directed turn asked in `epoch` is the one the user still waits for. What
    /// they did by hand meanwhile does not unask it: its edits go on top of theirs.
    pub fn awaited(&self, epoch: u64) -> bool {
        self.flying(Tier::Direct) && epoch == self.directed
    }
    pub fn flying(&self, tier: Tier) -> bool {
        self.asked[tier as usize].is_some()
    }
    pub fn busy(&self) -> bool {
        self.asked.iter().any(Option::is_some)
    }

    /// The user took the map into their own hands. Turns in flight no longer speak for it,
    /// and the steward leaves their work alone until there is something new to read.
    pub fn take_over(&mut self, session: &Session) {
        self.epoch += 1;
        self.stewarded = session.next_id;
        (self.called, self.answering, self.idle) = (false, false, 0);
    }
    /// Words are arriving, though no thought has landed yet.
    pub fn speaking(&mut self, now: f64) {
        self.heard_at = self.heard_at.max(now);
    }
    /// The placer met something only the steward can fix.
    pub fn call(&mut self) {
        self.called = true;
    }

    /// What to do now, for the map as it stands. `holds` are captures still being split.
    pub fn due(&mut self, session: &Session, holds: &BTreeSet<u64>, now: f64) -> Due {
        if !std::mem::replace(&mut self.started, true) {
            // A map left without a reading gets one look. One already read stays as found.
            let unread = !session.thoughts.is_empty() && brief::narrative(session).is_empty();
            if self.epoch == 0 {
                let look = if unread { WORTH } else { 0 };
                self.stewarded = session.next_id.saturating_sub(look);
            }
            (self.heard, self.rested) = (session.next_id, now - STEADY);
            self.heard_at = self.heard_at.max(now);
        }
        if session.next_id > self.heard {
            (self.heard, self.idle) = (session.next_id, 0);
            self.heard_at = self.heard_at.max(now);
        }
        // A lane that never answers has died. Free its tier, so the next ask finds that out.
        for tier in [Tier::Place, Tier::Steward, Tier::Direct] {
            let limit = 4.0 * tier.patience().as_secs_f64();
            if self.asked[tier as usize].is_some_and(|at| now - at > limit) {
                self.failed(tier, now);
            }
        }
        if at_limit(session) {
            return Due::Rest;
        }
        let ask = |tier: Tier, at: f64| {
            if now >= at {
                Due::Ask(tier)
            } else {
                Due::At(at)
            }
        };
        let [placing, stewarding, directing] = self.asked.map(|at| at.is_some());
        // The map is the user's until what they asked for has landed.
        if directing {
            return Due::Rest;
        }
        let unfiled = !brief::waiting(session, holds).is_empty();
        let stuck = self.idle >= STUCK;
        if unfiled && !placing && !stuck {
            return ask(Tier::Place, self.after[0]);
        }
        // The steward reads a map at rest: nothing being split, nothing on its way to a branch.
        let material = unfiled || session.thoughts.iter().any(|t| t.category.is_some());
        if placing || stewarding || (unfiled && !stuck) || !holds.is_empty() || !material {
            return Due::Rest;
        }
        let fresh = session.next_id.saturating_sub(self.stewarded);
        let quiet = |pause: f64| (self.rested + REST).max(self.heard_at + pause);
        let at = if fresh >= PRESSURE
            || (self.called && fresh >= WORTH)
            || (fresh > 0 && strains(session) > self.strains)
        {
            self.rested + STEADY
        } else if self.called || fresh >= WORTH {
            quiet(PAUSE)
        } else if fresh > 0 {
            quiet(LULL)
        } else {
            return Due::Rest;
        };
        ask(Tier::Steward, at.max(self.after[1]))
    }

    /// `tier` is being asked for a turn on `session`. Stamp the request with the epoch as it
    /// stands after this.
    pub fn sent(&mut self, tier: Tier, session: &Session, now: f64) {
        self.asked[tier as usize] = Some(now);
        match tier {
            Tier::Place => {}
            Tier::Steward => {
                self.briefed = session.next_id;
                self.answering = std::mem::take(&mut self.called);
            }
            // An instruction outranks whatever the agent was in the middle of.
            Tier::Direct => {
                self.take_over(session);
                self.directed = self.epoch;
            }
        }
    }
    /// A turn of this epoch, or one the user directed, came back and its edits were tried on
    /// the map, now `session`.
    /// `filed`: fewer thoughts wait than before, which is all the placer is measured by.
    pub fn landed(&mut self, tier: Tier, session: &Session, filed: bool, now: f64) {
        let index = tier as usize;
        self.asked[index] = None;
        match tier {
            Tier::Place if filed => {
                (self.failures[index], self.after[index], self.idle) = (0, now, 0)
            }
            // Asking again at once would pay for the same answer.
            Tier::Place => {
                self.idle += 1;
                self.called |= self.idle >= STUCK;
                self.back_off(tier, now);
            }
            Tier::Steward => {
                (self.failures[index], self.after[index]) = (0, now);
                (self.stewarded, self.rested, self.answering) = (self.briefed, now, false);
                self.strains = strains(session);
            }
            // What the user asked for stands as they left it, strains and all, until the
            // talk moves on.
            Tier::Direct => {
                self.take_over(session);
                self.strains = strains(session);
            }
        }
    }
    /// A turn failed. Each failure in a row waits longer before the next ask.
    pub fn failed(&mut self, tier: Tier, now: f64) {
        self.asked[tier as usize] = None;
        if tier == Tier::Steward {
            self.called |= std::mem::take(&mut self.answering);
        }
        self.back_off(tier, now);
    }
    /// A turn from before the user took over came back: nobody wants its edits.
    pub fn dropped(&mut self, tier: Tier) {
        self.asked[tier as usize] = None;
    }
    fn back_off(&mut self, tier: Tier, now: f64) {
        let index = tier as usize;
        self.failures[index] = (self.failures[index] + 1).min(8);
        let (first, most) = match tier {
            Tier::Place => (2.0, 30.0),
            Tier::Steward => (REST, 120.0),
            // The user decides when to ask again.
            Tier::Direct => return,
        };
        let wait = first * 2f64.powi(self.failures[index] as i32 - 1);
        self.after[index] = now + wait.min(most);
    }
}
