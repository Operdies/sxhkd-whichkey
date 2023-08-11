use std::{fs::File, io::BufReader};

use clap::Parser;

use super::{
    command::{self, FifoReader, Stroke},
    config::load_config,
    types::{Chord, Hotkeys},
};

pub struct Subscriber {
    generator: Box<dyn Iterator<Item = Stroke>>,
    config: Hotkeys,
}

#[derive(Debug, Clone)]
pub enum Event {
    ChainStarted,
    ChainEnded,
    KeyEvent(KeyEvent),
    CommandEvent(CommandEvent),
}

#[derive(Debug, Clone)]
pub struct CommandEvent {
    pub command: String,
}
#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub config: Hotkeys,
    pub keys: Vec<Chord>,
}

fn get_valid_continuations(cfg: &Hotkeys, strokes: &[Chord]) -> Hotkeys {
    let mut result = vec![];
    let n_strokes = strokes.len();
    for hk in cfg {
        if hk.chain.len() <= n_strokes {
            continue;
        }
        if hk
            .chain
            .iter()
            .zip(strokes)
            .all(|(chain, stroke)| chain.repr.eq(&stroke.repr))
        {
            let mut hk = hk.clone();
            hk.chain = hk.chain.into_iter().skip(n_strokes).collect();
            result.push(hk);
        }
    }
    result
}

impl Default for Subscriber {
    fn default() -> Self {
        let args = crate::cmd::Config::parse();
        let config = load_config(args.config_path.as_deref()).unwrap();

        // TODO: Use socket if fifo is not set
        let fifo = args.status_fifo.unwrap();

        let file = File::open(fifo).unwrap();
        let reader = BufReader::new(file);
        let cmd = command::FifoReader::new(reader);
        Self::new(cmd, config)
    }
}

impl Iterator for Subscriber {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let next = self.generator.next();
            match next {
                Some(ref e) => {
                    if Self::is_restart(e) {
                        // reload the config by reassigning self
                        *self = Self::default();
                    } else if let Some(e) = Self::parse(e, &self.config) {
                        return Some(e);
                    }
                }
                None => return None,
            }
        }
    }
}

impl Subscriber {
    pub fn new(reader: FifoReader, config: Hotkeys) -> Self {
        Self {
            generator: Box::new(reader),
            config,
        }
    }

    fn is_restart(stroke: &Stroke) -> bool {
        let restart_signal = "pkill -USR1 -x sxhkd";
        match stroke {
            Stroke::Command(s) => s == restart_signal,
            _ => false,
        }
    }
    pub fn parse(stroke: &Stroke, cfg: &Hotkeys) -> Option<Event> {
        match stroke {
            Stroke::Hotkey(ref h) => {
                dbg!(h);
                let Ok(chords) = super::parse_chord_chain(h) else {
                    println!("Failed to parse hotkey from '{}'", h);
                    return None;
                };
                let continuations = get_valid_continuations(cfg, &chords);
                if continuations.is_empty() {
                    None
                } else {
                    Some(Event::KeyEvent(KeyEvent {
                        config: continuations,
                        keys: chords,
                    }))
                }
            }
            Stroke::EndChain(_) => Some(Event::ChainEnded),
            Stroke::BeginChain(_) => Some(Event::ChainStarted),
            Stroke::Command(s) => Some(Event::CommandEvent(CommandEvent { command: s.into() })),
            _ => None,
        }
    }

    pub fn register<F>(self, callback: F)
    where
        F: Fn(Event) -> bool,
    {
        // TODO: Implement this in a less hardcoded way -- get sxhkd pid and monitor what
        // interrupts it receives?
        let mut restart = false;
        for stroke in self.generator {
            if let Some(event) = Self::parse(&stroke, &self.config) {
                if callback(event) {
                    break;
                }
            }
            if Self::is_restart(&stroke) {
                restart = true;
                break;
            };
        }

        if restart {
            Self::default().register(callback)
        }
    }
}
