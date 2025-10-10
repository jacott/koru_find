use std::{
    cmp::min,
    io::{self, pipe},
    sync::mpsc,
    thread,
    time::Duration,
};

use pretty_assertions::assert_matches;

use super::*;

struct MsgReader<R: Read> {
    input: R,
    buf: Vec<u8>,
    startp: usize,
    endp: usize,
}
impl<R: Read> MsgReader<R> {
    fn new(input: R) -> Self {
        Self {
            input,
            buf: vec![0; 100],
            startp: 0,
            endp: 0,
        }
    }

    fn read(&mut self) -> String {
        self.try_read().unwrap()
    }

    fn try_read(&mut self) -> Result<String, io::Error> {
        loop {
            self.buf.copy_within(self.startp..self.endp, 0);
            self.endp -= self.startp;
            self.startp = 0;
            if self.endp == self.startp {
                let n = self.input.read(self.buf.as_mut_slice())?;
                if n == 0 {
                    return Ok(String::new());
                }
                self.endp += n;

                if self.endp == self.buf.len() {
                    self.buf.extend_from_within(..);
                }
            }
            {
                let mut iter = self.buf[..self.endp].split(|c| *c == 0);
                if let Some(first) = iter.next()
                    && iter.next().is_some()
                {
                    self.startp = first.len() + 1;
                    return Ok(String::from_utf8_lossy(first).to_string());
                }
            }
        }
    }
}

#[test]
fn command_reader() {
    use std::sync::*;

    struct Reader {
        data: Arc<Mutex<Vec<u8>>>,
    }
    impl Read for Reader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut data = self.data.lock().unwrap();
            let dlen = data.len();
            let len = min(data.len(), buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            data.copy_within(len..dlen, 0);
            data.truncate(dlen - len);
            Ok(len)
        }
    }

    let data = Arc::new(Mutex::new(vec![]));

    let reader = Reader { data: data.clone() };

    let mut cr = CommandReader::new(reader);

    data.lock()
        .unwrap()
        .extend_from_slice(b"ignore >_test.rs\x00window_size 85\x00walk ~/src/koru-find\x00");

    cr.read().unwrap();
    let (c, a) = cr.get_cmd().unwrap();
    assert_eq!(c, "ignore");
    assert_eq!(a, ">_test.rs");

    cr.read().unwrap();
    let (c, a) = cr.get_cmd().unwrap();
    assert_eq!(c, "window_size");
    assert_eq!(a, "85");

    cr.read().unwrap();
    let (c, a) = cr.get_cmd().unwrap();
    assert_eq!(c, "walk");
    assert_eq!(a, "~/src/koru-find");

    assert_matches!(cr.read(), Err(walker::Error::Eof));
}

#[test]
fn exceed_window_size() {
    let (out_reader, out_writer) = pipe().unwrap();
    let (in_reader, mut in_writer) = pipe().unwrap();

    let (timeout_tx, timeout_rx) = mpsc::channel();

    let _ = thread::spawn(move || super::run(4, in_reader, out_writer));

    let _ = thread::spawn(move || {
        let mut mr = MsgReader::new(out_reader);

        let _ = in_writer
            .write(b"walk test\x00window_size 1\x00add 1\x00")
            .unwrap();

        assert_eq!(mr.read(), "started");
        let mut files = [mr.read()];
        files.sort();
        assert!(files == ["+a/1/2.txt"] || files == ["+a/1/3.txt"]);

        let _ = in_writer
            .write(b"stop-search\x00walk test\x00add a/2\x00")
            .unwrap();

        assert_eq!(mr.read(), "done");
        assert_eq!(mr.read(), "clear");

        assert_eq!(mr.read(), "started");
        assert_eq!(mr.read(), "+a/1/2.txt");

        timeout_tx.send(true).unwrap();
    });

    assert!(timeout_rx.recv_timeout(Duration::from_millis(200)).unwrap());
}

#[test]
fn run() {
    let (out_reader, out_writer) = pipe().unwrap();
    let (in_reader, mut in_writer) = pipe().unwrap();

    let (timeout_tx, timeout_rx) = mpsc::channel();

    let _ = thread::spawn(move || {
        let mut mr = MsgReader::new(out_reader);

        let _ = in_writer.write(b"walk test\x00window_size 3\x00").unwrap();

        assert_eq!(mr.read(), "started");

        let mut files = [mr.read(), mr.read()];
        files.sort();
        assert_eq!(files, ["+a/1/2.txt", "+a/1/3.txt"]);
        assert_eq!(mr.read(), "done");

        let _ = in_writer
            .write(b"stop-search\x00walk test\x00add a/2\x00")
            .unwrap();

        assert_eq!(mr.read(), "clear");
        assert_eq!(mr.read(), "started");
        assert_eq!(mr.read(), "+a/1/2.txt");
        assert_eq!(mr.read(), "done");

        timeout_tx.send(true).unwrap();
    });

    let _ = thread::spawn(move || super::run(4, in_reader, out_writer));

    assert!(timeout_rx.recv_timeout(Duration::from_millis(500)).unwrap());
}
