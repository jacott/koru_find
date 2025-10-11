use std::{
    env, fs, io,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    sync::{Arc, atomic, mpsc},
    thread,
};

use bytes::Bytes;
use ignore::{ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};

use crate::pattern::{Pattern, PatternScope};

use super::window::Window;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    InvalidCommand,
    ProtocolError,
    Utf8Error,
    IoError(io::ErrorKind),
    Eof,
    InvalidArgument,
    NotADirectory,
    UnknownCommand(String),
    CdInvalid,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl Error {
    pub fn from_io(err: io::Error) -> Self {
        Self::IoError(err.kind())
    }
}

#[derive(Debug, Clone)]
pub struct WalkerVersion {
    current_version: Arc<atomic::AtomicUsize>,
    my_version: usize,
}
impl Default for WalkerVersion {
    fn default() -> Self {
        Self {
            current_version: Arc::new(1.into()),
            my_version: 1,
        }
    }
}
impl WalkerVersion {
    pub fn is_wrong(&self) -> bool {
        self.my_version != self.current_version.load(atomic::Ordering::Relaxed)
    }

    pub fn kill(&self) {
        self.current_version.fetch_add(1, atomic::Ordering::Relaxed);
    }

    pub fn start(&mut self) {
        self.my_version = self.current_version.load(atomic::Ordering::Relaxed);
    }
}

#[derive(Debug, PartialEq)]
pub enum Msg {
    Clear,
    WalkDone,
    AddFile(Bytes),
    RmFile(Bytes),
    WalkStarted,
    Message(String),
    Resync,
}
impl Msg {
    pub(crate) fn write(&self, out: &mut impl io::Write) -> Result<(), io::Error> {
        match self {
            Msg::Clear => out.write_all(b"clear\x00")?,
            Msg::WalkDone => out.write_all(b"done\x00")?,
            Msg::WalkStarted => out.write_all(b"started\x00")?,
            Msg::Resync => out.write_all(b"resync\x00")?,
            Msg::Message(m) => out.write_all(format!("message {m}\x00").as_bytes())?,
            Msg::AddFile(msg) => {
                out.write_all(b"+")?;
                out.write_all(msg)?;
                out.write_all(b"\x00")?
            }
            Msg::RmFile(msg) => {
                out.write_all(b"-")?;
                out.write_all(msg)?;
                out.write_all(b"\x00")?
            }
        }
        Ok(())
    }
}

struct Visitor {
    out: Window,
    pattern: Pattern,
    ignore_pattern: Pattern,
    walker_version: WalkerVersion,
    dir_len: usize,
}
impl ParallelVisitor for Visitor {
    fn visit(&mut self, entry: Result<ignore::DirEntry, ignore::Error>) -> WalkState {
        if self.walker_version.is_wrong() {
            return WalkState::Quit;
        }
        match &entry {
            Ok(entry) => {
                if let Some(ft) = entry.file_type()
                    && ft.is_dir()
                {
                    WalkState::Continue
                } else {
                    let data = &entry.path().as_os_str().as_bytes()[self.dir_len..];
                    if self.ignore_pattern.any_matches(data) {
                        return WalkState::Continue;
                    }
                    let version = self.pattern.version(); // get before test
                    if self.pattern.all_matches(data)
                        && self
                            .out
                            .add(Bytes::copy_from_slice(data), version, &self.walker_version)
                            .is_none()
                    {
                        WalkState::Quit
                    } else {
                        WalkState::Continue
                    }
                }
            }
            Err(_) => WalkState::Continue,
        }
    }
}

#[derive(Clone)]
struct VisitorBuilder {
    out: Window,
    pattern: Pattern,
    ignore_pattern: Pattern,
    walker_version: WalkerVersion,
    dir_len: usize,
}
impl VisitorBuilder {
    fn new(out: Window, pattern: Pattern, ignore_pattern: Pattern, dir_len: usize) -> Self {
        Self {
            out,
            pattern,
            ignore_pattern,
            walker_version: WalkerVersion::default(),
            dir_len,
        }
    }

    fn kill(&self) {
        self.walker_version.kill();
        self.out.killed();
    }
}
impl<'s> ParallelVisitorBuilder<'s> for VisitorBuilder {
    fn build(&mut self) -> Box<dyn ignore::ParallelVisitor + 's> {
        Box::new(Visitor {
            out: self.out.clone(),
            pattern: self.pattern.clone(),
            ignore_pattern: self.ignore_pattern.clone(),
            walker_version: self.walker_version.clone(),
            dir_len: self.dir_len,
        })
    }
}

pub struct Walker {
    pattern: Pattern,
    ignore_pattern: Pattern,
    path: PathBuf,
    visitor: VisitorBuilder,
    walker_thread: Option<thread::JoinHandle<()>>,
    match_thread: Option<thread::JoinHandle<()>>,
    match_sender: Option<mpsc::Sender<Bytes>>,
    is_walking: bool,
}
impl Walker {
    pub fn new(out: Window) -> Self {
        let pattern = out.pattern().clone();
        let ignore_pattern = Pattern::default();
        let visitor = VisitorBuilder::new(out, pattern.clone(), ignore_pattern.clone(), 2);
        Self {
            pattern,
            ignore_pattern,
            path: "./".into(),
            visitor,
            walker_thread: None,
            match_thread: None,
            match_sender: None,
            is_walking: false,
        }
    }

