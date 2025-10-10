use std::{
    collections::BTreeSet,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::AtomicUsize,
        mpsc::{SendError, SyncSender},
    },
};

use bytes::Bytes;

use crate::pattern::Pattern;

use super::walker::{Msg, WalkerVersion};

struct Inner {
    pattern: Pattern,
    size: AtomicUsize,
    content: Mutex<BTreeSet<Bytes>>,
    lock: Mutex<()>,
    cvar: Condvar,
    out: SyncSender<Msg>,
}
impl Inner {
    fn size(&self) -> usize {
        self.size.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_size(&self, value: usize) {
        self.size.store(value, std::sync::atomic::Ordering::Relaxed);
        let mut content = self.content();
        while value < content.len() {
            content.pop_last();
        }
    }

    fn add(
        &self,
        value: impl Into<Bytes>,
        pattern_version: usize,
        walker_version: &WalkerVersion,
    ) -> Option<()> {
        let mut content = self.content_add(walker_version)?;

        let value: Bytes = value.into();
        // need to recheck; pattern has changed since our last check
        if (pattern_version == self.pattern.version() || self.pattern.all_matches(value.as_ref()))
            && content.insert(value.clone())
            && self.out.send(Msg::AddFile(value)).is_err()
        {
            None
        } else {
            Some(())
        }
    }

    fn remove(&self, value: impl Into<Bytes>, version: usize) -> Result<(), SendError<Msg>> {
        let mut content = self.content();

        let value = value.into();
        if (version == self.pattern.version() || !self.pattern.all_matches(value.as_ref()))
            && content.remove(value.as_ref())
            && content.len() < self.size()
        {
            self.cvar.notify_all();
        }
        Ok(())
    }

    fn clear(&self) {
        let _ = self.out.send(Msg::Clear);
        let mut content = self.content();
        content.clear();
        self.cvar.notify_all();
    }

    fn redraw(&self) {
        let _ = self.out.send(Msg::Clear);
        let content = self.content();
        for entry in content.iter() {
            let _ = self.out.send(Msg::AddFile(entry.to_owned()));
        }
    }

    fn killed(&self) {
        let _content = self.content();
        self.cvar.notify_all();
    }

    fn remove_unmatched(&self) {
        let mut content = self.content();
        let len = content.len();
        let pattern = self.pattern.clone();

        content.retain(|k| {
            if !pattern.all_matches(k) {
                let _ = self.out.send(Msg::RmFile(k.clone()));
                false
            } else {
                true
            }
        });

        if len > content.len() {
            self.cvar.notify_all();
        }
    }

    #[inline(always)]
    fn content(&self) -> MutexGuard<'_, BTreeSet<Bytes>> {
        self.content.lock().expect(crate::LOCK_SHOULD_BE_OK)
    }

    fn content_add(
        &self,
        walker_version: &WalkerVersion,
    ) -> Option<MutexGuard<'_, BTreeSet<Bytes>>> {
        let mut al = self.lock.lock().expect(crate::LOCK_SHOULD_BE_OK);

        loop {
            {
                let content = self.content();
                if walker_version.is_wrong() {
                    return None;
                }
                if content.len() < self.size() {
                    return Some(content);
                }
            }
            al = self.cvar.wait(al).expect(crate::LOCK_SHOULD_BE_OK);
        }
    }
}

#[derive(Clone)]
pub struct Window {
    inner: Arc<Inner>,
}
impl Window {
    pub fn new(size: usize, out: SyncSender<Msg>) -> Self {
        Self {
            inner: Arc::new(Inner {
                size: size.into(),
                out,
                pattern: Default::default(),
                content: Default::default(),
                cvar: Default::default(),
                lock: Default::default(),
            }),
        }
    }

    #[inline(always)]
    pub fn size(&self) -> usize {
        self.inner.size()
    }

    /// Add `value` to this window.  It is expected `version` is from the `pattern` used to match
    /// `value`.  If the pattern version has changed the test will be redone.
    #[inline(always)]
    pub fn add(
        &self,
        value: impl Into<Bytes>,
        pattern_version: usize,
        walker_version: &WalkerVersion,
    ) -> Option<()> {
        self.inner.add(value, pattern_version, walker_version)
    }

    /// Remove `value` from this window.  It is expected `version` is from the `pattern` used to
    /// test `value` does not match.  If the pattern version has changed the test will be redone.
    #[inline(always)]
    pub fn remove(&self, value: impl Into<Bytes>, version: usize) -> Result<(), SendError<Msg>> {
        self.inner.remove(value, version)
    }

    #[inline(always)]
    pub fn set_size(&self, value: usize) {
        self.inner.set_size(value);
    }

    #[inline(always)]
    pub fn done(&self) {
        let _ = self.inner.out.send(Msg::WalkDone);
    }

    #[inline(always)]
    pub fn started(&self) {
        let _ = self.inner.out.send(Msg::WalkStarted);
    }

    #[inline(always)]
    pub fn clear(&self) {
        self.inner.clear();
    }

    #[inline(always)]
    pub fn redraw(&self) {
        self.inner.redraw();
    }

    #[inline(always)]
    pub fn killed(&self) {
        self.inner.killed();
    }

    #[inline(always)]
    pub fn pattern(&self) -> &Pattern {
        &self.inner.pattern
    }

    #[inline(always)]
    pub fn remove_unmatched(&self) {
        self.inner.remove_unmatched();
    }

    #[inline(always)]
    pub fn message(&self, msg: String) {
        let _ = self.inner.out.send(Msg::Message(msg));
    }

    #[inline(always)]
    pub fn request_resync(&self) {
        let _ = self.inner.out.send(Msg::Resync);
    }
}

#[cfg(test)]
#[path = "window_test.rs"]
mod test;
