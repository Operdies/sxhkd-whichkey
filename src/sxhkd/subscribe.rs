use std::{fs::File, io::BufReader};

use clap::Parser;

use super::{
    bindings::Config,
    command::{self, FifoReader, Stroke},
    config::parse_config,
};

pub struct Subscriber {
    reader: FifoReader,
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
    pub config: Config,
    pub keys: Vec<String>,
}

fn get_valid_continuations(cfg: &Config, strokes: &[&str]) -> Config {
    let mut result = vec![];
    let n_strokes = strokes.len();
    for hk in cfg {
        if hk.chain.len() <= n_strokes {
            continue;
        }
        if hk.chain.iter().zip(strokes).all(|e| e.0.repr.eq(e.1)) {
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
        let config = parse_config(args.config_path);

        let file = File::open(args.status_fifo).unwrap();
        let reader = BufReader::new(file);
        let cmd = command::FifoReader::new(reader);
        Self::new(cmd, config)
    }
}

impl Iterator for Subscriber {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let next = self.reader.next();

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
    pub fn new(reader: FifoReader, config: Config) -> Self {
        Self { reader, config }
    }

    fn is_restart(stroke: &Stroke) -> bool {
        let restart_signal = "pkill -USR1 -x sxhkd";
        match stroke {
            Stroke::Command(s) => s == restart_signal,
            _ => false,
        }
    }
    pub fn parse(stroke: &Stroke, cfg: &Config) -> Option<Event> {
        match stroke {
            Stroke::Hotkey(ref h) => {
                let keys: Vec<&str> = h.split(';').collect();
                let continuations = get_valid_continuations(cfg, &keys);
                if continuations.is_empty() {
                    None
                } else {
                    Some(Event::KeyEvent(KeyEvent {
                        config: continuations,
                        keys: keys.iter().map(|c| c.to_string()).collect(),
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
        // TODO: Implement this in a less hardcoded way
        let mut restart = false;
        for stroke in self.reader {
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
            println!("Reloaded config!");
            Self::default().register(callback)
        }
    }
}
