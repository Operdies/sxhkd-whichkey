use crate::parser::{config::Config, Chord};
use crate::rhkc::ipc::{self, BindCommand, IpcCommand, UnbindCommand};
use crate::CliArguments;
use std::io::Read;
use std::time::Duration;

use super::keyboard;
use super::parser::config;
use nix::sys::select::FdSet;
use thiserror::Error;
use xcb::x::Event;

use anyhow::{anyhow, bail, Result};
use std::fmt::Display;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod executor;
mod fifo;
pub mod hotkey_handler;
use hotkey_handler::*;

#[derive(Debug)]
pub enum IpcMessage {
    Notify(Arc<str>),
    ConfigReloaded,
    BeginChain,
    EndChain,
    Timeout,
    Hotkey(Arc<str>),
    Command(Arc<str>),
    Error(Arc<str>),
    BindingRemoved(UnbindCommand),
    BindingAdded(BindCommand),
}

#[derive(Error, Debug)]
pub enum IpcMessageParseError {
    #[error("No input")]
    InputEmpty,
    #[error("Unrecognized prefix '{0}'")]
    UnknownPrefix(char),
}

impl TryFrom<String> for IpcMessage {
    type Error = IpcMessageParseError;

    fn try_from(mut value: String) -> std::result::Result<Self, IpcMessageParseError> {
        if value.is_empty() {
            return Err(IpcMessageParseError::InputEmpty);
        }
        let start = value.remove(0);
        let msg = match start {
            'B' => IpcMessage::BeginChain,
            'E' => IpcMessage::EndChain,
            'T' => IpcMessage::Timeout,
            'C' => IpcMessage::Command(value.into()),
            'H' => IpcMessage::Hotkey(value.into()),
            'R' => IpcMessage::ConfigReloaded,
            'N' => IpcMessage::Notify(value.into()),
            '?' => IpcMessage::Error(value.into()),
            _ => return Err(IpcMessageParseError::UnknownPrefix(start)),
        };
        Ok(msg)
    }
}

impl Display for IpcMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcMessage::Notify(n) => write!(f, "N{}", n),
            IpcMessage::ConfigReloaded => write!(f, "RReload"),
            IpcMessage::BeginChain => write!(f, "BBegin chain"),
            IpcMessage::EndChain => write!(f, "EEnd chain"),
            IpcMessage::Timeout => write!(f, "TTimeout reached"),
            IpcMessage::Hotkey(hk) => write!(f, "H{}", hk),
            IpcMessage::Command(c) => write!(f, "C{}", c),
            IpcMessage::Error(e) => write!(f, "?{}", e),
            IpcMessage::BindingRemoved(r) => write!(f, "D{}", r.hotkey),
            IpcMessage::BindingAdded(a) => write!(f, "A{}", a.hotkey),
        }
    }
}

// TODO: Make grab helper grab all combinations of locks ?
fn get_lockfields() -> u32 {
    let num_lock = keyboard::modfield_from_keysym("Num_Lock");
    let scroll_lock = keyboard::modfield_from_keysym("Scroll_Lock");
    let lock = xcb::x::ModMask::LOCK.bits();
    num_lock | scroll_lock | lock
}

lazy_static! {
    static ref LOCK_MASK: u32 = !get_lockfields() & 255;
}

fn as_key(event: &xcb::Event) -> Option<Key> {
    if let Ok(mut key) = Key::try_from(event) {
        key.modfield &= *LOCK_MASK;
        Some(key)
    } else {
        None
    }
}

