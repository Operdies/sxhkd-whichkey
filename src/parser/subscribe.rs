use std::{fs::File, io::BufReader};

use super::{
    command::{self, FifoReader, Stroke},
    config::{load_config, Config},
    types::{Chord, Hotkey},
};

pub struct Subscriber {
    generator: Box<dyn Iterator<Item = Stroke>>,
    config: Config,
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
    pub config: Vec<Hotkey>,
    pub keys: Vec<Chord>,
    pub current_index: usize,
}

fn get_valid_continuations(cfg: &Vec<Hotkey>, strokes: &[Chord]) -> Vec<Hotkey> {
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
            result.push(hk.clone());
        }
    }
    result
}

impl Default for Subscriber {
    fn default() -> Self {
        let args = crate::CliArguments::default();
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
                    } else if let Some(e) = Self::parse(e, self.config.get_hotkeys()) {
                        return Some(e);
                    }
                }
                None => return None,
            }
        }
    }
}

impl Subscriber {
    pub fn new(reader: FifoReader, config: Config) -> Self {
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
    pub fn parse(stroke: &Stroke, cfg: &Vec<Hotkey>) -> Option<Event> {
        match stroke {
            Stroke::Hotkey(ref h) => {
                let Ok(chords) = super::parse_chord_chain(h) else {
                    println!("Failed to parse hotkey from '{}'", h);
                    return None;
                };
                let current_index = chords.len();
                let continuations = get_valid_continuations(cfg, &chords);
                if continuations.is_empty() {
                    None
                } else {
                    Some(Event::KeyEvent(KeyEvent {
                        config: continuations,
                        keys: chords,
                        current_index,
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
            if let Some(event) = Self::parse(&stroke, self.config.get_hotkeys()) {
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
