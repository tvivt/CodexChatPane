use super::{Cycle, Sample};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};

// Seconds in server deadlines plus the observed <=3.3s sampling delay, not poll jitter.
const FIXED_MS: i64 = 5_000;
const FLOAT_MS: i64 = 5_000;
const WIDE_MS: i64 = 120_000;
const VERSION: i64 = 2;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    #[default]
    Unknown,
    Fixed,
    Floating,
}

#[derive(Clone, Serialize, Deserialize)]
struct Point {
    id: i64,
    at: i64,
    used: f64,
    deadline: Option<i64>,
    plan: Option<String>,
    source: Option<String>,
    account: Option<String>,
}
impl Point {
    fn priority(&self) -> u8 {
        match self.source.as_deref() {
            Some("online-mcp") => 3,
            Some("cli-fallback") => 2,
            Some("history-log") => 1,
            _ => 0,
        }
    }
    fn same_deadline(&self, other: &Self) -> bool {
        self.deadline
            .zip(other.deadline)
            .is_some_and(|(a, b)| (a - b).abs() <= FIXED_MS)
    }
    fn floating_step(&self, previous: &Self, minutes: i64) -> bool {
        let width = minutes * 60_000;
        self.at - previous.at >= 2 * FLOAT_MS
            && self.deadline.zip(previous.deadline).is_some_and(|(a, b)| {
                (a - self.at - width).abs() <= FLOAT_MS
                    && (b - previous.at - width).abs() <= FLOAT_MS
                    && ((a - b) - (self.at - previous.at)).abs() <= FLOAT_MS
            })
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Candidate {
    before: Point,
    first: Point,
    last: Point,
    reason: String,
    recovery: bool,
    before_mode: Mode,
    card: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Window {
    cycle: Cycle,
    last: Point,
    observed: Point,
    mode: Mode,
    fixed_steps: u32,
    floating_steps: u32,
    candidate: Option<Candidate>,
    seen: Vec<Point>,
    origin: Option<usize>,
}
impl Window {
    fn new(p: Point, minutes: i64) -> Self {
        Self {
            cycle: initial(&p, minutes),
            last: p.clone(),
            observed: p,
            mode: Mode::Unknown,
            fixed_steps: 0,
            floating_steps: 0,
            candidate: None,
            seen: Vec::new(),
            origin: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Event {
    id: String,
    minutes: i64,
    before: Point,
    after: Point,
    adopted: i64,
    kind: String,
    members: BTreeSet<i64>,
    floating: bool,
    historical: bool,
    card_ids: Vec<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct State {
    windows: BTreeMap<i64, Window>,
    completed: Vec<Cycle>,
    events: Vec<Event>,
    cards: HashMap<String, bool>,
    redemptions: BTreeMap<String, (i64, Option<String>)>,
    diagnostics: Vec<String>,
    ambiguous: bool,
}

fn event_id(id: i64, minutes: i64) -> String {
    if minutes == 10080 {
        id.to_string()
    } else {
        format!("{id}-{minutes}")
    }
}

fn initial(point: &Point, minutes: i64) -> Cycle {
    Cycle {
        id: format!("{}-{minutes}", point.id),
        minutes,
        started_at: None,
        observed_start: point.at,
        ended_at: None,
        boundary_from: None,
        boundary_to: None,
        scheduled_reset: point.deadline,
        reset_type: None,
        outcome: if point.used >= 100.0 {
            "exhausted"
        } else {
            "active"
        }
        .into(),
        exhausted_at: (point.used >= 100.0).then_some(point.at),
        remaining: 100.0 - point.used,
        observed_consumed: 0.0,
        incomplete: true,
        acknowledged: false,
        notice_id: None,
    }
}

impl State {
    fn diagnostic(&mut self, message: &str) {
        if !self.diagnostics.iter().any(|old| old == message) {
            self.diagnostics.push(message.into());
        }
    }
    fn reset(
        &mut self,
        w: &mut Window,
        before: &Point,
        first: &Point,
        current: &Point,
        kind: &str,
        natural_at: Option<i64>,
    ) {
        let minutes = w.cycle.minutes;
        let adopted = natural_at.unwrap_or(if matches!(kind, "plan_changed" | "account_changed") {
            first.at
        } else {
            before.at
        });
        let id = event_id(first.id, minutes);
        w.cycle.ended_at = Some(adopted);
        w.cycle.boundary_from = Some(before.at);
        w.cycle.boundary_to = Some(first.at);
        w.cycle.notice_id = Some(id.clone());
        w.cycle.reset_type = Some(kind.into());
        w.cycle.remaining = 100.0 - before.used;
        w.cycle.incomplete |= first.at - before.at > WIDE_MS;
        w.cycle.outcome = if w.cycle.exhausted_at.is_some() {
            "exhausted"
        } else {
            "unknown"
        }
        .into();
        self.completed.push(w.cycle.clone());
        self.events.push(Event {
            id: id.clone(),
            minutes,
            before: before.clone(),
            after: first.clone(),
            adopted,
            kind: kind.into(),
            members: BTreeSet::from([first.id, current.id]),
            floating: false,
            historical: first.id < 0,
            card_ids: if kind == "card" {
                self.redemptions
                    .iter()
                    .filter(|(_, proof)| proof.0 == first.at)
                    .map(|(id, _)| id.clone())
                    .collect()
            } else {
                Vec::new()
            },
        });
        w.origin = Some(self.events.len() - 1);
        w.cycle = initial(first, minutes);
        w.cycle.id = format!("{id}-{minutes}");
        w.cycle.started_at = Some(adopted);
        w.cycle.incomplete =
            first.at - before.at > WIDE_MS || matches!(kind, "plan_changed" | "account_changed");
        w.cycle.observed_consumed = first.used;
        w.last = first.clone();
        w.candidate = None;
        w.seen.clear();
    }
    fn accept(&mut self, w: &mut Window, point: Point) {
        w.cycle.observed_consumed += (point.used - w.last.used).max(0.0);
        w.cycle.remaining = 100.0 - point.used;
        w.cycle.scheduled_reset = point.deadline.or(w.cycle.scheduled_reset);
        if point.used >= 100.0 {
            w.cycle.exhausted_at.get_or_insert(point.at);
            w.cycle.outcome = "exhausted".into();
        }
        if let Some(index) = w.origin {
            let event = &mut self.events[index];
            event.members.insert(point.id);
            event.floating |= w.mode == Mode::Floating;
        }
        w.seen.push(w.last.clone());
        if w.seen.len() > 8 {
            w.seen.remove(0);
        }
        w.last = point;
    }
    fn observe(&mut self, sample: &Sample) {
        if sample.value.account_id.is_none()
            && self.windows.values().any(|w| w.last.account.is_some())
        {
            self.diagnostic("缺少账号归属的历史观察保留在原始缓存，不推进已知账号周期");
            return;
        }
        let mut points = Vec::new();
        for value in [&sample.value.primary, &sample.value.secondary]
            .into_iter()
            .flatten()
        {
            if ![300, 10080].contains(&value.window_minutes) || !value.used_percent.is_finite() {
                continue;
            }
            points.push((
                value.window_minutes,
                Point {
                    id: sample.id,
                    at: sample.at,
                    used: value.used_percent.clamp(0.0, 100.0),
                    deadline: value.resets_at.map(|n| n.saturating_mul(1000)),
                    plan: sample.value.plan_type.clone(),
                    source: sample.value.observation_source.clone(),
                    account: sample.value.account_id.clone(),
                },
            ));
        }
        let mut redeemed = false;
        if let Some(credits) = sample
            .value
            .reset_credits
            .as_ref()
            .and_then(|v| v.credits.as_ref())
        {
            for credit in credits {
                if credit.status == "available" {
                    self.cards.entry(credit.id.clone()).or_insert(false);
                } else if matches!(credit.status.as_str(), "consumed" | "redeemed") {
                    if self.cards.get(&credit.id) == Some(&false) {
                        redeemed = true;
                        self.redemptions.insert(
                            credit.id.clone(),
                            (sample.at, sample.value.observation_source.clone()),
                        );
                    }
                    self.cards.insert(credit.id.clone(), true);
                }
            }
        }
        let affected: Vec<i64> = points
            .iter()
            .filter_map(|(minutes, p)| {
                let w = self.windows.get(minutes)?;
                let updated = p.deadline.zip(w.last.deadline).is_some_and(|(a, b)| a != b);
                (p.used + 1.0 <= w.last.used || updated && w.mode != Mode::Floating)
                    .then_some(*minutes)
            })
            .collect();
        let affected = if affected.is_empty() {
            let updates: Vec<i64> = points
                .iter()
                .filter_map(|(minutes, p)| {
                    let w = self.windows.get(minutes)?;
                    p.deadline
                        .zip(w.last.deadline)
                        .is_some_and(|(a, b)| a != b)
                        .then_some(*minutes)
                })
                .collect();
            if updates.len() == 1 {
                updates
            } else {
                Vec::new()
            }
        } else {
            affected
        };
        if redeemed
            && (affected.is_empty()
                || affected.len() > 1
                    && !points.iter().any(|(minutes, p)| {
                        affected.contains(minutes)
                            && self
                                .windows
                                .get(minutes)
                                .is_some_and(|w| p.used + 1.0 <= w.last.used)
                    }))
        {
            self.diagnostic("兑卡已观察，但缺少可归属的窗口恢复证据");
        }
        for (minutes, mut p) in points {
            let Some(mut w) = self.windows.remove(&minutes) else {
                self.windows.insert(minutes, Window::new(p, minutes));
                continue;
            };
            if p.account != w.last.account {
                self.diagnostic("账号归属变化时重新建立观察基线；未知历史不推造已知账号的重置边界");
                self.windows.insert(minutes, Window::new(p, minutes));
                continue;
            }
            if p.at < w.observed.at {
                self.windows.insert(minutes, w);
                continue;
            }
            if p.plan.is_none() {
                p.plan = w.last.plan.clone();
            }
            let changed_plan = p.plan.is_some() && w.last.plan.is_some() && p.plan != w.last.plan;
            let recovery = p.used + 1.0 <= w.last.used;
            let card = redeemed && affected.contains(&minutes) && (affected.len() == 1 || recovery);
            let stale = !changed_plan
                && !card
                && w.seen.iter().any(|old| {
                    p.same_deadline(old)
                        && (p.used - old.used).abs() < 0.001
                        && p.plan == old.plan
                        && !p.same_deadline(&w.last)
                });
            if stale {
                self.diagnostic("忽略与已见旧窗口一致的交错快照；原始观察保留");
                self.windows.insert(minutes, w);
                continue;
            }
            let fallback_confirmed = p.source.as_deref() == Some("cli-fallback")
                && w.candidate.as_ref().is_some_and(|candidate| {
                    candidate.first.source == p.source
                        && p.at > candidate.first.at
                        && p.same_deadline(&candidate.first)
                });
            let conflict = p.source.is_some()
                && w.last.source.is_some()
                && p.priority() < w.last.priority()
                && !p.same_deadline(&w.last)
                && !fallback_confirmed;
            let account_known = p.account.is_some() && p.account == w.last.account;
            let old_mode = w.mode;
            let before_mode = old_mode;
            if !conflict
                && p.at > w.observed.at
                && p.deadline.is_some()
                && w.observed.deadline.is_some()
            {
                w.fixed_steps = if p.at - w.observed.at >= 1_000 && p.same_deadline(&w.observed) {
                    w.fixed_steps + 1
                } else {
                    0
                };
                w.floating_steps = if p.floating_step(&w.observed, minutes) {
                    w.floating_steps + 1
                } else {
                    0
                };
                if w.fixed_steps >= 1 {
                    w.mode = Mode::Fixed;
                }
                if w.floating_steps >= 2 {
                    w.mode = Mode::Floating;
                }
            }
            let floating = old_mode == Mode::Floating && p.floating_step(&w.observed, minutes)
                || w.floating_steps >= 2;
            let changed_deadline = p
                .deadline
                .zip(w.last.deadline)
                .is_some_and(|(a, b)| (a - b).abs() > FIXED_MS);
            let natural_at = (old_mode == Mode::Fixed
                && !p.same_deadline(&w.observed)
                && p.deadline.is_some_and(|at| at > p.at)
                && changed_deadline
                && !conflict)
                .then_some(w.last.deadline)
                .flatten()
                .filter(|at| *at <= p.at);
            let anchoring = old_mode == Mode::Floating
                && !recovery
                && !conflict
                && p.deadline.zip(w.last.deadline).is_some_and(|(new, old)| {
                    new >= old && new <= p.at + minutes * 60_000 + FLOAT_MS
                });
            if (sample.account_boundary || changed_plan || card || natural_at.is_some())
                && !conflict
                && account_known
            {
                let before = w.last.clone();
                let kind = if sample.account_boundary {
                    "account_changed"
                } else if changed_plan {
                    "plan_changed"
                } else if card {
                    "card"
                } else {
                    "natural"
                };
                self.reset(
                    &mut w,
                    &before,
                    &p,
                    &p,
                    kind,
                    if kind == "natural" { natural_at } else { None },
                );
                w.mode = Mode::Unknown;
                w.fixed_steps = 0;
                w.floating_steps = 0;
                self.accept(&mut w, p.clone());
            } else if anchoring {
                w.candidate = None;
                self.accept(&mut w, p.clone());
            } else if let Some(mut candidate) = w.candidate.take() {
                let returns =
                    p.same_deadline(&candidate.before) && p.used + 1.0 > candidate.before.used;
                let supports = p.same_deadline(&candidate.first)
                    || p.floating_step(&candidate.last, minutes)
                        && p.floating_step(&candidate.first, minutes);
                if returns && !conflict {
                    self.diagnostic("短暂窗口变化已被后续观察撤销");
                    w.mode = old_mode;
                    w.fixed_steps = 0;
                    w.floating_steps = 0;
                    self.accept(&mut w, p.clone());
                } else if (candidate.recovery || candidate.card)
                    && supports
                    && p.at > candidate.first.at
                    && (candidate.card || p.used + 1.0 <= candidate.before.used)
                    && !conflict
                    && account_known
                    && (candidate.card
                        || candidate
                            .first
                            .deadline
                            .is_some_and(|at| at > candidate.first.at)
                            && candidate.before.deadline.is_some_and(|at| {
                                candidate.first.at < at || candidate.before_mode == Mode::Fixed
                            }))
                {
                    let natural_at = (!candidate.card && candidate.before_mode == Mode::Fixed)
                        .then_some(candidate.before.deadline)
                        .flatten()
                        .filter(|at| *at <= candidate.first.at);
                    self.reset(
                        &mut w,
                        &candidate.before,
                        &candidate.first,
                        &p,
                        if candidate.card {
                            "card"
                        } else if natural_at.is_some() {
                            "natural"
                        } else {
                            "early"
                        },
                        natural_at,
                    );
                    self.accept(&mut w, p.clone());
                } else if floating && !candidate.recovery && !conflict {
                    self.accept(&mut w, p.clone());
                } else if supports && !candidate.recovery && !candidate.card && !conflict {
                    // A stable new window may carry consumption without proving a reset.
                    candidate.last = p.clone();
                    self.accept(&mut w, p.clone());
                    w.candidate = Some(candidate);
                } else {
                    if !supports && recovery && changed_deadline && !conflict {
                        candidate = Candidate {
                            before: w.last.clone(),
                            first: p.clone(),
                            last: p.clone(),
                            reason: "recovery-awaiting-confirmation".into(),
                            recovery: true,
                            before_mode,
                            card,
                        };
                    }
                    candidate.last = p.clone();
                    w.candidate = Some(candidate);
                }
            } else if changed_deadline && (!floating || recovery)
                || conflict
                || p.deadline.is_none()
            {
                w.candidate = Some(Candidate {
                    before: w.last.clone(),
                    first: p.clone(),
                    last: p.clone(),
                    reason: if !account_known {
                        "unknown-account"
                    } else if conflict {
                        "source-conflict"
                    } else if p.deadline.is_none() {
                        "missing-deadline"
                    } else if recovery {
                        "recovery-awaiting-confirmation"
                    } else {
                        "deadline-only"
                    }
                    .into(),
                    recovery,
                    before_mode,
                    card,
                });
            } else {
                self.accept(&mut w, p.clone());
            }
            if w.last.at != p.at || w.last.id != p.id {
                w.mode = old_mode;
            }
            w.observed = p;
            self.windows.insert(minutes, w);
        }
    }
    pub(super) fn cycles(&self) -> Vec<Cycle> {
        let mut result = self.completed.clone();
        result.extend(self.windows.values().map(|w| {
            let mut cycle = w.cycle.clone();
            let display = if w.observed.priority() < w.last.priority()
                && !w.observed.same_deadline(&w.last)
            {
                &w.last
            } else {
                &w.observed
            };
            cycle.remaining = 100.0 - display.used;
            cycle.observed_consumed += (display.used - w.last.used).max(0.0);
            cycle.incomplete |= w.candidate.is_some();
            cycle
        }));
        result.sort_by_key(|c| std::cmp::Reverse(c.ended_at.unwrap_or(i64::MAX)));
        result
    }
    fn rename(&mut self, minutes: i64, old: &str, new: &str) {
        for c in self
            .completed
            .iter_mut()
            .chain(self.windows.values_mut().map(|w| &mut w.cycle))
        {
            if c.minutes != minutes {
                continue;
            }
            if c.notice_id.as_deref() == Some(old) {
                c.notice_id = Some(new.into());
            }
            if c.id == format!("{old}-{minutes}") {
                c.id = format!("{new}-{minutes}");
            }
        }
        for event in &mut self.events {
            if event.minutes == minutes && event.id == old {
                event.id = new.into();
            }
        }
    }
    fn reconcile(&mut self, previous: &Self) {
        if self.events.iter().any(|event| {
            previous
                .events
                .iter()
                .filter(|old| {
                    old.minutes == event.minutes
                        && (event.after.id == old.after.id || event.members.contains(&old.after.id))
                })
                .count()
                > 1
        }) {
            *self = previous.clone();
            self.ambiguous = true;
            self.diagnostic("事件锚点关联有歧义，保留此前周期与关闭状态，等待补证据");
            return;
        }
        let matches: Vec<_> = self
            .events
            .iter()
            .enumerate()
            .filter_map(|(i, event)| {
                let old: Vec<_> = previous
                    .events
                    .iter()
                    .filter(|old| {
                        old.minutes == event.minutes
                            && (event.after.id == old.after.id
                                || event.members.contains(&old.after.id))
                    })
                    .collect();
                (old.len() == 1).then(|| (i, old[0].id.clone(), old[0].historical))
            })
            .collect();
        for (index, id, historical) in matches {
            let event = &self.events[index];
            if self
                .events
                .iter()
                .filter(|e| {
                    e.minutes == event.minutes
                        && e.members.contains(
                            &previous
                                .events
                                .iter()
                                .find(|old| old.minutes == event.minutes && old.id == id)
                                .unwrap()
                                .after
                                .id,
                        )
                })
                .count()
                != 1
            {
                continue;
            }
            let (minutes, old) = (event.minutes, event.id.clone());
            self.rename(minutes, &old, &id);
            self.events[index].historical = historical;
        }
    }
    fn preserve_live(&mut self, live: &Self) {
        if live.events.iter().any(|known| {
            !self
                .events
                .iter()
                .any(|event| event.minutes == known.minutes && event.id == known.id)
        }) {
            *self = live.clone();
            self.ambiguous = true;
            self.diagnostic("历史合并未能保留在线重置证据，暂保留在线周期；历史仍待核对");
        }
    }
    pub(super) fn warnings(&self) -> Vec<String> {
        let mut warnings = self.diagnostics.clone();
        for (minutes, w) in &self.windows {
            if let Some(candidate) = &w.candidate {
                warnings.push(format!(
                    "{minutes} 分钟窗口变化待核对（{}），未据此创建重置事件",
                    candidate.reason
                ));
            }
        }
        warnings
    }
}

fn observations(samples: &[Sample]) -> Vec<Sample> {
    let mut points = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        points.push(Sample {
            until: sample.at,
            ..sample.clone()
        });
        if sample.until > sample.at {
            points.push(Sample {
                at: sample.until,
                until: sample.until,
                account_boundary: false,
                ..sample.clone()
            });
        }
    }
    points.sort_by_key(|p| (p.at, std::cmp::Reverse(p.id)));
    points
}
#[cfg(test)]
pub(super) fn derive(samples: &[Sample]) -> State {
    let mut state = State::default();
    for sample in observations(samples) {
        state.observe(&sample);
    }
    state
}
fn fingerprint(samples: &[Sample]) -> Result<String, String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_vec(samples)
        .map_err(|e| e.to_string())?
        .hash(&mut hasher);
    Ok(format!("{:x}", hasher.finish()))
}

pub(super) fn resolve(
    db: &Connection,
    account: &str,
    scope: &str,
    samples: &[Sample],
    live: Option<&State>,
) -> Result<State, String> {
    let points = observations(samples);
    let saved:Option<(i64,usize,String,String)>=db.query_row("SELECT version,inputs,fingerprint,state FROM quota_derivations WHERE account=?1 AND scope=?2",params![account,scope],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(|e|e.to_string())?;
    let previous = saved
        .as_ref()
        .filter(|row| row.0 == VERSION)
        .and_then(|row| serde_json::from_str::<State>(&row.3).ok());
    let resume = previous.as_ref().is_some_and(|state| !state.ambiguous)
        && saved
            .as_ref()
            .filter(|row| row.0 == VERSION && row.1 <= points.len())
            .is_some_and(|row| fingerprint(&points[..row.1]).ok().as_ref() == Some(&row.2));
    let mut state = if resume {
        previous.clone().unwrap_or_default()
    } else {
        State::default()
    };
    let start = if resume && previous.is_some() {
        saved.as_ref().unwrap().1
    } else {
        0
    };
    for point in &points[start..] {
        state.observe(point);
    }
    if let Some(previous) = &previous {
        state.reconcile(previous);
    }
    if let Some(live) = live {
        state.reconcile(live);
        state.preserve_live(live);
    }
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let percentages: HashMap<(i64, i64), f64> = samples
        .iter()
        .flat_map(|s| {
            s.value
                .primary
                .as_ref()
                .into_iter()
                .chain(s.value.secondary.as_ref())
                .map(move |w| ((s.id, w.window_minutes), w.used_percent))
        })
        .collect();
    for event in &state.events {
        tx.execute("UPDATE quota_event_alias SET event_id=old_id WHERE account=?1 AND minutes=?2 AND old_id=?3",params![account,event.minutes,event.id]).map_err(|e|e.to_string())?;
    }
    for event in &state.events {
        if event.floating && event.minutes == 10080 {
            for id in event
                .members
                .iter()
                .filter(|id| **id > 0 && **id != event.after.id)
            {
                // Only old sliding-deadline notices within this proven floating episode are aliases.
                if percentages.get(&(*id, event.minutes)) == Some(&event.after.used) {
                    tx.execute(
                        "INSERT OR IGNORE INTO quota_event_alias VALUES(?1,?2,?3,?4)",
                        params![account, event.minutes, id.to_string(), event.id],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
    }
    tx.execute("INSERT INTO quota_derivations VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(account,scope) DO UPDATE SET version=excluded.version,inputs=excluded.inputs,fingerprint=excluded.fingerprint,state=excluded.state",params![account,scope,VERSION,points.len(),fingerprint(&points)?,serde_json::to_string(&state).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(state)
}

pub(super) fn historical(state: &State, cycle: &Cycle) -> bool {
    state.events.iter().any(|e| {
        e.minutes == cycle.minutes && Some(&e.id) == cycle.notice_id.as_ref() && e.historical
    })
}

#[cfg(test)]
mod tests;
