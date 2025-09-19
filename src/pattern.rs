use std::{
    cmp::min,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, atomic::AtomicUsize},
};

use regex::bytes::{Regex, RegexBuilder};

#[derive(Debug)]
pub enum PatternScope {
    Narrow,
    Widen,
    Change,
}

#[derive(Debug)]
enum Ptype {
    Regex(Regex),
}
impl Ptype {
    fn is_match(&self, haystack: &[u8]) -> bool {
        match self {
            Ptype::Regex(regex) => regex.is_match(haystack),
        }
    }

    fn extend(&mut self, p: &str) {
        match self {
            Ptype::Regex(regex) => {
                *regex = make_regex(format!("{regex}{p}").as_str());
            }
        }
    }
}

#[derive(Debug)]
enum AddMode {
    New,
    Ptype,
    StartsWith,
    EndsWith,
}
impl Default for AddMode {
    fn default() -> Self {
        Self::New
    }
}

#[derive(Default)]
struct Matcher {
    patterns: Vec<Ptype>,
    starts_with: Option<Vec<u8>>,
    ends_with: Option<Vec<u8>>,
    mode: AddMode,
    escape: bool,
    text: String,
}
impl Matcher {
    fn add(&mut self, text: &str) -> PatternScope {
        let mut scope = PatternScope::Narrow;
        self.text.push_str(text);

        let mut iter = text.split(' ');
        if !matches!(self.mode, AddMode::New) {
            match iter.next() {
                Some("") => {}
                Some(p) => match self.mode {
                    AddMode::Ptype => {
                        let p = self.relaxed_re(p);
                        self.patterns
                            .last_mut()
                            .expect("Last should exist")
                            .extend(&p)
                    }
                    AddMode::StartsWith => self.extend_starts_with(p),
                    AddMode::EndsWith => {
                        scope = PatternScope::Change;
                        self.extend_ends_with(p);
                    }
                    AddMode::New => unreachable!(),
                },
                None => {
                    return scope;
                }
            }
        }

        for p in iter {
            match p.chars().next() {
                Some('<') => {
                    self.extend_starts_with(&p[1..]);
                    self.mode = AddMode::StartsWith;
                }
                Some('>') => {
                    self.extend_ends_with(&p[1..]);
                    if !matches!(scope, PatternScope::Change) {
                        scope = PatternScope::Change;
                    }
                    self.mode = AddMode::EndsWith;
                }
                Some(_) => {
                    let p = self.relaxed_re(p);
                    self.patterns.push(Ptype::Regex(make_regex(&p)));
                    self.mode = AddMode::Ptype;
                }
                None => {
                    self.mode = AddMode::New;
                }
            }
        }
        scope
    }

    fn rm(&mut self, amount: usize) -> PatternScope {
        let text = std::mem::take(&mut self.text);
        self.reset();
        if amount < text.len() {
            let text = text[..text.len() - amount].to_string();
            self.add(&text);
        }
        PatternScope::Change
    }

    fn set(&mut self, start: usize, text: &str) -> PatternScope {
        let start = min(start, self.text.len());
        if start == self.text.len() {
            return self.add(text);
        }
        let (pfx, sfx) = self.text.split_at(start);
        if let Some(v) = text.strip_prefix(sfx) {
            return self.add(v);
        }

        let text = format!("{pfx}{text}");
        self.reset();
        self.add(&text);
        PatternScope::Change
    }

    fn reset(&mut self) {
        self.text.truncate(0);
        self.patterns.truncate(0);
        self.starts_with = None;
        self.ends_with = None;
        self.mode = AddMode::New;
    }

    fn all_matches(&self, haystack: &[u8]) -> bool {
        self.text.is_empty() || {
            (match &self.starts_with {
                Some(needle) => haystack.starts_with(needle),
                None => true,
            }) && (match &self.ends_with {
                Some(needle) => haystack.ends_with(needle),
                None => true,
            }) && self.patterns.iter().all(|v| v.is_match(haystack))
        }
    }

