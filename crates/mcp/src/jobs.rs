// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Commands launched by the MCP. A foreground one is waited on until it finishes; a background one
//! stays here so it can be queried or stopped. In both cases, when it runs past its maximum time or is stopped, the
//! remote process is killed (without a terminal, cutting the connection does not kill it).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sessions::{ConnectError, Output, Sessions};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The ones kept for querying. Beyond this count, the oldest finished ones are forgotten.
const KEEP: usize = 50;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum State {
    Running,
    Done,
    TimedOut,
    Stopped,
    Failed(String),
}

pub(crate) struct Job {
    pub id: Uuid,
    pub vault: Uuid,
    pub host: Uuid,
    pub method: Uuid,
    pub command: String,
    pub timeout: Duration,
    pub out: Mutex<Output>,
    pub state: Mutex<State>,
    /// Child of the server's token: shutting down the server stops it for everyone.
    pub stop: CancellationToken,
}

impl Job {
    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    /// Waits until it stops running, for at most `max`. This way the agent does not have to poll in a loop.
    pub async fn wait(&self, max: Duration) {
        let deadline = tokio::time::Instant::now() + max;

        while self.state() == State::Running && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Runs it to the end: it finishes, times out, is stopped, or the connection fails.
    pub async fn run(&self, sessions: &Sessions) {
        let stop = async {
            tokio::select! {
                () = self.stop.cancelled() => {}
                () = tokio::time::sleep(self.timeout) => {}
            }
        };

        let result = sessions
            .run_command(
                self.vault,
                self.host,
                self.method,
                &self.command,
                &self.out,
                stop,
            )
            .await;

        *self.state.lock().unwrap() = match result {
            Ok(true) => State::Done,
            Ok(false) if self.stop.is_cancelled() => State::Stopped,
            Ok(false) => State::TimedOut,
            Err(e) => State::Failed(explain(e)),
        };
    }
}

/// The error, in English, with what the agent can do about it (usually, asking the user).
fn explain(e: ConnectError) -> String {
    match e {
        ConnectError::Gone => "The host or access method no longer exists, or the vault was locked.".into(),
        ConnectError::KeyGone => {
            "The SSH key of this access method is no longer in the vault. The user has to fix the method in the app."
                .into()
        }
        ConnectError::Ssh(e) => match e {
            ssh::Error::UnknownHostKey(_) => {
                "The server's host key has not been trusted yet. Ask the user to connect to this host once from the Vaultty app to accept it."
                    .into()
            }
            ssh::Error::HostKeyChanged { expected, found } => format!(
                "The server's host key changed (expected {expected}, got {found}). This may be an attack or a reinstall; the connection was refused. Tell the user."
            ),
            ssh::Error::Timeout => "The server did not respond (connection timed out).".into(),
            ssh::Error::AuthFailed => "The server rejected the credentials of this access method.".into(),
            ssh::Error::BadKey(why) => format!("The private key could not be used: {why}"),
            ssh::Error::Ssh(e) => format!("SSH error: {e}"),
            ssh::Error::Sftp(why) => format!("SFTP error: {why}"),
        },
    }
}

pub(crate) struct Jobs {
    list: Mutex<Vec<Arc<Job>>>,
    /// The server's.
    stop: CancellationToken,
}

impl Jobs {
    pub fn new(stop: CancellationToken) -> Self {
        Self {
            list: Mutex::default(),
            stop,
        }
    }

    pub fn new_job(
        &self,
        vault: Uuid,
        host: Uuid,
        method: Uuid,
        command: String,
        timeout: Duration,
    ) -> Arc<Job> {
        Arc::new(Job {
            id: Uuid::new_v4(),
            vault,
            host,
            method,
            command,
            timeout,
            out: Mutex::default(),
            state: Mutex::new(State::Running),
            stop: self.stop.child_token(),
        })
    }

    /// Keeps it for querying later.
    pub fn keep(&self, job: Arc<Job>) {
        let mut list = self.list.lock().unwrap();
        list.push(job);

        // The oldest go first; those still running are never forgotten.
        while list.len() > KEEP {
            match list.iter().position(|j| j.state() != State::Running) {
                Some(i) => {
                    list.remove(i);
                }
                None => break,
            }
        }
    }

    pub fn get(&self, id: Uuid) -> Option<Arc<Job>> {
        self.list
            .lock()
            .unwrap()
            .iter()
            .find(|j| j.id == id)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgets_oldest_finished_and_never_running() {
        let jobs = Jobs::new(CancellationToken::new());
        let job = || {
            jobs.new_job(
                Uuid::nil(),
                Uuid::nil(),
                Uuid::nil(),
                String::new(),
                Duration::ZERO,
            )
        };

        let running = job();
        jobs.keep(running.clone());

        let first_done = job();
        *first_done.state.lock().unwrap() = State::Done;
        jobs.keep(first_done.clone());

        for _ in 0..KEEP - 1 {
            let j = job();
            *j.state.lock().unwrap() = State::Done;
            jobs.keep(j);
        }

        assert!(jobs.get(running.id).is_some());
        assert!(jobs.get(first_done.id).is_none());
        assert_eq!(jobs.list.lock().unwrap().len(), KEEP);
    }

    #[test]
    fn stopping_the_server_stops_its_commands() {
        let server = CancellationToken::new();
        let jobs = Jobs::new(server.clone());
        let job = jobs.new_job(
            Uuid::nil(),
            Uuid::nil(),
            Uuid::nil(),
            String::new(),
            Duration::ZERO,
        );

        server.cancel();
        assert!(job.stop.is_cancelled());
    }
}
