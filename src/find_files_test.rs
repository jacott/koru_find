use std::{env, path::Path, thread::spawn, time::Duration};

use super::*;

#[test]
fn find_files() {
    let (t1, rx) = {
        let cd = env!("CARGO_MANIFEST_DIR");
        let cd = Path::new(&cd);
        env::set_current_dir(cd).unwrap();
        let path = PathBuf::from("test");

        let pattern = Pattern::default();
        pattern.add("1");

        let (tx, rx) = mpsc::channel();
        (spawn(|| super::find_files(path, pattern, tx)), rx)
    };

    let to = Duration::from_millis(200);
    let mut result = vec![];
    while let Ok(p) = rx.recv_timeout(to) {
        result.push(p);
    }

    assert!(t1.join().is_ok());

    result.sort();

    assert_eq!(result.len(), 2);
    assert_eq!(result[0], b"test/a/1");
    assert_eq!(result[1], b"test/a/1/2.txt");
}
