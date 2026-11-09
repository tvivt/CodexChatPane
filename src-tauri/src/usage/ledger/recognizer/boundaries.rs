use super::*;

const DEADLINE_JITTER_MS: i64 = 3 * 60_000;

impl State {
    fn valid_points(samples: &[Sample]) -> BTreeMap<i64, Vec<Point>> {
        let mut replay = State::default();
        let mut points: BTreeMap<i64, BTreeMap<(i64, i64), Point>> = BTreeMap::new();
        for sample in observations(samples) {
            replay.observe(&sample);
            for (&minutes, window) in &replay.windows {
                if window.last.at == sample.at && window.last.id == sample.id {
                    points
                        .entry(minutes)
                        .or_default()
                        .insert((sample.at, sample.id), window.last.clone());
                    if let Some(candidate) = window
                        .candidate
                        .as_ref()
                        .filter(|c| !c.recovery && !c.card && window.last.same_deadline(&c.first))
                    {
                        points.entry(minutes).or_default().insert(
                            (candidate.first.at, candidate.first.id),
                            candidate.first.clone(),
                        );
                    }
                }
            }
        }
        for event in &replay.events {
            for point in [&event.before, &event.after] {
                points
                    .entry(event.minutes)
                    .or_default()
                    .insert((point.at, point.id), point.clone());
            }
        }
        points
            .into_iter()
            .filter_map(|(minutes, rows)| {
                let account = replay.windows.get(&minutes)?.last.account.as_ref()?;
                let rows: Vec<_> = rows
                    .into_values()
                    .filter(|p| p.account.as_ref() == Some(account))
                    .collect();
                (!rows.is_empty()).then_some((minutes, rows))
            })
            .collect()
    }

