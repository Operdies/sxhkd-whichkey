use std::io::Read;
use std::ops::BitAnd;
use std::{os::unix::net::UnixListener, path::PathBuf};

use clap::{arg, Args, ValueEnum};
use thiserror::Error;

pub fn get_socket_path() -> String {
    std::env::var("RHKD_SOCKET_PATH").unwrap_or(format!(
        "/tmp/rhkd_socket_{}",
        std::env::var("DISPLAY").unwrap_or("_".to_string())
    ))
}

pub struct DroppableListener {
    path: PathBuf,
    pub listener: UnixListener,
}
impl DroppableListener {
    pub fn force() -> Result<Self, std::io::Error> {
        let path = get_socket_path();
        let _ = std::fs::remove_file(&path);
        Self::new(path.into())
    }
    pub fn new(path: PathBuf) -> Result<Self, std::io::Error> {
        let listener = UnixListener::bind(&path)?;
        Ok(Self { path, listener })
    }
}

impl Drop for DroppableListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Args, Debug)]
pub struct UnbindCommand {
    pub hotkey: String,
}
#[derive(Args, Debug)]
pub struct BindCommand {
    /// Whether or not to overwrite existing bindings
    #[arg(short, long, default_value_t = false)]
    pub overwrite: bool,
    /// Hotkey text. Same syntax as sxhkdrc
    pub hotkey: String,
    /// Command text. Same syntax as sxhkdrc
    #[arg(short, long)]
    pub command: String,
    /// Title for this binding
    #[arg(short, long)]
    pub title: Option<String>,
    /// Description of this binding. Replacement is supported with the same syntax as commands and
    /// hotkeys
    #[arg(short, long)]
    pub description: Option<String>,
}

#[derive(ValueEnum, Debug, Clone, PartialEq, Copy)]
pub enum SubscribeEventMask {
    Notifications = 1,
    Reload = 2,
    Errors = 4,
    Timeout = 8,
    Chain = 16,
    Hotkey = 32,
    Command = 64,
    All = 127,
}

impl BitAnd<u8> for SubscribeEventMask {
    type Output = u8;

    fn bitand(self, rhs: u8) -> Self::Output {
        (self as u8) & rhs
    }
}

impl SubscribeEventMask {
    fn has(self, u: u8) -> bool {
        (self as u8 & u) == (self as u8)
    }

    fn vec_from_u8(u: u8) -> Vec<SubscribeEventMask> {
        use SubscribeEventMask::*;
        if All.has(u) {
            return vec![All];
        }
        let all: &[_] = &[
            Notifications,
            Reload,
            Errors,
            Timeout,
            Chain,
            Hotkey,
            Command,
        ];
        all.iter().filter(|v| v.has(u)).copied().collect()
    }
}

pub struct SubscribeCommand {
    pub events: Vec<SubscribeEventMask>,
}

pub enum IpcCommand {
    Bind(BindCommand),
    Unbind(UnbindCommand),
    Subscribe(SubscribeCommand),
}

#[derive(Error, Debug)]
pub enum IpcCommandError {
    #[error("Reader error")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse input: {0}")]
    ParseError(String),
    #[error("Read 0 bytes")]
    Empty,
    #[error("The payload did not contain any null separators.")]
    NoSplit,
    #[error("Attempted to subscribe to 0 events")]
    NoEvents,
    #[error("The command contained trailing garbage")]
    TrailingGarbage,
    #[error("Expected 4 arguments in binding")]
    BindingError,
}

impl TryFrom<&mut dyn Read> for IpcCommand {
    type Error = IpcCommandError;

    fn try_from(value: &mut dyn Read) -> Result<Self, Self::Error> {
        let mut buf = [0; 200];
        let mut all = vec![];

        loop {
            match value.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => all.extend_from_slice(&buf[0..n]),
                Err(_) => break,
            }
        }

        let buckets: Vec<Vec<u8>> = all.split(|b| *b == 0).map(|i| i.to_vec()).collect();
        if buckets.is_empty() {
            return Err(IpcCommandError::Empty);
        }
        if buckets.len() == 1 {
            return Err(IpcCommandError::NoSplit);
        }

        let first = &buckets[0];
        match first[..] {
            [b'B'] => {
                if buckets.len() < 5 {
                    return Err(IpcCommandError::BindingError);
                }
                let title = &buckets[1];
                let description = &buckets[2];
                let hotkey = &buckets[3];
                let command = &buckets[4];
                let overwrite = &buckets[5];
                let title = if !title.is_empty() { Some(title) } else { None };
                let description = if !description.is_empty() {
                    Some(description)
                } else {
                    None
                };
                let title = title.map(|t| String::from_utf8_lossy(t).to_string());
                let description = description.map(|t| String::from_utf8_lossy(t).to_string());
                let hotkey = String::from_utf8_lossy(hotkey).to_string();
                let command = String::from_utf8_lossy(command).to_string();
                Ok(IpcCommand::Bind(BindCommand {
                    title,
                    description,
                    hotkey,
                    command,
                    overwrite: overwrite[0] == b't',
                }))
            }
            [b'U'] => {
                // parse Unbinding
                let hotkey = String::from_utf8_lossy(&buckets[1]).to_string();
                Ok(IpcCommand::Unbind(UnbindCommand { hotkey }))
            }
            [b'S'] => {
                // parse Subscription
                let flags = &buckets[1];
                if flags.len() > 1 {
                    return Err(IpcCommandError::TrailingGarbage);
                }

                if flags.len() != 1 {
                    return Err(IpcCommandError::ParseError(
                        "Trailing garbage in event flags".to_string(),
                    ));
                }

                let mask = SubscribeEventMask::vec_from_u8(flags[0]);
                if mask.is_empty() {
                    return Err(IpcCommandError::NoEvents);
                }
                Ok(IpcCommand::Subscribe(SubscribeCommand { events: mask }))
            }
            _ => Err(IpcCommandError::ParseError(format!(
                "Unrecognized discriminant: {}",
                String::from_utf8_lossy(first)
            ))),
        }
    }
}

impl From<IpcCommand> for Vec<u8> {
    fn from(value: IpcCommand) -> Self {
        let mut result = vec![];
        match value {
            IpcCommand::Bind(b) => {
                result.push(b'B');
                result.push(0);
                if let Some(t) = b.title {
                    result.extend_from_slice(t.as_bytes());
                }
                result.push(0);
                if let Some(d) = b.description {
                    result.extend_from_slice(d.as_bytes());
                }
                result.push(0);
                result.extend_from_slice(b.hotkey.as_bytes());
                result.push(0);
                result.extend_from_slice(b.command.as_bytes());
                result.push(0);
                result.push(if b.overwrite { b't' } else { b'f' });
                result.push(0);
            }
            IpcCommand::Unbind(u) => {
                result.push(b'U');
                result.push(0);
                result.extend_from_slice(u.hotkey.as_bytes());
                result.push(0);
            }
            IpcCommand::Subscribe(s) => {
                let sub = s.events;
                result.push(b'S');
                result.push(0);
                let mut mask: u8 = 0;
                for item in sub {
                    mask |= item as u8;
                }
                result.push(mask);
            }
        }
        result
    }
}
