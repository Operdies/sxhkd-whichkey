use crate::parser::{config::Config, Chord};
use crate::CliArguments;

use super::keyboard;
use super::parser::config;
use xcb::x::Event;

use anyhow::{anyhow, bail, Result};
use std::fmt::Display;
use std::sync::mpsc;

use serde::{Deserialize, Serialize};

mod hotkey_handler;
mod executor;
mod fifo;
use hotkey_handler::*;

#[derive(Debug)]
enum Message {
    Shutdown,
    ReloadConfig,
    ToggleGrab,
    Event(Key),
    Timeout,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum IpcMessage {
    Notify(String),
    ConfigReloaded,
    BeginChain,
    EndChain,
    Timeout,
    Hotkey(String),
    Command(String),
}

fn start_signal_handler(sender: mpsc::Sender<Message>) -> Result<()> {
    use signal_hook::consts::signal::*;
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGUSR1, SIGUSR2, SIGALRM, SIGINT, SIGTERM])?;
    std::thread::spawn(move || -> anyhow::Result<()> {
        for signal in &mut signals {
            let res = match signal {
                SIGUSR1 => sender.send(Message::ReloadConfig),
                SIGUSR2 => sender.send(Message::ToggleGrab),
                SIGALRM => sender.send(Message::Timeout),
                SIGINT | SIGTERM | SIGHUP => sender.send(Message::Shutdown),

                _ => Ok(()),
            };
            if res.is_err() {
                eprintln!("Failed to send singal across chanenl. Shutting down.");
                sender.send(Message::Shutdown)?;
                res?;
            }
        }
        Ok(())
    });
    Ok(())
}

fn get_lockfields() -> u32 {
    let num_lock = keyboard::modfield_from_keysym("Num_Lock");
    let scroll_lock = keyboard::modfield_from_keysym("Scroll_Lock");
    let lock = xcb::x::ModMask::LOCK.bits();
    num_lock | scroll_lock | lock
}

fn start_keyboard_handler(sender: mpsc::Sender<Message>) {
    let lock = get_lockfields();
    let mask = !lock & 255;
    std::thread::spawn(move || -> Result<()> {
        loop {
            let evt = keyboard::kbd().next_event();
            if let Ok(e) = evt {
                if let Ok(mut key) = Key::try_from(&e) {
                    // Discard num lock / caps lock / scroll lock
                    key.modfield &= mask;
                    sender.send(Message::Event(key))?;
                }
            } else {
                let e = evt.unwrap_err();
                match e {
                    xcb::Error::Connection(_) => {
                        break;
                    }
                    xcb::Error::Protocol(x) => {
                        println!("Protocol error: {}", x);
                        continue;
                    }
                }
            }
        }
        sender.send(Message::Shutdown)?;
        Ok(())
    });
}

pub fn start(settings: CliArguments) -> Result<()> {
    let (sender, receiver) = mpsc::channel();

    let mut state = {
        let cfg = config::load_config(settings.config_path.as_deref())?;
        HotkeyHandler::new(settings, cfg)
    };

    state.setup()?;

    start_signal_handler(sender.clone())?;
    start_keyboard_handler(sender);

    for message in receiver.iter() {
        match message {
            Message::Event(key) => state.handle_key(key)?,
            Message::ReloadConfig => state.reload()?,
            Message::ToggleGrab => state.toggle_grab()?,
            Message::Timeout => state.timeout()?,
            Message::Shutdown => {
                state.cleanup()?;
                break;
            }
        }
    }
    Ok(())
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
