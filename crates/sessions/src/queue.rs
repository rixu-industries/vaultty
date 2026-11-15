// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Transfer queue of each SFTP tab: the task that copies, the conflict questions and the
//! notifications. What is in the queue and how it changes is in `sftp::queue`.

use std::sync::{Arc, Mutex};

use sftp::queue::{Jobs, Triage, triage};
use sftp::transfer::{CopyError, copy, plan};
use sftp::{Answer, Ask, CANCELLED, Choice, Item, Job, QueueView, Side};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::files::failed;
use crate::{Event, Sessions};

/// The queue of an SFTP tab.
pub struct Queue {
    tab: Uuid,
    jobs: Mutex<Jobs>,
    /// How to answer the open question.
    reply: Mutex<Option<oneshot::Sender<Option<Answer>>>>,
    runner: Mutex<Option<JoinHandle<()>>>,
    /// Conflict questions go one transfer at a time: if two come up at once, the second
    /// waits for the first to be answered (tokio's lock serves in arrival order).
    turn: tokio::sync::Mutex<()>,
}

impl Queue {
    pub(crate) fn new(tab: Uuid) -> Self {
        Queue {
            tab,
            jobs: Mutex::default(),
            reply: Mutex::default(),
            runner: Mutex::default(),
            turn: tokio::sync::Mutex::new(()),
        }
    }

    pub fn view(&self) -> QueueView {
        self.jobs.lock().unwrap().view()
    }

    fn changed(&self, sessions: &Sessions) {
        sessions.emit(Event::Queue(self.tab));
    }

    /// The tab closes: the task is stopped and the open question is answered "cancel".
    pub(crate) fn close(&self) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.closed = true;
        jobs.asking = None;
        self.reply.lock().unwrap().take();

        if let Some(runner) = self.runner.lock().unwrap().take() {
            runner.abort();
        }
    }

    pub(crate) fn fail_waiting(&self, sessions: &Sessions) {
        if self.jobs.lock().unwrap().fail_waiting() {
            self.changed(sessions);
        }
    }

    /// Asks about a file that already exists. The receiver gets the answer (`None` = cancel), or
    /// fails if the tab closes first.
    fn ask(&self, sessions: &Sessions, item: Item) -> oneshot::Receiver<Option<Answer>> {
        let (reply, answer) = oneshot::channel();
        let mut jobs = self.jobs.lock().unwrap();

        if !jobs.closed {
            jobs.asking = Some(Ask {
                id: Uuid::new_v4(),
                item,
            });
            *self.reply.lock().unwrap() = Some(reply);
            drop(jobs);
            self.changed(sessions);
        }

        answer
    }

    /// Splits the plan into what gets copied and what gets skipped, asking about each file that already
    /// exists. Returns `None` if the user cancels or the tab closes.
    async fn decide(
        &self,
        sessions: &Sessions,
        mut items: Vec<Item>,
    ) -> Option<(Vec<Item>, Vec<Item>)> {
        let _turn = self.turn.lock().await;
        let (mut jobs, mut skipped) = (Vec::new(), Vec::new());
        let mut apply_all = None;

        loop {
            let Triage {
                jobs: ready,
                skipped: skip,
                ask,
                rest,
            } = triage(items, apply_all);
            jobs.extend(ready);
            skipped.extend(skip);

            let Some(ask) = ask else {
                return Some((jobs, skipped));
            };

            let answer = self.ask(sessions, ask.clone()).await.ok().flatten()?;

            match answer.choice {
                Choice::Overwrite => jobs.push(ask),
                Choice::Skip => skipped.push(ask),
            }

            apply_all = answer.all.then_some(answer.choice);
            items = rest;
        }
    }

    fn add(
        self: &Arc<Self>,
        sessions: &Arc<Sessions>,
        from: Side,
        jobs: Vec<Item>,
        skipped: Vec<Item>,
    ) {
        let start = self.jobs.lock().unwrap().add(from, jobs, skipped);
        self.changed(sessions);

        if start {
            let task = sessions.runtime.spawn(run(sessions.clone(), self.clone()));
            *self.runner.lock().unwrap() = Some(task);
        }
    }

    fn next(&self, sessions: &Sessions) -> Option<Job> {
        let job = {
            let mut jobs = self.jobs.lock().unwrap();
            let i = jobs.next_job()?;
            jobs.jobs[i].clone()
        };

        self.changed(sessions);
        Some(job)
    }

    fn progress(&self, sessions: &Sessions, id: Uuid, bytes: u64) {
        let mut jobs = self.jobs.lock().unwrap();

        if let Some(i) = jobs.index(id) {
            jobs.jobs[i].bytes = bytes;
            drop(jobs);
            self.changed(sessions);
        }
    }

    /// Records how an item finished and, if it was the last one of its transfer, asks to reload the
    /// destination panel.
    fn finish(&self, sessions: &Sessions, job: &Job, result: Result<(), String>) {
        let Some((_, done)) = self.jobs.lock().unwrap().finish(job.id, result) else {
            return;
        };

        self.changed(sessions);

        if done {
            sessions.emit(Event::Reload {
                tab: self.tab,
                side: job.from.other(),
            });
        }
    }
}

