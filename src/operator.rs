//! Operator commands are available only on the service's standard input.
use std::{
    io::{BufRead, Write},
    process::{Command, Stdio},
};

use tokio::sync::oneshot;

use crate::sessions::Sessions;

fn show_invite(origin: &str, token: &str, pin: &str) -> std::io::Result<()> {
    let mut child = Command::new("qrencode")
        .args(["-t", "ANSIUTF8", "-m", "2", "-o", "-"])
        .stdin(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let result = write!(stdin, "{origin}/#invite={token}");
    drop(stdin);
    let status = child.wait()?;
    println!("One-time PIN {pin} expires in two minutes.");
    result?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("qrencode failed"))
    }
}

pub(crate) fn start(sessions: Sessions, origin: String) -> oneshot::Receiver<()> {
    let (stop, stopped) = oneshot::channel();
    std::thread::spawn(move || {
        println!("Commands: invite | list | approve ID | reject ID | end ID | quit");
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            let fields: Vec<_> = line.split_whitespace().collect();
            match fields.as_slice() {
                ["invite"] => {
                    let invite = sessions.lock().invite();
                    match invite {
                        Ok((token, pin)) => {
                            if show_invite(&origin, &token, &pin).is_err() {
                                println!("QR unavailable. Use PIN {pin} or install qrencode.");
                            }
                        }
                        Err(_) => println!("Invite limit reached."),
                    }
                }
                ["list"] => print!("{}", sessions.lock().describe()),
                ["quit"] => break,
                [command @ ("approve" | "reject" | "end"), id] => {
                    let found = id.parse().ok().is_some_and(|id| {
                        let mut registry = sessions.lock();
                        match *command {
                            "end" => registry.end(id),
                            "approve" => registry.resolve(id, true),
                            _ => registry.resolve(id, false),
                        }
                    });
                    println!("{}", if found { "OK" } else { "Unknown or expired ID" });
                }
                _ => println!("Commands: invite | list | approve ID | reject ID | end ID | quit"),
            }
        }
        let _ = stop.send(());
    });
    stopped
}
