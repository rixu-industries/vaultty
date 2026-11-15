// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Transfer queue of an SFTP tab, without tasks or connections: what is waiting, what is being
//! copied, how each item ended and what is being asked. It runs in series: one item at a
//! time. Whoever uses it (`sessions`) starts the copying task and notifies the UI of each change.

use std::collections::HashSet;

use uuid::Uuid;

use crate::files::Side;
use crate::transfer::{CANCELLED, Item};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobState {
    Waiting,
    Running,
    Done,
    Failed,
    Cancelled,
    Skipped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: Uuid,
    /// Items of the same transfer: when the group finishes, the destination pane is reloaded.
    pub group: Uuid,
    pub from: Side,
    pub item: Item,
    pub state: JobState,
    pub bytes: u64,
    pub error: Option<String>,
}

impl Job {
    /// Still in progress: waiting or being copied.
    pub fn live(&self) -> bool {
        matches!(self.state, JobState::Waiting | JobState::Running)
    }
}

/// What to do with a file that already exists at the destination.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Overwrite,
    Skip,
}

/// Answer to a conflict question. `all`: apply it to the rest of the same transfer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Answer {
    pub choice: Choice,
    pub all: bool,
}

/// File that already exists at the destination, waiting for the user to decide.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub id: Uuid,
    pub item: Item,
}

/// State of the queue, for rendering it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueueView {
    pub jobs: Vec<Job>,
    pub asking: Option<Ask>,
}

impl QueueView {
    /// Fraction copied by bytes of the transfers still in progress, not counting what was skipped,
    /// cancelled or failed. Empty files count as one unit.
    pub fn progress(&self) -> f32 {
        let live: HashSet<Uuid> = self
            .jobs
            .iter()
            .filter(|j| j.live())
            .map(|j| j.group)
            .collect();

        let (done, total) = self
            .jobs
            .iter()
            .filter(|j| live.contains(&j.group) && !j.item.dir)
            .fold((0u128, 0u128), |(done, total), job| {
                let size = job.item.size.max(1);
                let bytes = match job.state {
                    JobState::Done => size,
                    JobState::Running => job.bytes.min(job.item.size),
                    JobState::Waiting => 0,
                    JobState::Failed | JobState::Cancelled | JobState::Skipped => {
                        return (done, total);
                    }
                };

                (done + u128::from(bytes), total + u128::from(size))
            });

        if total == 0 {
            0.
        } else {
            done as f32 / total as f32
        }
    }
}

#[derive(Default, Debug)]
pub struct Jobs {
    pub jobs: Vec<Job>,
    pub asking: Option<Ask>,
    /// A task is copying the waiting items.
    pub running: bool,
    /// The tab was closed: nothing else is copied or asked.
    pub closed: bool,
}

impl Jobs {
    pub fn view(&self) -> QueueView {
        QueueView {
            jobs: self.jobs.clone(),
            asking: self.asking.clone(),
        }
    }

    pub fn index(&self, id: Uuid) -> Option<usize> {
        self.jobs.iter().position(|j| j.id == id)
    }

    /// Adds an already decided transfer. Returns whether the copying task must be started.
    pub fn add(&mut self, from: Side, jobs: Vec<Item>, skipped: Vec<Item>) -> bool {
        let group = Uuid::new_v4();

        let job = |item: Item, state, error| Job {
            id: Uuid::new_v4(),
            group,
            from,
            item,
            state,
            bytes: 0,
            error,
        };

        let skipped = skipped.into_iter().map(|item| {
            let error = item
                .skipped
                .clone()
                .unwrap_or("Already exists, skipped".into());
            job(item, JobState::Skipped, Some(error))
        });

        let waiting = jobs
            .into_iter()
            .map(|item| job(item, JobState::Waiting, None));

        self.jobs.extend(skipped.chain(waiting).collect::<Vec<_>>());

        let start = !self.running && !self.closed;
        self.running |= start;
        start
    }

    /// Moves the first waiting item to "copying". If none is left, the task ends.
    pub fn next_job(&mut self) -> Option<usize> {
        let next = self
            .jobs
            .iter()
            .position(|j| j.state == JobState::Waiting)
            .filter(|_| !self.closed);

        match next {
            Some(i) => self.jobs[i].state = JobState::Running,
            None => self.running = false,
        }

        next
    }

    /// Records how an item ended. Returns its position and whether its transfer ended with it.
    pub fn finish(&mut self, id: Uuid, result: Result<(), String>) -> Option<(usize, bool)> {
        let i = self.index(id)?;
        let job = &mut self.jobs[i];

        match result {
            Ok(()) => {
                job.state = JobState::Done;
                job.bytes = job.item.size;
            }
            Err(e) if e == CANCELLED => job.state = JobState::Cancelled,
            Err(e) => {
                job.state = JobState::Failed;
                job.error = Some(e);
            }
        }

        let group = job.group;
        let done = !self
            .jobs
            .iter()
            .any(|j| j.group == group && j.state == JobState::Waiting);

        Some((i, done))
    }