/// Copies the waiting items one at a time until the queue is empty.
async fn run(sessions: Arc<Sessions>, queue: Arc<Queue>) {
    while let Some(job) = queue.next(&sessions) {
        let progress = {
            let (sessions, queue, id) = (sessions.clone(), queue.clone(), job.id);
            move |bytes| queue.progress(&sessions, id, bytes)
        };

        // Only what existed when the plan was made is overwritten, and the user confirmed it;
        // anything that appears later fails.
        let result = copy_job(&sessions, queue.tab, &job, progress).await;
        queue.finish(&sessions, &job, result);
    }
}

async fn copy_job(
    sessions: &Sessions,
    tab: Uuid,
    job: &Job,
    progress: impl FnMut(u64) + Send + 'static,
) -> Result<(), String> {
    let (src, a) = sessions.fs(tab, job.from)?;
    let (dst, b) = sessions.fs(tab, job.from.other())?;
    let conn = a.or(b);
    // There is always a remote side, so there is always a slot.
    let slot = conn.as_ref().map(|(slot, _)| slot.clone());

    let result = copy(&src, &dst, &job.item, job.item.exists, progress, |handle| {
        if let Some(slot) = &slot {
            *slot.transfer.lock().unwrap() = Some(handle);
        }
    })
    .await;

    if let Some(slot) = &slot {
        slot.transfer.lock().unwrap().take();
    }

    match result {
        Ok(()) => Ok(()),
        Err(CopyError::Cancelled) => Err(CANCELLED.into()),
        Err(CopyError::Io(e, path)) => Err(failed(&conn, &e, &path)),
    }
}

impl Sessions {
    /// Copies `paths` from `from` into `to_dir` on the other panel, asking about whatever already exists.
    /// Returns as soon as the plan is ready: the questions and the copy carry on in the background.
    pub async fn enqueue(
        self: Arc<Self>,
        tab: Uuid,
        from: Side,
        paths: Vec<String>,
        to_dir: String,
    ) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }

        let queue = self.queue(tab)?;
        let (src, a) = self.fs(tab, from)?;
        let (dst, b) = self.fs(tab, from.other())?;
        let conn = a.or(b);

        let items = plan(&src, &dst, &paths, &to_dir)
            .await
            .map_err(|e| failed(&conn, &e, &to_dir))?;

        let sessions = self.clone();

        self.runtime.spawn(async move {
            // Cancelling discards the whole transfer; nothing has been copied yet.
            if let Some((jobs, skipped)) = queue.decide(&sessions, items).await {
                queue.add(&sessions, from, jobs, skipped);
            }
        });

        Ok(())
    }

    /// Answers the question `ask`; `None` cancels the whole transfer. An answer to a
    /// question that is no longer open does nothing.
    pub fn answer(&self, tab: Uuid, ask: Uuid, answer: Option<Answer>) {
        let Ok(queue) = self.queue(tab) else {
            return;
        };

        let mut jobs = queue.jobs.lock().unwrap();

        if jobs.asking.as_ref().is_none_or(|a| a.id != ask) {
            return;
        }

        jobs.asking = None;
        drop(jobs);

        if let Some(reply) = queue.reply.lock().unwrap().take() {
            let _ = reply.send(answer);
        }

        queue.changed(self);
    }

    /// Cancels an item, or all of them with `None`: the waiting ones never start, and the one being
    /// copied is aborted (and whatever was half-copied is deleted).
    pub fn cancel(&self, tab: Uuid, job: Option<Uuid>) {
        let Ok(queue) = self.queue(tab) else {
            return;
        };

        let (changed, running) = queue.jobs.lock().unwrap().cancel(job);

        if !changed.is_empty() {
            queue.changed(self);
        }

        if running {
            self.abort_transfer(tab);
        }
    }

    /// Removes what has already finished.
    pub fn clear_finished(&self, tab: Uuid) {
        if let Ok(queue) = self.queue(tab)
            && queue.jobs.lock().unwrap().clear()
        {
            queue.changed(self);
        }
    }
}
