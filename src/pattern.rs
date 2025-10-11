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
enum AddMode {
    New,
    Fuzzy,
    Regex,
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
    patterns: Vec<Regex>,
    starts_with: Option<Vec<u8>>,
    ends_with: Option<Vec<u8>>,
    mode: AddMode,
    escape: bool,
    text: String,
    bad_regex: Option<String>,
    skip_prefix: usize,
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
                    AddMode::Fuzzy => self.extend_regex(fuzzy_build(self.escape, p)),
                    AddMode::Regex => self.extend_regex(regex_build(self.escape, p)),
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
            if self.bad_regex.is_some() {
                self.bad_regex.take();
                self.patterns.pop();
            }
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
                Some('*') => {
                    self.add_regex(regex_build(false, &p[1..]));
                    self.mode = AddMode::Regex;
                }
                Some(_) => {
                    self.add_regex(fuzzy_build(false, p));
                    self.mode = AddMode::Fuzzy;
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

    fn skip_prefix(&mut self, n: usize) {
        self.skip_prefix = n;
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
            let haystack = self.adjust_haystack(haystack);
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
            let haystack = self.adjust_haystack(haystack);
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

    fn extend_regex(&mut self, esc_p: (bool, String)) {
        let lre = self.patterns.last_mut().expect("Last should exist");
        let last = match self.bad_regex.take() {
            Some(s) => s,
            None => lre.to_string(),
        };
        self.escape = esc_p.0;
        let restr = format!("{last}{}", &esc_p.1);
        match make_regex(&restr) {
            Ok(regex) => *lre = regex,
            Err(_) => {
                self.bad_regex = Some(restr);
            }
        }
    }

    fn add_regex(&mut self, esc_p: (bool, String)) {
        self.escape = esc_p.0;
        self.patterns.push(match make_regex(&esc_p.1) {
            Ok(regex) => regex,
            Err(_) => {
                self.bad_regex = Some(esc_p.1);
                Regex::new("").expect("Empty regex should be valid")
            }
        });
    }

    fn adjust_haystack<'a>(&self, haystack: &'a [u8]) -> &'a [u8] {
        if self.skip_prefix > 0 {
            &haystack[min(haystack.len(), self.skip_prefix)..]
        } else {
            haystack
        }
    }
}

fn fuzzy_build(mut esc: bool, text: &str) -> (bool, String) {
    let text = text
        .chars()
        .filter_map(|mut c| {
            if esc || c != '\\' {
                if esc {
                    esc = false;
                    if c == 's' {
                        c = ' '
                    }
                }
                if c == '/' {
                    Some("/.*".to_owned())
                } else {
                    Some(format!("{}[^/]*", regex::escape(&c.to_string())))
                }
            } else {
                esc = true;
                None
            }
        })
        .collect();
    (esc, text)
}

fn regex_build(esc: bool, text: &str) -> (bool, String) {
    let lesc = text.ends_with('\\');
    let text = if lesc { &text[0..text.len() - 1] } else { text };
    (
        lesc,
        if esc {
            format!("\\{text}")
        } else {
            text.to_owned()
        },
    )
}

fn make_regex(text: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(text)
        .case_insensitive(text == text.to_lowercase())
        .size_limit(50000)
        .unicode(false)
        .swap_greed(true)
        .build()
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
            .field("skip", &content.skip_prefix)
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

    #[inline(always)]
    pub fn skip_prefix(&self, n: usize) {
        self.write_matcher().skip_prefix(n);
    }

    #[inline(always)]
    pub fn reset(&self) {
        self.write_matcher().reset();
    }

    #[inline(always)]
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
