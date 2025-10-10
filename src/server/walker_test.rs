use std::{sync::mpsc, time::Duration};

use pretty_assertions::assert_matches;

use super::*;

const WT: Duration = Duration::from_millis(200);

fn to_raf(rx: &mut mpsc::Receiver<Msg>, mut count: usize) -> String {
    let mut result = vec![];
    while count > 0
        && let Ok(m) = rx.recv_timeout(WT)
    {
        count -= 1;
        let (t, b) = match m {
            Msg::AddFile(bytes) => ("+", bytes),
            Msg::RmFile(bytes) => ("-", bytes),
            o => ("unexpected ", Bytes::from_owner(format!("{o:?}"))),
        };
        result.push(format!("{t}{}", str::from_utf8(b.as_ref()).unwrap()));
    }
    if count > 0 {
        result.push("timeout".to_string());
    }
    result.sort();
    result.join(" ")
}

pub fn wait_running(walker: &mut Walker, timeout: Duration) {
    let Some(t) = walker.walker_thread.take() else {
        return;
    };
    walker.walker_thread = None;

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let _ = t.join();
        let _ = tx.send(true);
    });

    let _ = rx.recv_timeout(timeout).unwrap();
}

#[test]
fn match_command() {
    let (tx, mut rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win);
    assert!(!walker.is_walking);

    walker.command("add", "123").unwrap();

    walker.command("match", "123456").unwrap();
    walker.command("match", "456").unwrap();
    walker.command("match", "012hello3").unwrap();

    assert_eq!(to_raf(&mut rx, 2), "+012hello3 +123456");

    walker.command("add", "4").unwrap();

    assert_eq!(to_raf(&mut rx, 1), "-012hello3");
    walker.command("rm", "1").unwrap();
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::Resync);
}

#[test]
fn window_size() {
    let (tx, _rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win.clone());

    walker.command("window_size", "3").unwrap();

    assert_eq!(win.size(), 3);
}

#[test]
fn remove() {
    let (tx, mut rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win);

    walker.command("walk", "test").unwrap();

    walker.command("add", "123").unwrap();

    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);

    walker.command("rm", "2").unwrap();

    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(to_raf(&mut rx, 2), "+a/1/2.txt +a/1/3.txt");
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);
}

#[test]
fn stop() {
    let (tx, mut rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win);

    walker.command("walk", "test").unwrap();
    wait_running(&mut walker, WT);
    let _ = rx.try_iter().take(5).count();

    walker.command("set", "0 1/3").unwrap();
    wait_running(&mut walker, WT);
    assert_eq!(to_raf(&mut rx, 1), "-a/1/2.txt");
    assert_matches!(rx.try_recv(), Err(_));

    walker.command("stop", "").unwrap();

    walker.command("set", "0 >2.txt ").unwrap();
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::Clear);

    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::Resync);
    assert_matches!(rx.try_recv(), Err(_));
    walker.command("walk", "test").unwrap();
    walker.command("set", "8 a/1").unwrap();

    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(to_raf(&mut rx, 1), "+a/1/2.txt");
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);
    assert_matches!(rx.try_recv(), Err(_));

    walker.command("ignore", "foo").unwrap();
    assert_eq!(walker.visitor.ignore_pattern.clone_text(), "foo");
    assert_eq!(walker.visitor.pattern.clone_text(), ">2.txt a/1");

    walker.command("stop", "").unwrap();

    assert_eq!(walker.visitor.pattern.clone_text(), "");
    assert_eq!(walker.visitor.ignore_pattern.clone_text(), "");
}

#[test]
fn redraw() {
    let (tx, mut rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win);

    walker.command("walk", "test").unwrap();
    walker.command("set", "0 txt").unwrap();

    let _ = rx.try_iter().take(5).count();

    walker.command("redraw", "").unwrap();
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::Clear);
    assert_eq!(to_raf(&mut rx, 2), "+a/1/2.txt +a/1/3.txt");
    assert_matches!(rx.try_recv(), Err(_));
}

#[test]
fn set() {
    let (tx, mut rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win);

    walker.command("walk", "test").unwrap();
    wait_running(&mut walker, WT);
    let _ = rx.try_iter().take(5).count();

    walker.command("set", "0 1/3").unwrap();
    wait_running(&mut walker, WT);
    assert_eq!(to_raf(&mut rx, 1), "-a/1/2.txt");
    assert_matches!(rx.try_recv(), Err(_));

    walker.command("set", "1 /2").unwrap();
    assert_eq!(to_raf(&mut rx, 1), "-a/1/3.txt");
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(to_raf(&mut rx, 1), "+a/1/2.txt");
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);

    walker.command("set", "2 2tx").unwrap();
    wait_running(&mut walker, WT);
    assert_matches!(rx.try_recv(), Err(_));

    walker.command("set", "1 /2txt").unwrap();

    wait_running(&mut walker, WT);
    assert_matches!(rx.try_recv(), Err(_));
}

#[test]
fn remove_unmatched() {
    let (tx, mut rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win);

    walker.command("walk", "test").unwrap();

    walker.command("add", "1").unwrap();

    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(to_raf(&mut rx, 2), "+a/1/2.txt +a/1/3.txt");
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);

    walker.command("add", "14").unwrap();

    assert_eq!(&to_raf(&mut rx, 2), "-a/1/2.txt -a/1/3.txt");
}

#[test]
fn ends_with() {
    let (tx, mut rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win);

    walker.command("walk", "test/").unwrap();

    walker.command("add", ">.t").unwrap();

    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);

    walker.command("add", "xt").unwrap();

    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(to_raf(&mut rx, 2), "+a/1/2.txt +a/1/3.txt");
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);

    walker.visitor.kill();
}

#[test]
fn add() {
    let (tx, mut rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win);

    walker.command("walk", "test/").unwrap();

    walker.command("add", "1").unwrap();

    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(to_raf(&mut rx, 2), "+a/1/2.txt +a/1/3.txt");
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);

    walker.command("stop", "").unwrap();
    walker.command("walk", "test/a/1").unwrap();
    walker.command("add", "2.t").unwrap();

    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::Clear);
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(to_raf(&mut rx, 1), "+2.txt");
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);

    walker.visitor.kill();
}

#[test]
fn ignore_pattern() {
    let (tx, mut rx) = mpsc::sync_channel(5);
    let win = Window::new(5, tx);
    let mut walker = Walker::new(win);
    walker.command("ignore", ">2.txt").unwrap();

    walker.command("walk", "test/").unwrap();
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::Clear);
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkStarted);
    assert_eq!(to_raf(&mut rx, 1), "+a/1/3.txt");
    assert_eq!(rx.recv_timeout(WT).unwrap(), Msg::WalkDone);
}
