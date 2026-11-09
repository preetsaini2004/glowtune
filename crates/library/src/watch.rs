use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::Result;

/// Recursive native watcher delivering batches after roughly one second of quiet.
/// Dropping the watcher stops notifications and joins its debounce thread.
#[derive(Debug)]
pub struct LibraryWatcher {
    watcher: Option<RecommendedWatcher>,
    stop: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

enum Message {
    Event(notify::Result<Event>),
    Stop,
}

impl LibraryWatcher {
    /// Watches the supplied folders recursively and returns a channel of changed paths.
    /// Both sides of renames are retained. Runtime backend errors are logged with tracing.
    pub fn start(folders: &[PathBuf]) -> Result<(Self, Receiver<Vec<PathBuf>>)> {
        let (input, events) = mpsc::channel();
        let callback = input.clone();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = callback.send(Message::Event(event));
        })?;
        for folder in folders {
            watcher.watch(folder, RecursiveMode::Recursive)?;
        }
        let (output, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("library-watch".into())
            .spawn(move || debounce(events, output))?;
        Ok((
            Self {
                watcher: Some(watcher),
                stop: input,
                worker: Some(worker),
            },
            receiver,
        ))
    }
}

impl Drop for LibraryWatcher {
    fn drop(&mut self) {
        self.watcher.take();
        let _ = self.stop.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn debounce(events: Receiver<Message>, output: Sender<Vec<PathBuf>>) {
    let quiet = Duration::from_secs(1);
    let mut paths = BTreeSet::new();
    let mut deadline: Option<Instant> = None;
    loop {
        let message = match deadline {
            Some(end) => events.recv_timeout(end.saturating_duration_since(Instant::now())),
            None => events.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match message {
            Ok(Message::Event(Ok(event))) if !matches!(event.kind, EventKind::Access(_)) => {
                paths.extend(event.paths);
                if !paths.is_empty() {
                    deadline = Some(Instant::now() + quiet);
                }
            }
            Ok(Message::Event(Err(error))) => tracing::warn!(%error, "Library watcher error"),
            Ok(Message::Event(Ok(_))) => (),
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if output
                    .send(std::mem::take(&mut paths).into_iter().collect())
                    .is_err()
                {
                    break;
                }
                deadline = None;
            }
        }
    }
}
