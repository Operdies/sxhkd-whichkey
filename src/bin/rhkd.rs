#![allow(unused)]
use lazy_static::lazy_static;
use nix::sys::signal::Signal::SIGTERM;
use rhkd::keyboard;
use rhkd::keyboard::Keyboard;
use rhkd::parser::config;
use rhkd::parser::Hotkey;
use xcb::x::Event::*;
use xcb::x::ModMask;

use anyhow::{anyhow, Context, Result};

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
                _ => Err(anyhow!("Not a key event")),
            },
            xcb::Event::Unknown(_) => Err(anyhow!("Not a key event")),
        }
    }
}

fn setup_signal_handlers(sender: std::sync::mpsc::Sender<Message>) {
    use signal_hook::consts::*;
    std::thread::spawn(move || {
        let sigs = vec![SIGUSR1, SIGUSR2, SIGALRM, SIGINT, SIGTERM];
    });
}

enum Message {
    Shutdown,
    ReloadConfig,
    ToggleGrab,
    Event(Key),
    Timeout,
}

fn main() -> anyhow::Result<()> {
    let cfg = config::load_config(None)?;

    let (sender, receiver) = std::sync::mpsc::channel::<Message>();
    {
        use signal_hook::consts::signal::*;
        use signal_hook::consts::*;
        use signal_hook::iterator::Signals;
        use signal_hook::iterator::SignalsInfo;

        let sender = sender.clone();
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
                    sender.send(Message::Shutdown);
                    res?;
                }
            }
            Ok(())
        });
    };

    std::thread::spawn(move || loop {
        let evt = keyboard::kbd().next_event();
        if let Ok(e) = evt {
            if let Ok(key) = Key::try_from(e) {
                sender.send(Message::Event(key));
            } else {
            }
        } else {
            let e = evt.unwrap_err();
            match e {
                xcb::Error::Connection(_) => {
                    sender.send(Message::Shutdown);
                    return;
                }
                xcb::Error::Protocol(x) => {
                    println!("Protocol error: {}", x);
                    continue;
                }
            }
        }
    });

    let kbd = keyboard::kbd();

    let escape_keysym = "Escape";
    let escape_symbol = keyboard::keysyms::symbol_from_string(escape_keysym).unwrap();
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
