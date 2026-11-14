//! Test helper standing in for UxPlay. First arg selects a script; extra args
//! are ignored except `--pid-file=<path>` (write own PID) and
//! `--count-file=<path>` (append one line per launch).

use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

const LISTENING: &str = "Initialized server socket(s)";
const CONNECT: &str =
    "connection request from Test iPhone (iPhone14,2) with deviceID = AA:BB:CC:DD:EE:FF";

fn forever() -> ! {
    loop {
        sleep(Duration::from_secs(3600));
    }
}

fn closes() {
    for n in 1..=3 {
        println!("Connection closed on socket {n}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for a in &args {
        if let Some(p) = a.strip_prefix("--pid-file=") {
            let _ = std::fs::write(p, std::process::id().to_string());
        } else if let Some(p) = a.strip_prefix("--count-file=") {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "launch");
            }
        }
    }
    match args.first().map(String::as_str) {
        Some("ok") => {
            println!("{LISTENING}");
            forever()
        }
        Some("connect") => {
            println!("{LISTENING}");
            println!("{CONNECT}");
            forever()
        }
        Some("connect_then_disconnect") => {
            println!("{LISTENING}");
            println!("{CONNECT}");
            sleep(Duration::from_millis(200));
            closes();
            forever()
        }
        Some("reconnect") => {
            println!("{LISTENING}");
            println!("{CONNECT}");
            sleep(Duration::from_millis(200));
            closes();
            sleep(Duration::from_millis(300));
            println!("{CONNECT}");
            forever()
        }
        Some("crash") => {
            eprintln!("*** ERROR: fake failure");
            std::process::exit(1)
        }
        Some("crash_after_ok") => {
            println!("{LISTENING}");
            sleep(Duration::from_millis(300));
            std::process::exit(1)
        }
        Some("error_live") => {
            eprintln!("*** ERROR: Error initialising socket 10013");
            forever()
        }
        _ => std::process::exit(2),
    }
}
