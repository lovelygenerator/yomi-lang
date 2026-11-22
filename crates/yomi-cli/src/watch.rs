//! The input of `yomi run --watch`: standard input lines and file changes in one stream.
//!
//! A change of a script or the text table arrives as the line `:reload`, so the session handles it like a typed command,
//! even while it is waiting for input.

use std::collections::VecDeque;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Changes that follow each other within this time make one reload.
const QUIET: Duration = Duration::from_millis(200);

enum Msg {
    Line(Vec<u8>),
    Eof,
    Changed,
}

pub(crate) struct WatchInput {
    rx: Receiver<Msg>,
    /// Messages that arrived while waiting for the changes to settle.
    pending: VecDeque<Msg>,
    line: Vec<u8>,
    pos: usize,
    eof: bool,
    /// Kept so that the watching goes on.
    _watcher: RecommendedWatcher,
}

impl WatchInput {
    /// Starts reading standard input and watching the search directory (and the text table). Fails with the reason.
    pub(crate) fn start(
        entry: &Path,
        root: Option<&Path>,
        texts: Option<&Path>,
    ) -> Result<Self, String> {
        let root: PathBuf = match root {
            Some(r) => r.to_path_buf(),
            None => match entry.parent() {
                Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
                _ => PathBuf::from("."),
            },
        };
        let texts_file =
            texts.map(|t| std::fs::canonicalize(t).unwrap_or_else(|_| t.to_path_buf()));
        let (tx, rx) = mpsc::channel();
        let sender = tx.clone();
        let watched = texts_file.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            // Reading a file is also reported (as an access), so only changes count.
            if !matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                return;
            }
            let relevant = event.paths.iter().any(|p| {
                p.extension().is_some_and(|e| e == "yomi")
                    || watched.as_deref() == Some(p.as_path())
            });
            if relevant {
                let _ = sender.send(Msg::Changed);
            }
        })
        .map_err(|e| e.to_string())?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| format!("{}: {e}", root.display()))?;
        if let Some(dir) = texts_file.as_deref().and_then(Path::parent) {
            watcher
                .watch(dir, RecursiveMode::NonRecursive)
                .map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::thread::spawn(move || read_stdin(tx));
        Ok(Self {
            rx,
            pending: VecDeque::new(),
            line: Vec::new(),
            pos: 0,
            eof: false,
            _watcher: watcher,
        })
    }

    /// Takes the next message, waiting for it.
    fn next(&mut self) -> Msg {
        self.pending
            .pop_front()
            .or_else(|| self.rx.recv().ok())
            .unwrap_or(Msg::Eof)
    }

    /// After a change, waits until no more changes arrive; other messages are kept in order.
    fn settle(&mut self) {
        loop {
            match self.rx.recv_timeout(QUIET) {
                Ok(Msg::Changed) => {}
                Ok(other) => self.pending.push_back(other),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

fn read_stdin(tx: Sender<Msg>) {
    let mut stdin = io::stdin().lock();
    loop {
        let mut line = Vec::new();
        match stdin.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => {
                let _ = tx.send(Msg::Eof);
                return;
            }
            Ok(_) => {
                if tx.send(Msg::Line(line)).is_err() {
                    return;
                }
            }
        }
    }
}

impl Read for WatchInput {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for WatchInput {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.pos >= self.line.len() && !self.eof {
            self.pos = 0;
            self.line = match self.next() {
                Msg::Line(l) => l,
                Msg::Eof => {
                    self.eof = true;
                    Vec::new()
                }
                Msg::Changed => {
                    self.settle();
                    b":reload\n".to_vec()
                }
            };
        }
        Ok(&self.line[self.pos.min(self.line.len())..])
    }

    fn consume(&mut self, amt: usize) {
        self.pos += amt;
    }
}
