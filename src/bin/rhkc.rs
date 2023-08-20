use clap::{Args, Parser, Subcommand};

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use rhkd::rhkc::ipc::{
    self, BindCommand, IpcCommand, SubscribeCommand, SubscribeEventMask, UnbindCommand,
};

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

fn subscribe(sub: Subscription) -> Result<(), std::io::Error> {
    let cmd = IpcCommand::Subscribe(SubscribeCommand { events: sub.events });
    let bytes: Vec<u8> = cmd.into();
    let reconnect = || -> Result<UnixStream, std::io::Error> {
        let mut new_conn = connect()?;
        new_conn.write_all(&bytes)?;
        Ok(new_conn)
    };

    let mut conn = if sub.with_reconnect {
        loop {
            if let Ok(c) = reconnect() {
                break c;
            }
        }
    } else {
        reconnect()?
    };
    let mut buf = [0; 200];

    loop {
        match conn.read(&mut buf) {
            Ok(0) if !sub.with_reconnect => break Ok(()),
            Ok(0) => {
                println!("Connection broken.");
                if let Ok(new) = reconnect() {
                    println!("Reconnected!");
                    conn = new;
                }
            }
            Ok(n) => print!("{}", String::from_utf8_lossy(&buf[0..n])),
            Err(e) => {
                eprintln!("Connection broken: {} {}", e, e.kind());
                if sub.with_reconnect {
                    println!("Connection broken.");
                    if let Ok(new) = reconnect() {
                        println!("Reconnected!");
                        conn = new;
                    }
                } else {
                    break Ok(());
                }
            }
        }
    }
}

fn connect() -> Result<UnixStream, std::io::Error> {
    let wait_ms = [10, 25, 50, 100, 125, 150, 200, 300, 400, 500];
    for (i, ms) in wait_ms.iter().enumerate() {
        if let Ok(stream) = UnixStream::connect(ipc::get_socket_path()) {
            return Ok(stream);
        }
        eprint!("\rFailed to connect to server. ({}/{})", i + 1, wait_ms.len());
        std::thread::sleep(Duration::from_millis(*ms));
    }
    eprintln!();
    UnixStream::connect(ipc::get_socket_path())
}

fn bind(b: BindCommand, quiet: bool) -> Result<(), std::io::Error> {
    let mut conn = connect()?;
    let b = IpcCommand::Bind(b);
    let bytes: Vec<u8> = b.into();
    conn.write_all(&bytes)?;
    if !quiet {
        std::io::copy(&mut conn, &mut std::io::stdout())?;
    }
    Ok(())
}

fn unbind(u: UnbindCommand, quiet: bool) -> Result<(), std::io::Error> {
    let mut conn = connect()?;
    let u = IpcCommand::Unbind(u);
    let bytes: Vec<u8> = u.into();
    conn.write_all(&bytes)?;
    if !quiet {
        std::io::copy(&mut conn, &mut std::io::stdout())?;
    }
    Ok(())
}

fn main() -> Result<(), std::io::Error> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Subscribe(mut s) => {
            if s.events.is_empty() {
                s.events.push(SubscribeEventMask::All);
            }
            subscribe(s)
        }
        Commands::Bind(b) => bind(b, cli.quiet),
        Commands::Unbind(c) => unbind(c, cli.quiet),
    }
}
