//! Watching files, so a script can be run again when they change.

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher as _};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

/// A running watch. Dropping it stops watching.
pub struct Watcher {
    events: Receiver<Event>,
    _watcher: RecommendedWatcher,
}

impl Watcher {
    /// Block until something changes.
    pub fn next(&self) -> Event {
        self.events
            .recv()
            .expect("the watcher thread stopped while it was still watching")
    }
}

/// Watch these paths, recursively.
pub fn watch<'a>(paths: impl IntoIterator<Item = &'a Path>) -> notify::Result<Watcher> {
    let (tx, events) = channel();

    // notify calls back on its own thread; a channel is all the hand-off that needs.
    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            if let Ok(event) = result {
                if event.kind.is_access() {
                    let _ = tx.send(event);
                }
            }
        },
        Config::default(),
    )?;

    for path in paths {
        watcher.watch(path, RecursiveMode::Recursive)?;
    }

    Ok(Watcher {
        events,
        _watcher: watcher,
    })
}
