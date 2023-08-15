use crate::parser::subscribe::Subscriber;
use crate::parser::{Cycle, Hotkey};

use super::fifo::{Fifo, FifoError};
use super::*;

use super::executor::Executor;
use super::keyboard;
use xcb::x::ModMask;

use anyhow::{bail, Result};
use std::collections::HashMap;

#[derive(Default)]
struct AbortKeysym {
    keycodes: Vec<u8>,
}

pub struct HotkeyHandler {
    cli: CliArguments,
    config: Config,
    chain: Vec<ChainItem>,
    abort: AbortKeysym,
    grab: bool,
    fifo: Option<Fifo>,
    executor: Executor,
    subscribers: Vec<IpcRequestObject>,
}

#[derive(Clone)]
struct ChainItem {
    key: Key,
    locking: bool,
}

impl HotkeyHandler {
    pub fn dump_config(&self) -> String {
        self.config.dump(Default::default())
    }
    pub fn toggle_grab(&mut self) -> Result<()> {
        if self.grab {
            self.ungrab()?;
            self.grab = false;
        } else {
            self.grab()?;
            self.grab = true;
        }
        Ok(())
    }
    pub fn reload(&mut self) -> Result<()> {
        match self.config.reload() {
            Ok(new) => {
                self.config = new;
                self.grab()?;
                self.publish(&IpcMessage::ConfigReloaded)?;
            }
            Err(e) => self.publish(&IpcMessage::Notify(format!(
                "Config reload failed: {:?}",
                e
            )))?,
        }
        Ok(())
    }

    pub fn publish(&mut self, message: &IpcMessage) -> Result<()> {
        println!("PUBLISH {:?}", message);
        if let Some(ref mut fifo) = self.fifo {
            fifo.write_message(message)?;
        }
        for i in (0..self.subscribers.len()).rev() {
            let sub = &mut self.subscribers[i];
            if sub.send(message).is_err() {
                self.subscribers.swap_remove(i);
            }
        }
        Ok(())
    }

    fn is_abort(&self, key: &Key) -> bool {
        match key {
            Key {
                modfield: 0,
                is_press: true,
                symbol: s,
            } => self.abort.keycodes.contains(s),
            _ => false,
        }
    }

    fn end_chain(&mut self) -> Result<()> {
        if !self.chain.is_empty() {
            self.chain.clear();
            self.publish(&IpcMessage::EndChain)?;
        }
        Ok(())
    }

    fn cancel_timeout(&self) {
        nix::unistd::alarm::cancel();
    }
    fn schedule_timeout(&self) {
        nix::unistd::alarm::set(self.cli.timeout);
    }

    fn hotkey_matches(&self, hk: &Hotkey, chain: &[ChainItem]) -> bool {
        for (i, c) in chain.iter().enumerate() {
            match hk.chain.get(i) {
                Some(v) if &c.key == v => {
                    continue;
                }
                _ => {
                    return false;
                }
            }
        }
        true
    }

    fn find_hotkey(&self, chain: &[ChainItem]) -> Vec<Hotkey> {
        let mut result = vec![];
        for hk in self.config.get_hotkeys() {
            if self.hotkey_matches(hk, chain) {
                result.push(hk);
            }
        }
        result.into_iter().cloned().collect()
    }

    fn align_locks(&mut self, hotkey: &Hotkey) {
        for (i, item) in self.chain.iter_mut().enumerate() {
            item.locking = hotkey.chain[i].is_locking();
        }
    }

    fn last_lock_index(&self) -> Option<usize> {
        self.chain.iter().rposition(|i| i.locking)
    }
    fn chain_locked(&self) -> bool {
        self.last_lock_index().is_some()
    }

