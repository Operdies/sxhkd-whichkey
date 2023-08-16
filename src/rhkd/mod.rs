use crate::parser::{config::Config, Chord};
use crate::rhkc::ipc::{self, IpcClient, IpcRequestObject, IpcResponse};
use crate::CliArguments;

use super::keyboard;
use super::parser::config;
use nix::sys::select::FdSet;
use nix::sys::signal::Signal::SIGUSR2;
use xcb::x::Event;

use anyhow::{anyhow, bail, Result};
use std::fmt::Display;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use serde::{Deserialize, Serialize};

mod executor;
mod fifo;
mod hotkey_handler;
use hotkey_handler::*;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum IpcMessage {
    Notify(String),
    ConfigReloaded,
    BeginChain,
    EndChain,
    Timeout,
    Hotkey(String),
    Command(String),
}

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
    socket.set_nonblocking(true)?;
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
    let mut sock_buf = String::new();
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
                        use std::io::Read;
                        while let Ok((mut client, _)) = socket.accept() {
                            sock_buf.clear();
                            let mut read_buf = [0; 100];
                            if let Ok(count) = client.read(&mut read_buf) {
                                let str = String::from_utf8_lossy(&read_buf[0..count]);
                                // TODO: Implement IPC
                                println!("Client said: {}", str);
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
            // I'm not sure if this can happen?
            _ => {
                println!("'select' passsed with no reayd fds and no errors?");
            }
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
