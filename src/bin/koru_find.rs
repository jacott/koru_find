use std::{env, io, path::PathBuf, process};

use clap::Parser;
use koru_find::server;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Base Directory
    dir: Option<PathBuf>,

    /// Server
    #[arg(long)]
    server: bool,
}

fn main() {
    let args = Args::parse();

    let _dir = if let Some(dir) = args.dir {
        match env::set_current_dir(&dir) {
            Ok(_) => dir,
            Err(err) => {
                eprintln!("{err}");
                process::exit(1);
            }
        }
    } else {
        PathBuf::from(".")
    };

    if args.server {
        match server::run(num_cpus::get(), io::stdin(), io::stdout()) {
            Ok(_) => process::exit(0),
            Err(err) => {
                eprintln!("{err}");
                process::exit(1);
            }
        }
    } else {
        todo!()
    }
}

// use std::io::{stdout, Write};
// use std::{
//     io::{self},
//     path::PathBuf,
//     sync::mpsc,
//     thread::spawn,
//     time::Duration,
// };

// use koru_find::{find_files::find_files, pattern::Pattern};

// use crossterm::{
//     event, execute,
//     style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
//     ExecutableCommand,
// };

// fn main() -> std::io::Result<()> {
//     let (t1, rx) = {
//         let path = PathBuf::from("..");

//         let pattern = Pattern::default();
//         pattern.add(".git/config");

//         let (tx, rx) = mpsc::channel();
//         (spawn(|| find_files(path, pattern, tx)), rx)
//     };

//     let to = Duration::from_millis(200);
//     while let Ok(p) = rx.recv_timeout(to) {
//         let _ = io::stdout().write(p.as_bytes());
//         let _ = io::stdout().write(b"\n");
//     }
//     let _ = t1.join();

//     // using the macro
//     execute!(
//         stdout(),
//         SetForegroundColor(Color::Red),
//         SetBackgroundColor(Color::Black),
//         Print("Styled text here."),
//         ResetColor
//     )?;

//     // or using functions
//     stdout()
//         .execute(SetForegroundColor(Color::Blue))?
//         .execute(SetBackgroundColor(Color::Red))?
//         .execute(Print("Styled text here."))?
//         .execute(ResetColor)?;

//     Ok(())
// }