    pub fn handle_key(&mut self, key: Key) -> Result<()> {
        if key.is_press {
            self.cancel_timeout();
        }

        let mut chained = !self.chain.is_empty();
        let locked = self.chain_locked();

        if self.is_abort(&key) {
            self.end_chain()?;
            if chained || locked {
                self.sync()?;
            } else {
                self.replay()?;
            }
            return Ok(());
        }

        // Push the current key onto the stack
        self.chain.push(ChainItem {
            key,
            locking: false,
        });

        // Find all hotkeys matching the current chain
        let matching = self.find_hotkey(&self.chain);
        // If there are no matches for the current chain, and it isn't locked, check if another binding starts with this key.
        if chained && !locked && matching.is_empty() {
            // If the current chain has no continuations,
            let new_chain = ChainItem {
                key,
                locking: false,
            };
            let matching = self.find_hotkey(&[new_chain.clone()]);
            if !matching.is_empty() {
                self.end_chain()?;
                chained = false;
                self.chain.push(new_chain);
            }
        }

        if matching.is_empty() {
            self.chain.pop();
            self.replay()?;
            return Ok(());
        }

        let mut replay = false;
        for hk in matching.iter() {
            let this_chord = &hk.chain[self.chain.len() - 1];
            if this_chord.replay_event.is_replay() {
                replay = true;
                break;
            }
        }

        // We should be nice X citizens and replay / sync as early as possible
        if replay {
            self.replay()?;
        } else {
            self.sync()?;
        }

        let terminals: Vec<_> = matching
            .iter()
            .filter(|t| t.chain.len() == self.chain.len())
            .collect();
        let terminal = terminals.first();
        match terminal {
            Some(hotkey) => {
                self.publish(&IpcMessage::Command(hotkey.command.clone()))?;
                match self.executor.run(hotkey) {
                    Ok(_) => {}
                    Err(e) => eprintln!("Error running command {}: {}", hotkey.command, e),
                }
                if hotkey.cycle.is_some() {
                    if let Err(e) = self.config.cycle_hotkey(hotkey) {}
                }
            }
            None => {
                if !chained {
                    self.publish(&IpcMessage::BeginChain)?;
                }
            }
        }

        // Update the current chain to match the lock of whatever is currently matching.
        self.align_locks(&matching[0]);
        if !self.chain.iter().any(|c| c.locking) {
            // if matching.iter().any(|m| m.chain.len() > self.chain.len()) {
            self.schedule_timeout();
            // }
        }

        if let Some(chain) = matching.first() {
            let mut hotkey_string = String::new();
            for item in &chain.chain[0..self.chain.len() - 1] {
                hotkey_string.push_str(&item.repr);
                hotkey_string.push_str(if item.is_locking() { " : " } else { " ; " });
            }
            let last = &chain.chain[self.chain.len() - 1];
            hotkey_string.push_str(&last.repr);
            self.publish(&IpcMessage::Hotkey(hotkey_string))?;
        }

        // If this was a terminal command, we should pop the current chain until we encounter a lock
        if terminal.is_some() {
            for idx in (0..self.chain.len()).rev() {
                if self.chain[idx].locking {
                    break;
                } else {
                    self.chain.pop();
                }
            }
            if chained && self.chain.is_empty() {
                self.publish(&IpcMessage::EndChain)?;
            }
        }
        Ok(())
    }

    pub fn timeout(&mut self) -> Result<()> {
        self.publish(&IpcMessage::Timeout)?;
        self.end_chain()?;
        Ok(())
    }

    pub fn cleanup(&mut self) -> Result<()> {
        self.sync()?;
        self.ungrab()?;
        Ok(())
    }

    pub fn new(cli: CliArguments, config: Config) -> Self {
        let redir_file = cli.redir_file.clone();
        Self {
            cli,
            config,
            chain: vec![],
            abort: Default::default(),
            grab: false,
            fifo: None,
            executor: Executor::new(redir_file),
            subscribers: vec![],
        }
    }

    pub fn setup(&mut self) -> Result<()> {
        match self.make_fifo() {
            Ok(fifo) => self.fifo = Some(fifo),
            Err(FifoError::FifoNotConfigured) => {}
            Err(e) => Err(e)?,
        }
        self.grab()?;

        let escape_keysym = self.cli.abort_keysym.as_deref().unwrap_or("Escape");
        let keysym = keyboard::symbol_from_string(escape_keysym)?;
        let escape_symbols = keyboard::kbd().get_keycodes(keysym);
        let Some(keycodes) = escape_symbols else { bail!(format!("No keycode for specified abort symbol '{}'", escape_keysym))};

        for key in keycodes.iter() {
            self.grab_key(*key, ModMask::from_bits_truncate(0))?;
        }

        self.abort = AbortKeysym { keycodes };

        Ok(())
    }

    fn grab_key(&self, keycode: u8, modfield: ModMask) -> Result<()> {
        keyboard::kbd().grab(keycode, modfield)?;
        Ok(())
    }

    fn grab(&mut self) -> Result<()> {
        self.ungrab()?;

        let kbd = keyboard::kbd();

        self.abort.keycodes.iter().for_each(|keycode| {
            if let Err(e) = kbd.grab(*keycode, ModMask::from_bits_truncate(0)) {
                println!("Failed to grab Escape {}: {}", keycode, e);
            }
        });

        let mut repr_map: HashMap<String, bool> = Default::default();
        'outer: for hotkey in self.config.get_hotkeys() {
            for chain in &hotkey.chain {
                if let Some(keycodes) = kbd.get_keycodes(chain.keysym) {
                    for keycode in keycodes {
                        if let Err(e) = self.grab_key(keycode, chain.modfield.into()) {
                            if repr_map.insert(chain.repr.clone(), true).is_none() {
                                eprintln!("Error grabbing '{}': {}", chain.repr, e);
                            }
                        }
                    }
                } else {
                    eprintln!(
                    "Failed to get keycode for symbol in chain: {:?}. Skipping this hotkey. ({:?})",
                    chain, hotkey
                );
                    continue 'outer;
                }
            }
        }
        self.grab = true;
        Ok(())
    }
    fn ungrab(&mut self) -> Result<()> {
        keyboard::kbd().ungrab_all()?;
        self.grab = false;
        Ok(())
    }
    fn make_fifo(&self) -> Result<Fifo, FifoError> {
        let Some(ref status_fifo) = self.cli.status_fifo else { return Err(FifoError::FifoNotConfigured); };
        Fifo::new(status_fifo)
    }
    fn replay(&self) -> Result<()> {
        keyboard::kbd().replay_keyboard()?;
        Ok(())
    }
    fn sync(&self) -> Result<()> {
        keyboard::kbd().sync_keyboard()?;
        Ok(())
    }

    pub fn add_subscriber(&mut self, request: IpcRequestObject) {
        self.subscribers.push(request);
    }
}
