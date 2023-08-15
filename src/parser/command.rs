use anyhow::Result;
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
    Reload,
    EOF,
}

impl Default for FifoReader {
    fn default() -> Self {
        let args = crate::CliArguments::parse();
        if let Some(fifo) = args.status_fifo {
            let file = File::open(fifo).unwrap();
            let reader = BufReader::new(file);
            return Self::new(reader);
        }
        todo!()
    }
}

impl FifoReader {
    fn next_event(&mut self) -> Result<Stroke> {
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
                'R' => Stroke::Reload,
                _ => Stroke::EOF,
            };
            return Ok(stroke);
        }
        Ok(Stroke::EOF)
    }
    pub fn new(fifo: BufReader<File>) -> Self {
        let buf = String::new();
        FifoReader { fifo, buf }
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