    /// Cancels one item, or all with `None`: the waiting ones no longer start. Returns the
    /// ones that changed and whether the one being copied is among them (it must be stopped).
    pub fn cancel(&mut self, id: Option<Uuid>) -> (Vec<usize>, bool) {
        let mut changed = Vec::new();
        let mut running = false;

        for (i, job) in self.jobs.iter_mut().enumerate() {
            if id.is_some_and(|id| id != job.id) {
                continue;
            }

            match job.state {
                JobState::Waiting => {
                    job.state = JobState::Cancelled;
                    changed.push(i);
                }
                JobState::Running => running = true,
                _ => {}
            }
        }

        (changed, running)
    }

    /// The connection was lost: what was waiting will no longer be copied.
    pub fn fail_waiting(&mut self) -> bool {
        let mut changed = false;

        for job in &mut self.jobs {
            if job.state == JobState::Waiting {
                job.state = JobState::Failed;
                job.error = Some("Connection lost".into());
                changed = true;
            }
        }

        changed
    }

    /// Removes what already finished (succeeded, failed, cancelled or skipped).
    pub fn clear(&mut self) -> bool {
        let before = self.jobs.len();
        self.jobs.retain(Job::live);

        self.jobs.len() != before
    }
}

/// Split of a plan: what is copied, what is skipped and, if needed, the file to ask about (`ask`),
/// with what remains undecided after it (`rest`).
#[derive(Default, Debug)]
pub struct Triage {
    pub jobs: Vec<Item>,
    pub skipped: Vec<Item>,
    pub ask: Option<Item>,
    pub rest: Vec<Item>,
}

