use crate::parser::subscribe::Subscriber;
use crate::parser::{Cycle, Hotkey};

use super::fifo::{Fifo, FifoError};
use super::*;

use super::executor::Executor;
use super::keyboard;
use xcb::x::ModMask;

use anyhow::{bail, Result};

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
                self.ungrab()?;
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
        self.ungrab()?;
        self.chain.clear();
        self.grab()?;
        self.publish(&IpcMessage::EndChain)?;
        Ok(())
    }

    fn cancel_timeout(&self) {
        nix::unistd::alarm::cancel();
    }
    fn schedule_timeout(&self) {
        if !self.chain.is_empty() && !self.chain_locked() {
            nix::unistd::alarm::set(self.cli.timeout);
        }
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
                result.push(hk.clone());
            }
        }
        result.into_iter().collect()
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

    fn publish_hotkey(&mut self, chain: &Hotkey) -> Result<()> {
        let mut hotkey_string = String::new();
        for item in &chain.chain[0..self.chain.len() - 1] {
            hotkey_string.push_str(&item.repr);
            hotkey_string.push_str(if item.is_locking() { " : " } else { " ; " });
        }
        let last = &chain.chain[self.chain.len() - 1];
        hotkey_string.push_str(&last.repr);
        self.publish(&IpcMessage::Hotkey(hotkey_string))?;
        Ok(())
    }

    pub fn handle_key(&mut self, key: Key) -> Result<()> {
        if key.is_press {
            self.cancel_timeout();
        }

        let mut chained = !self.chain.is_empty();
        let locked = self.chain_locked();

        // This makes it impossible to use ABORT_KEYSYM in a binding,
        // but that's a necessary compromise because it would otherwise
        // be possible to get stuck in a locking chain , e.g. super + a : ABORT_KEYSYM could never
        // terminate
        if chained && self.is_abort(&key) {
            self.end_chain()?;
            self.sync()?;
            return Ok(());
        }

        // Push the current key onto the stack
        self.chain.push(ChainItem {
            key,
            locking: false,
        });

        // Find all hotkeys matching the current chain
        let mut matching = self.find_hotkey(&self.chain);
        // If there are no matches for the current chain, and it isn't locked, check if another binding starts with this key.
        if chained && !locked && matching.is_empty() {
            let new_chain = ChainItem {
                key,
                locking: false,
            };
            matching = self.find_hotkey(&[new_chain.clone()]);
            // If we started a new chain, we should abort the previous chain
            if !matching.is_empty() {
                self.end_chain()?;
                chained = false;
                self.chain.push(new_chain);
            }
        }

        if matching.is_empty() {
            self.chain.pop();
            self.sync()?;
            self.schedule_timeout();
            return Ok(());
        }

        self.publish_hotkey(&matching[0])?;

        // Update the current chain to match the lock of whatever is currently matching.
        self.align_locks(&matching[0]);

        // We should replay this key if any matched chains has requested it
        let replay = matching
            .iter()
            .filter_map(|h| h.chain.get(self.chain.len() - 1))
            .any(|f| f.replay_event.is_replay());

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
        if terminals.len() > 1 {
            eprintln!(
                "The sequence matched {} hotkeys, but only one will be triggered:\n{}",
                terminals.len(),
                terminals[0]
            );
        }
        if let Some(hotkey) = terminals.get(0) {
            self.publish(&IpcMessage::Command(hotkey.command.clone()))?;
            match self.executor.run(hotkey) {
                Ok(_) => {}
                Err(e) => eprintln!("Error running command {}: {}", hotkey.command, e),
            }
            if hotkey.cycle.is_some() {
                if let Err(e) = self.config.cycle_hotkey(hotkey) {
                    eprintln!("Error cycling hotkey: {}", e);
                }
            }
            self.pop_non_locking();
            if self.chain.is_empty() && chained {
                self.end_chain()?;
            }
        }

        // update grab set
        let _ = self.ungrab();
        // If the chain isn't locked, we should keep index 0 grabbed
        if !self.chain_locked() {
            Self::grab_index(self.config.get_hotkeys(), 0);
        }
        if !self.chain.is_empty() {
            self.grab_abort();
            Self::grab_index(&matching, self.chain.len());
            if !chained {
                self.publish(&IpcMessage::BeginChain)?;
            }
        }

        self.schedule_timeout();

        Ok(())
    }

    fn pop_non_locking(&mut self) {
        // Keep popping the chain until a lock is encountered
        while let Some(back) = self.chain.pop() {
            if back.locking {
                self.chain.push(back);
                return;
            }
        }
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

        let escape_keysym = self.cli.abort_keysym.as_deref().unwrap_or("Escape");
        let keysym = keyboard::symbol_from_string(escape_keysym)?;
        let escape_symbols = keyboard::kbd().get_keycodes(keysym);
        let Some(keycodes) = escape_symbols else { bail!(format!("No keycode for specified abort symbol '{}'", escape_keysym))};

        self.abort = AbortKeysym { keycodes };

        self.ungrab()?;
        self.grab()?;

        Ok(())
    }

    fn grab_abort(&self) {
        self.abort.keycodes.iter().for_each(|keycode| {
            if let Err(e) = keyboard::kbd().grab(*keycode, ModMask::from_bits_truncate(0)) {
                println!("Failed to grab Escape {}: {}", keycode, e);
            }
        });
    }
    fn grab_chain(chain: &Chord) {
        let kbd = keyboard::kbd();
        if let Some(keycodes) = kbd.get_keycodes(chain.keysym) {
            for keycode in keycodes {
                if let Err(e) = kbd.grab(keycode, chain.modfield.into()) {
                    eprintln!("Error grabbing '{}': {}", chain.repr, e);
                }
            }
        }
    }

    fn grab_index(hotkeys: &[Hotkey], index: usize) {
        hotkeys
            .iter()
            .filter_map(|a| a.chain.get(index))
            .for_each(Self::grab_chain);
    }

    fn grab(&mut self) -> Result<()> {
        Self::grab_index(self.config.get_hotkeys(), 0);
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
