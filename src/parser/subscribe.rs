use std::{fs::File, io::BufReader};

use crate::{rhkc::ipc::SocketReader, CliArguments};

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

impl Subscriber {
    pub fn new(fifo: &str) -> Self {
        let args = crate::CliArguments::default();
        let config = load_config(args.config_path.as_deref()).unwrap();
        let file = File::open(fifo).unwrap();
        let reader = BufReader::new(file);
        let cmd = command::FifoReader::new(reader);
        Self::from_fiforeader(cmd, config)
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
                        *self = Self::new(&CliArguments::default().status_fifo.unwrap())
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
    pub fn from_fiforeader(reader: FifoReader, config: Config) -> Self {
        Self {
            generator: Box::new(reader),
            config,
        }
    }

    fn is_restart(stroke: &Stroke) -> bool {
        let restart_signal = "pkill -USR1 -x sxhkd";
        match stroke {
            Stroke::Command(s) => s == restart_signal,
            Stroke::Reload => true,
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
}
