//! `<input type=file>` support through the desktop's own file chooser (XDG portal, so KDE
//! gets the KDE dialog and GNOME the GNOME one).
//!
//! The portal call blocks until the user picks something, so it runs on a worker thread.
//! The FilePicker itself is a Servo object and stays here on the main thread until the
//! answer comes back.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use servo::{EmbedderControlId, EventLoopWaker, FilePicker};

type Answer = (u64, Option<Vec<PathBuf>>);

pub struct Pickers {
    next: u64,
    pending: HashMap<u64, FilePicker>,
    tx: Sender<Answer>,
    rx: Receiver<Answer>,
}

impl Default for Pickers {
    fn default() -> Self {
        let (tx, rx) = channel();
        Pickers {
            next: 0,
            pending: HashMap::new(),
            tx,
            rx,
        }
    }
}

impl Pickers {
    /// Opens the chooser for `picker`. `origin` goes into the title so it is clear which page
    /// asked for files.
    pub fn show(&mut self, picker: FilePicker, origin: &str, waker: Box<dyn EventLoopWaker>) {
        self.next += 1;
        let key = self.next;
        let extensions: Vec<String> = picker
            .filter_patterns()
            .iter()
            .map(|p| p.0.trim_start_matches('.').to_owned())
            .filter(|e| !e.is_empty() && !e.contains('/'))
            .collect();
        let multiple = picker.allow_select_multiple();
        let title = format!("Choose a file for {origin}");
        self.pending.insert(key, picker);

        let tx = self.tx.clone();
        let spawned = thread::Builder::new()
            .name("drift-file-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title(title);
                if !extensions.is_empty() {
                    dialog = dialog.add_filter("Allowed files", &extensions);
                }
                let picked = if multiple {
                    dialog.pick_files()
                } else {
                    dialog.pick_file().map(|p| vec![p])
                };
                let _ = tx.send((key, picked));
                waker.wake();
            });
        if let Err(err) = spawned {
            // dropping the picker tells the page nothing was chosen
            log::warn!("could not open the file chooser: {err}");
            self.pending.remove(&key);
        }
    }

    /// Hands finished answers back to the pages that asked.
    pub fn poll(&mut self) {
        while let Ok((key, picked)) = self.rx.try_recv() {
            let Some(mut picker) = self.pending.remove(&key) else {
                continue;
            };
            match picked {
                Some(paths) if !paths.is_empty() => {
                    picker.select(&paths);
                    picker.submit();
                }
                _ => picker.dismiss(),
            }
        }
    }

    /// The page went away or cancelled on its own. The chooser stays open on the desktop, but
    /// whatever gets picked there is dropped.
    pub fn hide(&mut self, id: EmbedderControlId) {
        self.pending.retain(|_, p| p.id() != id);
    }
}