    fn any_matches(&self, haystack: &[u8]) -> bool {
        !self.text.is_empty() && {
            (match &self.starts_with {
                Some(needle) => haystack.starts_with(needle),
                None => false,
            }) || (match &self.ends_with {
                Some(needle) => haystack.ends_with(needle),
                None => false,
            }) || self.patterns.iter().any(|v| v.is_match(haystack))
        }
    }

    fn extend_starts_with(&mut self, text: &str) {
        let mut current = self.starts_with.take().unwrap_or_default();
        self.unescape_extend(&mut current, text);
        self.starts_with = Some(current);
    }

    fn extend_ends_with(&mut self, text: &str) {
        let mut current = self.ends_with.take().unwrap_or_default();
        self.unescape_extend(&mut current, text);
        self.ends_with = Some(current);
    }

    fn relaxed_re(&mut self, text: &str) -> String {
        let mut esc = self.escape;
        let re = text
            .chars()
            .filter_map(|mut c| {
                if esc || c != '\\' {
                    if esc {
                        esc = false;
                        if c == 's' {
                            c = ' '
                        }
                    }
                    Some(format!(".*{}", regex::escape(&c.to_string())))
                } else {
                    esc = true;
                    None
                }
            })
            .collect();
        self.escape = esc;
        re
    }

    fn unescape_extend(&mut self, text: &mut Vec<u8>, ext: &str) {
        let mut esc = self.escape;
        let mut buf = [0; 4];
        for c in ext.chars() {
            if esc || c != '\\' {
                if esc {
                    esc = false;
                    if c == 's' {
                        text.push(b' ');
                        continue;
                    }
                }
                text.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            } else {
                esc = true;
            }
        }
        self.escape = esc;
    }
}

fn make_regex(text: &str) -> Regex {
    RegexBuilder::new(text)
        .case_insensitive(text == text.to_lowercase())
        .size_limit(50000)
        .unicode(false)
        .swap_greed(true)
        .build()
        .unwrap()
}

#[derive(Default)]
struct PatternInner {
    matcher: RwLock<Matcher>,
    version: AtomicUsize,
}

#[derive(Default, Clone)]
pub struct Pattern {
    inner: Arc<PatternInner>,
}
impl std::fmt::Debug for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let content = self.read_matcher();
        f.debug_struct("Pattern")
            .field("text", &content.text)
            .field("<", &content.starts_with)
            .field(">", &content.ends_with)
            .field("patterns", &content.patterns)
            .finish()
    }
}
impl Pattern {
    pub fn all_matches(&self, line: &[u8]) -> bool {
        self.read_matcher().all_matches(line)
    }

    pub fn any_matches(&self, line: &[u8]) -> bool {
        self.read_matcher().any_matches(line)
    }

    pub fn version(&self) -> usize {
        self.inner
            .version
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn add(&self, text: &str) -> PatternScope {
        self.inc_version();
        self.write_matcher().add(text)
    }

    #[inline(always)]
    pub fn rm(&self, amount: usize) -> PatternScope {
        self.inc_version();
        self.write_matcher().rm(amount)
    }

    #[inline(always)]
    pub fn set(&self, start: usize, text: &str) -> PatternScope {
        self.inc_version();
        self.write_matcher().set(start, text)
    }

    pub fn reset(&self) {
        self.write_matcher().reset();
    }

    pub fn clone_text(&self) -> String {
        self.read_matcher().text.clone()
    }

    #[inline(always)]
    fn write_matcher(&self) -> RwLockWriteGuard<'_, Matcher> {
        self.inner.matcher.write().expect(crate::LOCK_SHOULD_BE_OK)
    }

    #[inline(always)]
    fn read_matcher(&self) -> RwLockReadGuard<'_, Matcher> {
        self.inner.matcher.read().expect(crate::LOCK_SHOULD_BE_OK)
    }

    #[inline(always)]
    fn inc_version(&self) {
        self.inner
            .version
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
#[path = "pattern_test.rs"]
mod test;