    fn card_window(
        &self,
        card: &super::super::reset_cards::CardUse,
        points: &BTreeMap<i64, Vec<Point>>,
    ) -> Option<(i64, Option<usize>, Point, Point)> {
        if self.events.iter().any(|event| {
            matches!(event.kind.as_str(), "plan_changed" | "account_changed")
                && event.before.at <= card.at
                && card.at <= event.after.at
        }) {
            return None;
        }
        let matches: Vec<_> = self
            .events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                matches!(event.kind.as_str(), "early" | "natural" | "card")
                    && event.before.account.as_deref() == Some(&card.account)
                    && event.after.account == event.before.account
                    && event.before.at <= card.at
                    && card.at <= event.after.at
            })
            .collect();
        let windows: BTreeSet<_> = matches.iter().map(|(_, event)| event.minutes).collect();
        if windows.len() == 1 {
            let (index, event) = matches[0];
            return Some((
                event.minutes,
                Some(index),
                event.before.clone(),
                event.after.clone(),
            ));
        }
        if !windows.is_empty() {
            return None;
        }
        let candidates: Vec<_> = points
            .iter()
            .filter_map(|(&minutes, rows)| {
                let before = rows
                    .iter()
                    .rev()
                    .find(|p| p.at < card.at && p.account.as_deref() == Some(&card.account))?;
                let after = rows
                    .iter()
                    .find(|p| p.at >= card.at && p.account == before.account)?;
                if before.plan.is_some() && after.plan.is_some() && before.plan != after.plan {
                    return None;
                }
                // A used event has no window field; multiple plausible windows remain unassigned.
                (after.used + 1.0 <= before.used
                    || points.len() == 1
                        && after
                            .deadline
                            .zip(before.deadline)
                            .is_some_and(|(a, b)| (a - b).abs() > DEADLINE_JITTER_MS))
                .then(|| (minutes, None, before.clone(), after.clone()))
            })
            .collect();
        (candidates.len() == 1).then(|| candidates[0].clone())
    }

    fn first_use(
        points: &[Point],
        minutes: i64,
        from: Option<i64>,
        to: Option<i64>,
    ) -> Option<i64> {
        let rows: Vec<_> = points
            .iter()
            .filter(|p| from.is_none_or(|at| p.at >= at) && to.is_none_or(|at| p.at < at))
            .collect();
        let mut stable: BTreeMap<i64, Vec<&Point>> = BTreeMap::new();
        for point in &rows {
            if let Some(deadline) = point.deadline.filter(|at| *at > point.at) {
                stable.entry(deadline).or_default().push(point);
            }
        }
        let starts: Vec<_> = stable
            .into_iter()
            .filter_map(|(deadline, group)| {
                let first = group.first()?;
                let last = group.last()?;
                let start = deadline - minutes * 60_000;
                // A held deadline starts a timer; a deadline rolling with idle samples does not.
                (last.at - first.at >= 60_000
                    && start <= first.at + FIXED_MS
                    && from.is_none_or(|at| start >= at)
                    && to.is_none_or(|at| start < at)
                    && !rows.iter().any(|p| p.at < start && p.used > 0.0))
                .then_some((
                    start,
                    group.iter().map(|p| p.priority()).max().unwrap_or(0),
                    group.len(),
                ))
            })
            .collect();
        let earliest = starts.iter().map(|s| s.0).min()?;
        let latest = starts.iter().map(|s| s.0).max()?;
        (latest - earliest <= DEADLINE_JITTER_MS).then(|| {
            starts
                .into_iter()
                .max_by_key(|s| (s.1, s.2, -s.0))
                .unwrap()
                .0
        })
    }

    fn closing_boundary(
        event: &mut Event,
        points: &[Point],
        previous: Option<i64>,
        next: Option<i64>,
    ) {
        if event.minutes != 10080
            || matches!(
                event.kind.as_str(),
                "card" | "account_changed" | "plan_changed"
            )
        {
            return;
        }
        let next_start = Self::first_use(points, event.minutes, Some(event.before.at + 1), next);
        let confirmed = next_start.map_or(event.after.at, |at| at.min(event.after.at));
        let stable_old = event.before.deadline.is_some_and(|deadline| {
            points.iter().any(|p| {
                p.at < event.before.at
                    && p.account == event.before.account
                    && p.plan == event.before.plan
                    && p.deadline
                        .is_some_and(|at| (at - deadline).abs() <= DEADLINE_JITTER_MS)
            })
        });
        let expected = Self::first_use(points, event.minutes, previous, Some(event.after.at))
            .map(|start| start + event.minutes * 60_000)
            .or_else(|| stable_old.then_some(event.before.deadline).flatten());
        if expected.is_some_and(|at| at - confirmed >= 30 * 60_000) {
            event.kind = "early".into();
            event.adopted = confirmed;
        } else if let Some(expected) = expected.filter(|at| confirmed >= *at) {
            let witnessed = event.before.at <= expected
                && event.after.at >= expected
                && event.after.at - event.before.at <= DEADLINE_JITTER_MS;
            event.kind = if witnessed {
                "natural"
            } else {
                "natural_pending"
            }
            .into();
            event.adopted = expected;
        } else {
            event.kind = "unknown".into();
            event.adopted = confirmed;
        }
    }

    fn period_observations(
        cycle: &mut Cycle,
        points: &[Point],
        opening: Option<&Event>,
        closing: Option<&Event>,
    ) {
        let from = cycle.started_at.unwrap_or(cycle.observed_start);
        let rows: Vec<_> = points
            .iter()
            .filter(|p| {
                p.at >= from
                    && cycle.ended_at.is_none_or(|end| p.at < end)
                    && opening.is_none_or(|e| {
                        !(p.at == e.before.at && p.id == e.before.id && e.adopted == e.before.at)
                    })
            })
            .collect();
        let last = closing
            .map(|e| &e.before)
            .filter(|p| p.at >= from && cycle.ended_at.is_none_or(|end| p.at <= end))
            .or_else(|| rows.last().copied());
        cycle.remaining_known = last.is_some();
        cycle.remaining = last.map_or(0.0, |p| 100.0 - p.used);
        cycle.observed_start = rows
            .first()
            .map_or(cycle.ended_at.map_or(from, |at| at.min(from)), |p| p.at);
        cycle.scheduled_reset = last.and_then(|p| p.deadline);
        cycle.exhausted_at = rows.iter().find(|p| p.used >= 100.0).map(|p| p.at);
        cycle.outcome = if cycle.exhausted_at.is_some() {
            "exhausted"
        } else if cycle.ended_at.is_none() {
            "active"
        } else {
            "unknown"
        }
        .into();
        let baseline = if cycle.started_at.is_some() {
            0.0
        } else {
            rows.first().map_or(0.0, |p| p.used)
        };
        cycle.observed_consumed = rows.iter().map(|p| p.used).fold(baseline, f64::max) - baseline;
        cycle.incomplete |= rows.is_empty() || rows.first().is_some_and(|p| p.at - from > WIDE_MS);
    }

    pub(super) fn project_boundaries(
        &mut self,
        samples: &[Sample],
        cards: &[super::super::reset_cards::CardUse],
    ) -> Vec<(i64, String, String)> {
        let previous = self.projected.take().unwrap_or_default();
        let raw = self.observed_cycles();
        let points = Self::valid_points(samples);
        let mut boundaries = self.events.clone();
        let mut replacements = BTreeSet::new();
        let mut assigned = Vec::new();
        let mut aliases = Vec::new();
        let mut cards: Vec<_> = cards.iter().collect();
        cards.sort_by_key(|c| (c.at, &c.id));
        let conflicts: BTreeSet<_> = cards
            .iter()
            .filter(|card| {
                cards
                    .iter()
                    .any(|other| other.id == card.id && other.at != card.at)
            })
            .map(|card| card.id.clone())
            .collect();
        let mut seen = BTreeSet::new();
        for card in cards {
            if !seen.insert(card.id.clone()) {
                continue;
            }
            if conflicts.contains(&card.id) {
                self.diagnostic("同一兑卡事件的时间证据冲突，未据此切分周期");
                continue;
            }
            let Some((minutes, original, before, after)) = self.card_window(card, &points) else {
                self.diagnostic("兑卡历史缺少唯一的同账号窗口归属，保留原始证据等待核对");
                continue;
            };
            let key = format!("{}:{minutes}", card.id);
            let id = self
                .card_events
                .entry(key)
                .or_insert_with(|| {
                    original
                        .filter(|i| !replacements.contains(i))
                        .map(|i| self.events[i].id.clone())
                        .unwrap_or_else(|| event_id(-card.at, minutes))
                })
                .clone();
            let mut boundary = original
                .map(|i| self.events[i].clone())
                .unwrap_or_else(|| Event {
                    id: id.clone(),
                    minutes,
                    before: before.clone(),
                    after: after.clone(),
                    adopted: card.at,
                    kind: "card".into(),
                    members: BTreeSet::new(),
                    floating: false,
                    historical: true,
                    card_ids: vec![],
                });
            if let Some(index) = original {
                replacements.insert(index);
                if boundary.id != id {
                    aliases.push((minutes, boundary.id.clone(), id.clone()));
                }
            }
            boundary.historical |= boundary.kind == "natural" || boundary.id != id;
            boundary.id = id;
            boundary.kind = "card".into();
            boundary.adopted = card.at;
            boundary.card_ids = vec![format!("server:{}", card.id)];
            self.redemptions.insert(
                format!("server:{}", card.id),
                (card.at, Some("server-history".into())),
            );
            assigned.push(boundary);
        }
        boundaries = boundaries
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| (!replacements.contains(&i)).then_some(e))
            .collect();
        boundaries.extend(assigned);
        boundaries.retain(|e| {
            e.minutes != 10080
                || e.kind == "card"
                || e.after.used + 1.0 <= e.before.used
                || !e
                    .before
                    .deadline
                    .zip(e.after.deadline)
                    .is_some_and(|(a, b)| (a - b).abs() <= DEADLINE_JITTER_MS)
        });
        boundaries.sort_by_key(|e| (e.minutes, e.adopted));
        for index in 0..boundaries.len() {
            let next = boundaries[index + 1..]
                .iter()
                .find(|e| e.minutes == boundaries[index].minutes)
                .map(|e| e.adopted);
            if let Some(rows) = points.get(&boundaries[index].minutes) {
                // Only a corresponding server use retains card classification.
                if boundaries[index].kind == "card"
                    && !boundaries[index]
                        .card_ids
                        .iter()
                        .any(|id| id.starts_with("server:"))
                {
                    boundaries[index].kind = "early".into();
                }
                let previous = boundaries[..index]
                    .iter()
                    .rev()
                    .find(|e| e.minutes == boundaries[index].minutes)
                    .map(|e| e.adopted);
                Self::closing_boundary(&mut boundaries[index], rows, previous, next);
            }
        }
        boundaries.sort_by_key(|e| (e.minutes, e.adopted));
        let mut merged: Vec<Event> = Vec::new();
        for event in boundaries {
            if let Some(last) = merged
                .last_mut()
                .filter(|e| e.minutes == event.minutes && e.adopted == event.adopted)
            {
                if last.id != event.id {
                    aliases.push((event.minutes, event.id.clone(), last.id.clone()));
                }
                if event.kind == "card" {
                    last.kind = event.kind;
                }
                last.card_ids.extend(event.card_ids);
            } else {
                merged.push(event);
            }
        }
        let mut result: Vec<_> = raw
            .iter()
            .filter(|c| !points.contains_key(&c.minutes))
            .cloned()
            .collect();
        for (&minutes, rows) in &points {
            let Some(first) = rows.first() else {
                continue;
            };
            let first_use = |lower: Option<i64>, upper: Option<i64>, id: &str| {
                let inferred = Self::first_use(rows, minutes, lower, upper);
                let known = previous
                    .iter()
                    .find(|c| c.id == id && c.minutes == minutes)
                    .and_then(|c| c.started_at);
                known
                    .filter(|old| {
                        inferred.is_some_and(|at| (at - old).abs() <= DEADLINE_JITTER_MS)
                            && lower.is_none_or(|at| *old >= at)
                            && upper.is_none_or(|at| *old < at)
                    })
                    .or(inferred)
            };
            let mut cycle = initial(first, minutes);
            let mut opening = None;
            for closing in merged.iter().filter(|e| e.minutes == minutes) {
                let original = raw.iter().find(|c| c.id == cycle.id);
                if let Some(old) = original {
                    cycle.incomplete = old.incomplete;
                }
                cycle.ended_at = Some(closing.adopted);
                cycle.boundary_from = Some(closing.before.at);
                cycle.boundary_to = Some(closing.after.at);
                cycle.reset_type = Some(closing.kind.clone());
                cycle.notice_id = Some(closing.id.clone());
                cycle.acknowledged = closing.historical;
                if minutes == 10080 {
                    cycle.started_at = first_use(
                        opening.map(|e: &Event| e.adopted),
                        cycle.ended_at,
                        &cycle.id,
                    );
                }
                Self::period_observations(&mut cycle, rows, opening, Some(closing));
                result.push(cycle);
                cycle = initial(&closing.after, minutes);
                cycle.id = format!("{}-{minutes}", closing.id);
                cycle.started_at = Some(closing.adopted);
                cycle.start_reset_type = Some(closing.kind.clone());
                opening = Some(closing);
            }
            if minutes == 10080 {
                cycle.started_at = first_use(opening.map(|e| e.adopted), None, &cycle.id);
            }
            Self::period_observations(&mut cycle, rows, opening, None);
            result.push(cycle);
        }
        result.sort_by_key(|c| std::cmp::Reverse(c.ended_at.unwrap_or(i64::MAX)));
        self.projected = Some(result);
        aliases
    }
}
