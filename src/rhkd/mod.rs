#![allow(unused_imports)]

use crate::Config;

use super::keyboard;
use super::keyboard::Keyboard;
use super::parser::config;
use super::parser::Hotkey;
use xcb::x::Event::*;
use xcb::x::ModMask;

use anyhow::{anyhow, bail, Context, Result};
use std::sync::mpsc;

enum Message {
    Shutdown,
    ReloadConfig,
    ToggleGrab,
    Event(Key),
    Timeout,
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
                SIGINT | SIGTERM | SIGHUP => {
                    break;
                }
                _ => Ok(()),
            };
            if res.is_err() {
                sender.send(Message::Shutdown)?;
                res?;
            }
        }
        Ok(())
    });
    Ok(())
}

fn start_keyboard_handler(sender: mpsc::Sender<Message>) {
    std::thread::spawn(move || -> Result<()> {
        loop {
            let evt = keyboard::kbd().next_event();
            if let Ok(e) = evt {
                if let Ok(key) = Key::try_from(e) {
                    sender.send(Message::Event(key))?;
                } else {
                }
            } else {
                let e = evt.unwrap_err();
                match e {
                    xcb::Error::Connection(_) => {
                        sender.send(Message::Shutdown)?;
                        return Ok(());
                    }
                    xcb::Error::Protocol(x) => {
                        println!("Protocol error: {}", x);
                        continue;
                    }
                }
            }
        }
    });
}

pub fn start(settings: Config) -> Result<()> {
    let cfg = config::load_config(settings.config_path.as_deref())?;
    let (sender, receiver) = mpsc::channel();

    start_signal_handler(sender.clone())?;
    start_keyboard_handler(sender);

    let kbd = keyboard::kbd();

    let escape_keysym = settings.abort_keysym.unwrap_or("Escape".into());
    let escape_symbol = keyboard::symbol_from_string(&escape_keysym)?;
    let escape_symbols = kbd.get_keycodes(escape_symbol).unwrap();
    for keycode in escape_symbols.iter() {
        if let Err(e) = kbd.grab(*keycode, ModMask::from_bits(0).unwrap()) {
            println!("{:?}", e);
        }
    }

    grab(kbd, &cfg)?;
    let mut state = vec![];
    for message in receiver.iter() {
        let Message::Event(evt) = message else { continue };
        dbg!(&evt);
        let evt = kbd.next_event();
        let evt = match evt {
            Err(e) => match e {
                xcb::Error::Connection(_) => break,
                xcb::Error::Protocol(x) => {
                    println!("Protocol error: {}", x);
                    continue;
                }
            },
            Ok(e) => e,
        };
        println!("{:?}", evt);
        let Ok(key) = Key::try_from(evt) else {
            kbd.replay_keyboard()?;
            state.clear();
            continue;
        };
        if key.is_press && escape_symbols.contains(&key.symbol) {
            println!("Chain aborted!");
            kbd.replay_keyboard()?;
            state.clear();
            continue;
        }

        state.push(key);
        kbd.replay_keyboard()?;
    }
    Ok(())
}

fn ungrab(kbd: &Keyboard) -> Result<()> {
    Ok(kbd.ungrab_all()?)
}

fn grab(kbd: &Keyboard, cfg: &[Hotkey]) -> Result<()> {
    ungrab(kbd)?;
    'outer: for hotkey in cfg {
        for chain in &hotkey.chain {
            if let Some(keycodes) = kbd.get_keycodes(chain.keysym) {
                for keycode in keycodes {
                    if let Err(e) = keyboard::kbd().grab(keycode, chain.modfield.into()) {
                        println!("Error: {}", e);
                    } else {
                        println!("Grabbed {:?}", chain);
                    }
                }
            } else {
                println!(
                    "Failed to get keycode for symbol in chain: {:?}. Skipping this hotkey. ({:?})",
                    chain, hotkey
                );
                continue 'outer;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Copy, Clone)]
struct Key {
    symbol: u8,
    #[allow(unused)]
    modfield: u32,
    is_press: bool,
}

impl TryFrom<xcb::Event> for Key {
    type Error = anyhow::Error;

    fn try_from(value: xcb::Event) -> std::result::Result<Self, Self::Error> {
        match value {
            xcb::Event::X(x) => match x {
                KeyPress(x) => Ok(Key {
                    symbol: x.detail(),
                    modfield: x.state().bits(),
                    is_press: true,
                }),
                KeyRelease(x) => Ok(Key {
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
