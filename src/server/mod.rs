use std::{
    io::{self, Read, Write},
    sync::mpsc,
    thread,
};

use walker::Msg;
use window::Window;

pub mod walker;
pub mod window;

struct CommandReader<R: Read> {
    input: R,
    buf: Vec<u8>,
    startp: usize,
    endp: usize,
}
impl<R: Read> CommandReader<R> {
    fn new(input: R) -> Self {
        Self {
            input,
            buf: vec![0; 50],
            startp: 0,
            endp: 0,
        }
    }

    fn read(&mut self) -> Result<(), walker::Error> {
        loop {
            self.buf.copy_within(self.startp..self.endp, 0);
            self.endp -= self.startp;
            if self.startp == 0 {
                let n = self
                    .input
                    .read(&mut self.buf.as_mut_slice()[self.endp..])
                    .map_err(walker::Error::from_io)?;
                if n == 0 {
                    return Err(walker::Error::Eof);
                }
                self.endp += n;

                if self.endp == self.buf.len() {
                    self.buf.extend_from_within(..);
                }
            } else {
                self.startp = 0;
            }
            let mut iter = self.buf[..self.endp].split(|c| *c == 0);
            if let Some(first) = iter.next()
                && iter.next().is_some()
            {
                self.startp = first.len() + 1;
                return Ok(());
            }
        }
    }

    fn get_cmd(&self) -> Result<(&str, &str), walker::Error> {
        if self.startp > 0 {
            let buf = &self.buf[..self.startp - 1];
            let (cmd, arg) = split_at_space(buf);
            let Ok(cmd) = str::from_utf8(cmd) else {
                return Err(walker::Error::Utf8Error);
            };
            let Ok(arg) = str::from_utf8(arg) else {
                return Err(walker::Error::Utf8Error);
            };
            Ok((cmd, arg))
        } else {
            Err(walker::Error::InvalidCommand)
        }
    }
}

pub fn run(
    threads: usize,
    inp: impl Read,
    out: impl Write + Send + 'static,
) -> Result<(), walker::Error> {
    let mut commander = CommandReader::new(inp);
    let (tx, rx) = mpsc::sync_channel(threads * 2);

    let win = Window::new(threads, tx);
    let mut walker = walker::Walker::new(win);
    let _t1 = thread::spawn(move || {
        let _ = relay_to_out(rx, out);
    });
    loop {
        commander.read()?;
        match commander.get_cmd() {
            Ok((ct, arg)) => {
                walker.command(ct, arg)?;
            }
            Err(err) => {
                walker.message(format!("Command read error: {err:?}"));
            }
        }
    }
}

fn relay_to_out(
    rx: mpsc::Receiver<Msg>,
    mut out: impl Write + Send + 'static,
) -> Result<(), io::Error> {
    while let Ok(msg) = rx.recv() {
        msg.write(&mut out)?;
        while let Ok(msg) = rx.try_recv() {
            msg.write(&mut out)?;
        }
        out.flush()?;
    }
    Ok(())
}

fn split_at_space(data: &[u8]) -> (&[u8], &[u8]) {
    let pos = data.iter().position(|&b| b == b' ').unwrap_or(data.len());
    let (a, b) = data.split_at(pos);
    if b.is_empty() { (a, b) } else { (a, &b[1..]) }
}

fn chars_split_at_space(data: &str) -> (&str, &str) {
    let pos = data.chars().position(|b| b == ' ').unwrap_or(data.len());
    let (a, b) = data.split_at(pos);
    if b.is_empty() { (a, b) } else { (a, &b[1..]) }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod test;
