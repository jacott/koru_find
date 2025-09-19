use std::{sync::mpsc, thread, time::Duration};

use super::*;

fn content_to_string(w: &Window) -> String {
    let guard = w.inner.content();
    let r: Vec<String> = guard
        .iter()
        .map(|s| String::from_utf8_lossy(s).to_string())
        .collect();
    r.join(" ")
}

#[test]
fn remove_unmatched() {
    let (tx, rx) = mpsc::sync_channel(50);
    {
        let w = Window::new(3, tx);
        w.inner.pattern.add("o");

        let wv = WalkerVersion::default();
        let add = |t, n| w.add(t, n, &wv).unwrap();

        add("world", 1);
        add("hello", 1);
        add("brave", 0);
        add("odd", 1);

        w.inner.pattern.add("l");

        w.remove_unmatched();

        assert_eq!(content_to_string(&w), "world");
    }

    let msg = rx
        .iter()
        .map(|m| match m {
            Msg::AddFile(bytes) => ("+", bytes),
            Msg::RmFile(bytes) => ("-", bytes),
            o => panic!("Unexpected {o:?}"),
        })
        .map(|(t, b)| format!("{t}{}", str::from_utf8(b.as_ref()).unwrap()))
        .collect::<Vec<_>>()
        .join(" ");

    assert_eq!(msg, "+world +hello +odd -hello -odd")
}

#[test]
fn window_size() {
    let (tx, _rx) = mpsc::sync_channel(50);
    let w = Window::new(3, tx);
    let w2 = w.clone();
    w.inner.pattern.add("o");
    assert_eq!(w.size(), 3);

    let wv = WalkerVersion::default();
    let add = |t, n| w.add(t, n, &wv).unwrap();

    add("world", 1);
    add("hello", 1);
    add("brave", 0);

    assert_eq!(content_to_string(&w), "hello world");

    add("zoo", 1);

    let wv2 = wv.clone();
    let t1 = thread::spawn(move || {
        let add = |t, n| w2.add(t, n, &wv2).unwrap();
        add("1o", 1);
        add("1", 0);
        add("2o", 1);
        add("3o", 1);
    });

    thread::sleep(Duration::from_millis(1));
    assert_eq!(w.inner.content().len(), 3);

    w.remove("hello", 1).unwrap();

    thread::sleep(Duration::from_millis(1));
    assert_eq!(w.inner.content().len(), 3);

    w.remove("world", 1).unwrap();
    w.remove("1o", 1).unwrap();

    let _ = t1.join();

    w.set_size(4);

    add("arrow", 1);

    assert_eq!(content_to_string(&w), "2o 3o arrow zoo");

    w.set_size(2);

    w.remove("2o", 0).unwrap(); // wrong version retested
    assert_eq!(content_to_string(&w), "2o 3o");
}
