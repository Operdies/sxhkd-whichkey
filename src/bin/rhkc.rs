use clap::{Args, Parser, Subcommand};

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use rhkd::rhkc::ipc::{
    self, BindCommand, IpcCommand, SubscribeCommand, SubscribeEventMask, UnbindCommand,
};

fn alive() -> Option<UnixStream> {
    match UnixStream::connect(ipc::get_socket_path()) {
        Ok(c) => Some(c),
        _ => None,
    }
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
    /// Don't log rhkd response to stdout. This has no effect for 'subscribe'
    #[arg(short = 'q', long = "quiet", default_value_t = false)]
    quiet: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Subscribe to a specified list of events
    Subscribe(Subscription),
    /// Add a new binding
    Bind(BindCommand),
    /// Remove all bindings in a given group
    Unbind(UnbindCommand),
}

#[derive(Args, Debug)]
struct Subscription {
    events: Vec<SubscribeEventMask>,
    /// Automatically reconnect if connection is lost
    #[arg(short = 'r', long = "with-reconnect", default_value_t = false)]
    with_reconnect: bool,
}

fn subscribe(sub: Subscription) {
    let mut conn = connect(sub.with_reconnect);
    let cmd = IpcCommand::Subscribe(SubscribeCommand { events: sub.events });
    let bytes: Vec<u8> = cmd.into();
    conn.write_all(&bytes).expect("Failed to send message");
    let mut buf = [0; 200];
    loop {
        match conn.read(&mut buf) {
            Ok(0) if !sub.with_reconnect => {
                return;
            }
            Ok(0) => {
                println!("Connection broken. Trying to reconnect...");
                std::thread::sleep(Duration::from_secs(1));
                if let Some(a) = alive() {
                    conn = a;
                    println!("Reconnected!");
                } else {
                    println!("Failed to reconnect..");
                }
            }
            Ok(n) => print!("{}", String::from_utf8_lossy(&buf[0..n])),
            Err(e) => {
                eprintln!("Connection broken: {} {}", e, e.kind());
                if sub.with_reconnect {
                    continue;
                } else {
                    break;
                }
            }
        }
    }
}

fn connect(retry: bool) -> UnixStream {
    if retry {
        loop {
            match UnixStream::connect(ipc::get_socket_path()) {
                Ok(s) => break s,
                Err(e) => {
                    eprintln!("Failed to connect to server: {}", e);
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
    } else {
        UnixStream::connect(ipc::get_socket_path()).expect("Failed to connect to server.")
    }
}

fn bind(b: BindCommand, quiet: bool) {
    let mut conn = connect(false);
    let b = IpcCommand::Bind(b);
    let bytes: Vec<u8> = b.into();
    conn.write_all(&bytes).expect("Failed to send message.");
    if !quiet {
        let _ = std::io::copy(&mut conn, &mut std::io::stdout());
    }
}

fn unbind(u: UnbindCommand, quiet: bool) {
    let mut conn = connect(false);
    let u = IpcCommand::Unbind(u);
    let bytes: Vec<u8> = u.into();
    conn.write_all(&bytes).expect("Failed to send message.");
    if !quiet {
        let _ = std::io::copy(&mut conn, &mut std::io::stdout());
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Subscribe(mut s) => {
            if s.events.is_empty() {
                s.events.push(SubscribeEventMask::All);
            }
            subscribe(s);
        }
        Commands::Bind(b) => bind(b, cli.quiet),
        Commands::Unbind(c) => unbind(c, cli.quiet),
    }
}
