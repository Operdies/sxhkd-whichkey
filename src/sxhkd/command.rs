// The following prefixes are defined in sxhkd.h:
// #define HOTKEY_PREFIX       'H'
// #define COMMAND_PREFIX      'C'
// #define BEGIN_CHAIN_PREFIX  'B'
// #define END_CHAIN_PREFIX    'E'
// #define TIMEOUT_PREFIX      'T'

use std::fs::File;
use std::io::{BufRead, BufReader};

use clap::Parser;

pub struct FifoReader {
    fifo: BufReader<File>,
    buf: String,
}

#[derive(Debug)]
pub enum Stroke {
    Hotkey(String),
    Command(String),
    BeginChain(String),
    EndChain(String),
    Timeout(String),
    EOF,
}

impl Default for FifoReader {
    fn default() -> Self {
        let args = crate::cmd::Config::parse();
        let file = File::open(args.status_fifo).unwrap();
        let reader = BufReader::new(file);
        Self::new(reader)
    }
}

impl FifoReader {
    pub fn new(fifo: BufReader<File>) -> Self {
        let buf = String::new();
        FifoReader { fifo, buf }
    }
    pub fn next_event(&mut self) -> std::io::Result<Stroke> {
        self.buf.clear();
        let _ = self.fifo.read_line(&mut self.buf)?;
        if let Some(prefix) = self.buf.chars().next() {
            let line = self.buf.get(1..self.buf.len() - 1).unwrap().to_owned();
            let stroke = match prefix {
                'B' => Stroke::BeginChain(line),
                'E' => Stroke::EndChain(line),
                'T' => Stroke::Timeout(line),
                'H' => Stroke::Hotkey(line),
                'C' => Stroke::Command(line),
                _ => Stroke::EOF,
            };
            return Ok(stroke);
        }
        Ok(Stroke::EOF)
    }
}

impl Iterator for FifoReader {
    type Item = Stroke;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_event() {
            Ok(Stroke::EOF) => None,
            Ok(x) => Some(x),
            _ => None,
        }
    }
}