pub fn start(settings: CliArguments) -> Result<()> {
    let mut hotkey_handler = {
        let cfg = config::load_config(settings.config_path.as_deref())?;
        HotkeyHandler::new(settings, cfg)
    };
    hotkey_handler.setup()?;

    let keyboard_fd = keyboard::kbd().connection().as_raw_fd();
    let ipc_server = ipc::DroppableListener::force()?;
    let socket = &ipc_server.listener;
    socket
        .set_nonblocking(true)
        .expect("Failed to create non-blocking socket");
    let socket_fd = socket.as_raw_fd();

    use signal_hook::consts::signal::*;
    let toggle_grab: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let reload_config: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let timeout: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let terminate: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    signal_hook::flag::register(SIGUSR1, Arc::clone(&reload_config))?;
    signal_hook::flag::register(SIGUSR2, Arc::clone(&toggle_grab))?;
    signal_hook::flag::register(SIGALRM, Arc::clone(&timeout))?;
    signal_hook::flag::register(SIGINT, Arc::clone(&terminate))?;
    signal_hook::flag::register(SIGTERM, Arc::clone(&terminate))?;
    let kbd = keyboard::kbd();
    loop {
        let mut fd_list = FdSet::new();
        fd_list.insert(keyboard_fd);
        fd_list.insert(socket_fd);
        match nix::sys::select::select(None, &mut fd_list, None, None, None) {
            // Select returned because one of the fd's are ready for reading
            Ok(c) if c > 0 => {
                let ready = fd_list.fds(None);
                for fd in ready {
                    if fd == keyboard_fd {
                        // Drain the connection
                        while let Some(evt) = kbd.poll_event()? {
                            if let Some(key) = as_key(&evt) {
                                hotkey_handler.handle_key(key)?;
                            }
                        }
                    } else if fd == socket_fd {
                        while let Ok((mut client, _)) = socket.accept() {
                            let timeout = Duration::from_millis(1);
                            let e = client
                                .set_read_timeout(Some(timeout))
                                .and(client.set_write_timeout(Some(timeout)));
                            if let Err(e) = e {
                                eprintln!("Dropping client: failed to set timeout: {}", e);
                                continue;
                            }
                            let r: &mut dyn Read = &mut client;
                            let parse = IpcCommand::try_from(r);
                            match parse {
                                Ok(c) => match c {
                                    IpcCommand::Bind(binding) => {
                                        hotkey_handler.add_bindings(client, binding);
                                    }
                                    IpcCommand::Unbind(unbind) => {
                                        hotkey_handler.delete_bindings(client, unbind);
                                    }
                                    IpcCommand::Subscribe(subscribe) => {
                                        hotkey_handler.add_subscriber(client, subscribe.events)
                                    }
                                },
                                Err(e) => eprintln!("Failed to parse command: {}", e),
                            }
                        }
                    }
                }
            }
            // An error indicates the select was interrupted by a signal
            Err(_) => {
                if terminate.swap(false, Ordering::Relaxed) {
                    hotkey_handler.cleanup()?;
                    return Ok(());
                }
                if timeout.swap(false, Ordering::Relaxed) {
                    hotkey_handler.timeout()?;
                }
                if reload_config.swap(false, Ordering::Relaxed) {
                    hotkey_handler.reload()?;
                }
                if toggle_grab.swap(false, Ordering::Relaxed) {
                    hotkey_handler.toggle_grab()?;
                }
            }
            // TODO: I'm not sure if this can happen?
            _ => {}
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Key {
    symbol: u8,
    #[allow(unused)]
    modfield: u32,
    is_press: bool,
}

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let modfield = xcb::x::ModMask::from_bits_truncate(self.modfield);
        f.write_str(&format!("{:?} ", modfield))?;
        if !self.is_press {
            f.write_str("@")?;
        }
        f.write_str(&format!("{}", self.symbol))
    }
}

impl PartialEq<Chord> for Key {
    fn eq(&self, other: &Chord) -> bool {
        self.modfield == other.modfield.bits()
            && self.is_press == other.event_type.is_key_press()
            && {
                keyboard::kbd()
                    .get_keycodes(other.keysym)
                    .is_some_and(|i| i.contains(&self.symbol))
            }
    }
}

impl TryFrom<xcb::Event> for Key {
    type Error = anyhow::Error;
    fn try_from(value: xcb::Event) -> std::result::Result<Self, Self::Error> {
        Self::try_from(&value)
    }
}

impl TryFrom<&xcb::Event> for Key {
    type Error = anyhow::Error;

    fn try_from(value: &xcb::Event) -> std::result::Result<Self, Self::Error> {
        match value {
            xcb::Event::X(x) => match x {
                Event::KeyPress(x) => Ok(Key {
                    symbol: x.detail(),
                    modfield: x.state().bits(),
                    is_press: true,
                }),
                Event::KeyRelease(x) => Ok(Key {
                    symbol: x.detail(),
                    modfield: x.state().bits(),
                    is_press: false,
                }),
                _ => bail!("Not a key event"),
            },
            xcb::Event::Unknown(_) => Err(anyhow!("Not a key event")),
        }
    }
}