    pub fn command(&mut self, ct: &str, arg: &str) -> Result<(), Error> {
        match ct {
            "walk" => match self.walk(arg) {
                Ok(()) => {
                    self.ensure_running();
                }
                Err(err) => {
                    self.message(format!("walk {arg} failed: {err:?}"));
                }
            },
            "match" => self.match_line(arg),
            "stop" => {
                self.is_walking = false;
                self.kill_running();
                self.visitor.out.clear();
                self.pattern.reset();
                self.pattern.skip_prefix(0);
                self.ignore_pattern.reset();
                self.ignore_pattern.skip_prefix(0);
            }
            "add" => self.change_pattern(self.pattern.add(arg)),
            "ignore" => {
                self.ignore_pattern.set(0, arg);
                self.kill_running();
                self.visitor.out.clear();
            }
            "skip-prefix" => {
                let n = arg.parse().map_err(|_| Error::InvalidArgument)?;
                self.ignore_pattern.skip_prefix(n);
                self.pattern.skip_prefix(n);
                self.change_pattern(PatternScope::Change);
            }
            "rm" => self.change_pattern(
                self.pattern
                    .rm(arg.parse().map_err(|_| Error::InvalidArgument)?),
            ),
            "set" => {
                let (start, text) = super::chars_split_at_space(arg);
                self.change_pattern(
                    self.pattern
                        .set(start.parse().map_err(|_| Error::InvalidArgument)?, text),
                );
            }
            "redraw" => {
                self.visitor.out.redraw();
            }
            "window_size" => {
                self.visitor
                    .out
                    .set_size(arg.parse().map_err(|_| Error::InvalidArgument)?);
            }
            _ => {
                return Err(Error::UnknownCommand(ct.to_string()));
            }
        }
        Ok(())
    }

    #[inline(always)]
    pub fn message(&self, value: String) {
        self.visitor.out.message(value);
    }

    fn change_pattern(&mut self, scope: PatternScope) {
        if matches!(scope, PatternScope::Narrow) {
            self.visitor.out.remove_unmatched();
        } else if self.is_walking {
            self.kill_running();
            self.visitor.out.remove_unmatched();
            self.ensure_running();
        } else {
            self.kill_match_thread();
            self.visitor.out.request_resync();
        }
    }

    fn walk(&mut self, dir: &str) -> Result<(), Error> {
        self.path = dir.into();
        if self.path.starts_with("~/") {
            let rest = &self
                .path
                .strip_prefix("~/")
                .map_err(|_| Error::InvalidArgument)?
                .to_str()
                .ok_or(Error::Utf8Error)?;
            self.path = fs::canonicalize(format!(
                "{}/{rest}/",
                env::var("HOME").map_err(|_| Error::CdInvalid)?
            ))
            .map_err(Error::from_io)?;
        }
        if !self.path.is_dir() {
            return Err(Error::NotADirectory);
        }
        self.path.push("");
        self.kill_running();
        self.kill_match_thread();
        self.visitor.dir_len = self.path.as_os_str().len();
        self.is_walking = true;
        Ok(())
    }

    fn kill_running(&mut self) {
        self.visitor.kill();
        let Some(t) = self.walker_thread.take() else {
            return;
        };
        let _ = t.join();
    }

    fn kill_match_thread(&mut self) {
        let Some(t) = self.match_thread.take() else {
            return;
        };
        self.match_sender = None;
        let _ = t.join();
    }

    fn ensure_running(&mut self) {
        if self.walker_thread.is_none() {
            self.visitor.out.started();
            let walker = WalkBuilder::new(&self.path).build_parallel();
            self.visitor.walker_version.start();
            let mut builder = self.visitor.clone();
            self.walker_thread = Some(thread::spawn(move || {
                walker.visit(&mut builder);
                builder.out.done();
            }));
        }
    }

    fn match_line(&mut self, arg: &str) {
        if self.match_thread.is_none() {
            let (tx, rx) = mpsc::channel();
            self.match_sender = Some(tx);
            self.visitor.walker_version.start();
            let walker_version = self.visitor.walker_version.clone();
            let builder = self.visitor.clone();
            self.match_thread = Some(thread::spawn(move || {
                for msg in rx.iter() {
                    builder.out.add(msg, 0, &walker_version);
                }
            }))
        }

        if !self.ignore_pattern.any_matches(arg.as_bytes())
            && let Some(tx) = &self.match_sender
            && tx.send(Bytes::copy_from_slice(arg.as_bytes())).is_err()
        {
            self.kill_match_thread();
        };
    }
}

#[cfg(test)]
#[path = "walker_test.rs"]
mod test;