/// Stops at the first file that already exists if there is no decision for all (`apply_all`).
pub fn triage(items: Vec<Item>, apply_all: Option<Choice>) -> Triage {
    let mut t = Triage::default();
    let mut items = items.into_iter();

    while let Some(item) = items.next() {
        if item.skipped.is_some() {
            t.skipped.push(item);
        } else if item.dir || !item.exists || apply_all == Some(Choice::Overwrite) {
            t.jobs.push(item);
        } else if apply_all == Some(Choice::Skip) {
            t.skipped.push(item);
        } else {
            t.ask = Some(item);
            t.rest = items.collect();
            break;
        }
    }

    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(dst: &str) -> Item {
        Item {
            src: format!("/src/{dst}"),
            dst: dst.into(),
            dir: false,
            size: 1,
            exists: false,
            skipped: None,
        }
    }

    fn existing(dst: &str) -> Item {
        Item {
            exists: true,
            ..item(dst)
        }
    }

    fn dsts(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.dst.as_str()).collect()
    }

    #[test]
    fn without_existing_files_everything_is_copied() {
        let dir = Item {
            dir: true,
            ..item("d")
        };
        let t = triage(vec![dir, item("d/a"), item("d/b")], None);

        assert_eq!(dsts(&t.jobs), ["d", "d/a", "d/b"]);
        assert!(t.skipped.is_empty() && t.ask.is_none() && t.rest.is_empty());
    }

    #[test]
    fn without_decision_asks_about_the_first_existing() {
        let t = triage(
            vec![item("a"), existing("b"), existing("c"), item("d")],
            None,
        );

        assert_eq!(dsts(&t.jobs), ["a"]);
        assert_eq!(t.ask.unwrap().dst, "b");
        assert_eq!(dsts(&t.rest), ["c", "d"]);
    }

    #[test]
    fn apply_all_overwrites_or_skips_without_asking() {
        let items = || vec![existing("a"), item("b"), existing("c")];

        let over = triage(items(), Some(Choice::Overwrite));
        assert_eq!(dsts(&over.jobs), ["a", "b", "c"]);
        assert!(over.ask.is_none());

        let skip = triage(items(), Some(Choice::Skip));
        assert_eq!(dsts(&skip.jobs), ["b"]);
        assert_eq!(dsts(&skip.skipped), ["a", "c"]);
    }

    #[test]
    fn what_the_plan_omits_is_skipped_and_folders_are_created() {
        let link = Item {
            skipped: Some("Folder link skipped".into()),
            ..item("l")
        };
        let dir = Item {
            dir: true,
            ..existing("d")
        };
        let t = triage(vec![link, dir], None);

        assert_eq!(dsts(&t.skipped), ["l"]);
        assert_eq!(dsts(&t.jobs), ["d"]);
        assert!(t.ask.is_none());
    }

    fn states(jobs: &Jobs) -> Vec<JobState> {
        jobs.jobs.iter().map(|j| j.state).collect()
    }

    #[test]
    fn progress_by_bytes_without_counting_folders_skipped_or_cancelled() {
        let mut jobs = Jobs::default();

        assert_eq!(jobs.view().progress(), 0.);

        let sized = |dst, size| Item { size, ..item(dst) };
        jobs.add(
            Side::Local,
            vec![
                sized("a", 100),
                sized("b", 300),
                sized("c", 100),
                Item {
                    dir: true,
                    ..item("dir")
                },
            ],
            vec![item("skipped")],
        );

        let a = jobs.next_job().unwrap();
        jobs.finish(jobs.jobs[a].id, Ok(()));
        let b = jobs.next_job().unwrap();
        jobs.jobs[b].bytes = 150;

        assert_eq!(jobs.view().progress(), 0.5);

        jobs.jobs[b].bytes = 500;

        assert_eq!(jobs.view().progress(), 0.8);

        // Cancelling one removes it from the total: what remains can still reach 100 %.
        jobs.finish(jobs.jobs[b].id, Err(CANCELLED.into()));

        assert_eq!(jobs.view().progress(), 0.5);
    }

    #[test]
    fn progress_ignores_finished_transfers() {
        let mut jobs = Jobs::default();
        jobs.add(Side::Local, vec![item("a")], vec![]);
        let a = jobs.next_job().unwrap();
        jobs.finish(jobs.jobs[a].id, Ok(()));

        assert_eq!(jobs.view().progress(), 0.);

        jobs.add(Side::Local, vec![item("b")], vec![]);

        assert_eq!(jobs.view().progress(), 0.);
    }

    #[test]
    fn progress_of_empty_files() {
        let mut jobs = Jobs::default();
        jobs.add(
            Side::Local,
            vec![
                Item {
                    size: 0,
                    ..item("empty")
                },
                item("b"),
            ],
            vec![],
        );
        let i = jobs.next_job().unwrap();

        assert_eq!(jobs.view().progress(), 0.);

        jobs.finish(jobs.jobs[i].id, Ok(()));

        assert_eq!(jobs.view().progress(), 0.5);
    }

    #[test]
    fn the_queue_copies_in_order_and_notifies_when_the_transfer_ends() {
        let mut jobs = Jobs::default();

        assert!(jobs.add(Side::Local, vec![item("a"), item("b")], vec![existing("c")]));
        // A task is already copying: adding more does not start another.
        assert!(!jobs.add(Side::Local, vec![item("x")], vec![]));

        assert_eq!(
            jobs.jobs[0].error.as_deref(),
            Some("Already exists, skipped")
        );

        let a = jobs.next_job().unwrap();
        assert_eq!(jobs.jobs[a].item.dst, "a");
        assert_eq!(jobs.finish(jobs.jobs[a].id, Ok(())), Some((a, false)));

        let b = jobs.next_job().unwrap();
        assert_eq!(
            jobs.finish(jobs.jobs[b].id, Err("no space".into())),
            Some((b, true))
        );

        let x = jobs.next_job().unwrap();
        assert_eq!(
            jobs.finish(jobs.jobs[x].id, Err(CANCELLED.into())),
            Some((x, true))
        );

        assert_eq!(jobs.next_job(), None);
        assert!(!jobs.running);

        use JobState::*;
        assert_eq!(states(&jobs), [Skipped, Done, Failed, Cancelled]);
        assert_eq!(jobs.jobs[2].error.as_deref(), Some("no space"));
        assert_eq!(jobs.jobs[1].bytes, 1);
    }

    #[test]
    fn cancel_and_lose_connection() {
        let mut jobs = Jobs::default();
        jobs.add(Side::Remote, vec![item("a"), item("b"), item("c")], vec![]);
        jobs.next_job();

        let b = jobs.jobs[1].id;
        assert_eq!(jobs.cancel(Some(b)), (vec![1], false));
        assert_eq!(jobs.cancel(None), (vec![2], true));

        jobs.add(Side::Remote, vec![item("d")], vec![]);
        assert!(jobs.fail_waiting());
        assert!(!jobs.fail_waiting());

        use JobState::*;
        assert_eq!(states(&jobs), [Running, Cancelled, Cancelled, Failed]);

        assert!(jobs.clear());
        assert_eq!(states(&jobs), [Running]);
    }

    #[test]
    fn closed_copies_nothing_more() {
        let mut jobs = Jobs {
            closed: true,
            ..Jobs::default()
        };

        assert!(!jobs.add(Side::Local, vec![item("a")], vec![]));
        assert_eq!(jobs.next_job(), None);
    }
}
