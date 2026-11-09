use std::{
    collections::HashMap,
    io,
    path::Path,
    sync::mpsc::{self, RecvTimeoutError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::{Result, store::valid_id};

enum Message {
    Event(Event),
    Stop,
}

/// An event-driven, non-recursive theme directory watcher.
/// Dropping it stops the worker and waits for any running callback to finish.
pub struct ThemeWatcher {
    watcher: Option<RecommendedWatcher>,
    sender: mpsc::Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

impl ThemeWatcher {
    /// Watches create/modify/rename events for safe `*.toml` ids.
    /// Calls `on_change` once after 200 ms of quiet per id, on a dedicated thread.
    pub fn watch(
        dir: impl AsRef<Path>,
        on_change: impl Fn(String) + Send + 'static,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let event_sender = sender.clone();
        let mut watcher =
            notify::recommended_watcher(move |result: notify::Result<Event>| match result {
                Ok(event) => {
                    let _ = event_sender.send(Message::Event(event));
                }
                Err(error) => tracing::warn!(%error, "theme watcher error"),
            })
            .map_err(io::Error::other)?;
        watcher
            .watch(dir.as_ref(), RecursiveMode::NonRecursive)
            .map_err(io::Error::other)?;
        let worker = thread::Builder::new()
            .name("theme-watcher".into())
            .spawn(move || {
                let mut pending: HashMap<String, Instant> = HashMap::new();
                loop {
                    let message = match pending.values().min() {
                        Some(deadline) => receiver
                            .recv_timeout(deadline.saturating_duration_since(Instant::now())),
                        None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    match message {
                        Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                        Ok(Message::Event(event))
                            if matches!(
                                event.kind,
                                EventKind::Create(_) | EventKind::Modify(_)
                            ) =>
                        {
                            for path in event.paths {
                                if path.extension().and_then(|s| s.to_str()) == Some("toml")
                                    && let Some(id) = path
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                        .filter(|id| valid_id(id))
                                {
                                    pending.insert(
                                        id.into(),
                                        Instant::now() + Duration::from_millis(200),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                    let now = Instant::now();
                    let ready: Vec<_> = pending
                        .iter()
                        .filter(|(_, deadline)| **deadline <= now)
                        .map(|(id, _)| id.clone())
                        .collect();
                    for id in ready {
                        pending.remove(&id);
                        on_change(id);
                    }
                }
            })?;
        Ok(Self {
            watcher: Some(watcher),
            sender,
            worker: Some(worker),
        })
    }
}

impl Drop for ThemeWatcher {
    fn drop(&mut self) {
        self.watcher.take();
        let _ = self.sender.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
