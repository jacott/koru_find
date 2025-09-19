use pretty_assertions::{assert_eq, assert_matches};

use super::*;

#[test]
fn empty_pattern() {
    let pattern = Pattern::default();

    assert!(pattern.all_matches(b"to be or not 2 b"));
    assert!(!pattern.any_matches(b"to be or not 2 b"));
}

#[test]
fn trailing_escape_regex() {
    let pattern = Pattern::default();
    pattern.add("\\");
    pattern.add("s");

    assert!(!pattern.all_matches(b""));
    assert!(!pattern.any_matches(b""));

    assert!(!pattern.all_matches(b"\\s"));
    assert!(!pattern.any_matches(b"\\s"));

    assert!(pattern.all_matches(b" "));
    assert!(pattern.any_matches(b" "));
}

#[test]
fn trailing_escape_and() {
    let pattern = Pattern::default();
    pattern.add("\\");
    pattern.add(" a");

    assert!(pattern.all_matches(b"a"));
    assert!(!pattern.all_matches(b"\\"));
}

#[test]
fn trailing_escape_starts_with() {
    let pattern = Pattern::default();
    pattern.add("<a\\");
    pattern.add("sb");

    assert!(pattern.all_matches(b"a bx"));
    assert!(!pattern.all_matches(b"abx"));
}

#[test]
fn trailing_escape_ends_with() {
    let pattern = Pattern::default();
    pattern.add(">a\\");
    pattern.add("sb");

    assert!(pattern.all_matches(b"xa b"));
    assert!(!pattern.all_matches(b"xab"));
}

#[test]
fn starts_with() {
    let pattern = Pattern::default();
    assert_matches!(pattern.add("<hel"), PatternScope::Narrow);

    assert!(pattern.all_matches(b"hel"));
    assert_matches!(pattern.add("lo"), PatternScope::Narrow);
    assert_eq!(pattern.version(), 2);

    assert!(pattern.all_matches(b"hello world"));
    assert!(!pattern.all_matches(b"hhello"));
    assert!(!pattern.all_matches(b"hel"));

    assert_eq!(pattern.clone_text(), "<hello");
    pattern.rm(2);
    assert_eq!(pattern.clone_text(), "<hel");

    assert!(pattern.all_matches(b"hello world"));
    assert!(pattern.all_matches(b"hel world"));

    pattern.add(r#" <lo\sworld"#);

    assert!(pattern.all_matches(b"hello world"));
    assert!(!pattern.all_matches(b"helloworld"));

    pattern.reset();
    pattern.add(r#"<\s\\"#);
    pattern.add(r#"a\s\\"#);

    assert_eq!(
        String::from_utf8_lossy(&pattern.read_matcher().starts_with.clone().unwrap()),
        r#" \a \"#
    );
}

#[test]
fn starts_with_and() {
    let pattern = Pattern::default();
    pattern.add("<C ");
    pattern.add("<a");
    assert!(pattern.all_matches(b"Cargo.toml"));
}

#[test]
fn ends_with_and() {
    let pattern = Pattern::default();
    pattern.add(">.tom ");
    pattern.add(">");
    assert!(!pattern.all_matches(b"Cargo.toml"));
    pattern.add("l");
    assert!(pattern.all_matches(b"Cargo.toml"));
}

#[test]
fn ends_with() {
    let pattern = Pattern::default();
    assert_matches!(pattern.add(">wor"), PatternScope::Change);

    assert!(pattern.all_matches(b"hewor"));
    assert_matches!(pattern.add("ld"), PatternScope::Change);

    assert!(pattern.all_matches(b"hello world"));
    assert!(!pattern.all_matches(b"hello worldd"));
    assert!(!pattern.all_matches(b"rld"));

    assert_eq!(pattern.clone_text(), ">world");
    pattern.rm(2);
    assert_eq!(pattern.clone_text(), ">wor");

    assert!(pattern.all_matches(b"wor"));
    assert!(pattern.all_matches(b"hello wor"));

    pattern.add(" w");

    assert_matches!(pattern.add(" >"), PatternScope::Change);

    assert_matches!(pattern.add("lds\\s"), PatternScope::Change);
    assert_matches!(pattern.add("end"), PatternScope::Change);

    assert!(!pattern.all_matches(b"hello wor"));

    assert!(pattern.all_matches(b"hello worlds end"));
}

#[test]
fn simple_search() {
    let pattern = Pattern::default();
    assert_matches!(pattern.add("hello"), PatternScope::Narrow);
    assert_eq!(pattern.version(), 1);

    assert!(pattern.all_matches(b"hello world"));
    assert!(pattern.all_matches(b"hfdfeffdlldfdo"));
    assert!(!pattern.all_matches(b"fdfeffdlldfdo"));
    assert!(!pattern.all_matches(b"hel"));

    assert_eq!(pattern.clone_text(), "hello");
    pattern.rm(2);
    assert_eq!(pattern.clone_text(), "hel");

    assert!(pattern.all_matches(b"one hell world"));
}

#[test]
fn rm() {
    let pattern = Pattern::default();
    pattern.add("<");
    pattern.add("h");

    pattern.add(" <x");
    assert!(!pattern.all_matches(b"he"));
    pattern.rm(1);
    assert!(pattern.all_matches(b"he"));
    pattern.rm(2);
    assert!(pattern.all_matches(b"he"));
    pattern.add("x");

    assert!(pattern.all_matches(b"hx"));
    assert!(!pattern.all_matches(b"hhx"));
}

#[test]
fn and_search() {
    let pattern = Pattern::default();
    pattern.add("hell");
    assert_matches!(pattern.add("o world"), PatternScope::Narrow);
    assert!(pattern.all_matches(b" world hello"));
    assert!(pattern.any_matches(b" world hello"));

    assert!(pattern.all_matches(b"hellxo world earth"));
    assert_matches!(pattern.add(" earth"), PatternScope::Narrow);

    assert_eq!(pattern.version(), 3);
    assert!(pattern.all_matches(b"hello world earth"));
    assert!(pattern.any_matches(b"hello world earth"));

    assert!(!pattern.all_matches(b"hello world"));
    assert!(pattern.any_matches(b"hello world"));

    assert!(pattern.all_matches(b"world earth hello"));

    assert!(!pattern.all_matches(b"hello"));
    assert!(pattern.any_matches(b"hello"));
}

#[test]
fn regex_chars() {
    let pattern = Pattern::default();
    pattern.add("he([l])lo");
    assert!(!pattern.all_matches(b"hello"));
    assert!(pattern.all_matches(b"he(((l[l]))))lo"));
}

#[test]
fn convert_to_re() {
    let pattern = Pattern::default();
    assert_eq!(
        &pattern
            .write_matcher()
            .relaxed_re("a\\\\\\c([.*]\\s)")
            .as_str(),
        &".*a.*\\\\.*c.*\\(.*\\[.*\\..*\\*.*\\].* .*\\)"
    );
}
