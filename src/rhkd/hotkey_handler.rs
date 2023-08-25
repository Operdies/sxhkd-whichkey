use crate::parser::config::AddBindingError;
use crate::parser::Hotkey;
use crate::rhkc::ipc::{BindCommand, SubscribeEventMask, UnbindCommand};
use std::cell::RefCell;
use std::io::Write;

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
    subscribers: RefCell<Vec<Client>>,
}

struct Client {
    client: UnixStream,
    event_mask: Vec<SubscribeEventMask>,
}

#[derive(Clone)]
struct ChainItem {
    key: Key,
    locking: bool,
}

impl HotkeyHandler {
    pub fn toggle_grab(&mut self) -> Result<()> {
        if self.grab {
            self.ungrab_all()?;
            self.grab = false;
        } else {
            self.grab_index_0()?;
            self.grab = true;
        }
        Ok(())
    }
    pub fn reload(&mut self) -> Result<()> {
        match self.config.reload() {
            Ok(new) => {
                self.config = new;
                self.ungrab_all()?;
                self.grab_index_0()?;
                self.publish(&IpcMessage::ConfigReloaded);
            }
            Err(e) => self.publish(&IpcMessage::Error(
                format!("Config reload failed: {:?}", e).into(),
            )),
        }
        Ok(())
    }

    pub fn publish(&self, message: &IpcMessage) {
        fn is_interested(mask: &[SubscribeEventMask], message: &IpcMessage) -> bool {
            use SubscribeEventMask::*;
            if mask.contains(&All) {
                return true;
            }
            match message {
                IpcMessage::Notify(_) => mask.contains(&Notifications),
                IpcMessage::ConfigReloaded => mask.contains(&Reload),
                IpcMessage::BeginChain | IpcMessage::EndChain => mask.contains(&Chain),
                IpcMessage::Timeout => mask.contains(&Timeout),
                IpcMessage::Hotkey(_) => mask.contains(&Hotkey),
                IpcMessage::Command(_) => mask.contains(&Command),
                IpcMessage::Error(_) => mask.contains(&Errors),
                IpcMessage::BindingRemoved(_) | IpcMessage::BindingAdded(_) => {
                    mask.contains(&Change)
                }
            }
        }
        println!("PUBLISH {:?}", message);
        if let Some(ref fifo) = self.fifo {
            if let Err(e) = fifo.write_message(message) {
                eprintln!("Failed to write to fifo: {}", e);
            }
        }

        let msg = once_cell::sync::Lazy::new(|| {
            let mut bytes = match message {
                IpcMessage::BindingRemoved(r) => IpcCommand::Unbind(r.clone()).into(),
                IpcMessage::BindingAdded(a) => IpcCommand::Bind(a.clone()).into(),
                _ => {
                    let msg = message.to_string();
                    msg.bytes().collect::<Vec<u8>>()
                }
            };
            bytes.push(b'\n');
            bytes
        });

        self.subscribers.borrow_mut().retain_mut(|s| {
            if is_interested(&s.event_mask, message) {
                if let Err(e) = s.client.write_all(&msg) {
                    println!("Dropping slow subscriber: {}", e);
                    return false;
                }
            }
            true
        });
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
        self.ungrab_all()?;
        self.chain.clear();
        self.grab_index_0()?;
        self.publish(&IpcMessage::EndChain);
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

    fn hotkey_matches(hk: &Hotkey, chain: &[ChainItem]) -> bool {
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
            if Self::hotkey_matches(hk, chain) {
                result.push(hk.clone());
            }
        }
        result.into_iter().collect()
    }

    fn align_locks(&mut self, hotkeys: &[Hotkey]) {
        for (i, item) in self.chain.iter_mut().enumerate() {
            item.locking = hotkeys.iter().any(|hk| hk.chain[i].is_locking());
        }
    }

    fn last_lock_index(&self) -> Option<usize> {
        self.chain.iter().rposition(|i| i.locking)
    }
    fn chain_locked(&self) -> bool {
        self.last_lock_index().is_some()
    }

    fn publish_hotkey(&self, chain: &Hotkey) -> Result<()> {
        let mut hotkey_string = String::new();
        for item in &chain.chain[0..self.chain.len() - 1] {
            hotkey_string.push_str(&item.repr);
            hotkey_string.push_str(if item.is_locking() { " : " } else { " ; " });
        }
        let last = &chain.chain[self.chain.len() - 1];
        hotkey_string.push_str(&last.repr);
        self.publish(&IpcMessage::Hotkey(hotkey_string.into()));
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
        self.align_locks(&matching);

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
        if terminals.len() > 1 && terminals[0].cycle.is_none() {
            self.report_error(format!(
                "The sequence matched {} hotkeys, but only one will be triggered: {}",
                terminals.len(),
                terminals[0].chain_repr()
            ));
        }
        if let Some(hotkey) = terminals.get(0) {
            self.publish(&IpcMessage::Command(hotkey.command.clone()));
            match self.executor.run(hotkey) {
                Ok(_) => {}
                Err(e) => {
                    self.report_error(format!("Error running command {}: {}", hotkey.command, e))
                }
            }
            if hotkey.cycle.is_some() {
                if let Err(e) = self.config.cycle_hotkey(hotkey) {
                    self.report_error(format!("Error cycling hotkey: {}", e));
                }
            }
            self.pop_non_locking();
            if self.chain.is_empty() && chained {
                self.end_chain()?;
            }
        }

        self.update_grabset();

        if !self.chain.is_empty() && !chained {
            self.publish(&IpcMessage::BeginChain);
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
        self.publish(&IpcMessage::Timeout);
        self.end_chain()?;
        Ok(())
    }

    pub fn cleanup(&mut self) -> Result<()> {
        self.sync()?;
        self.ungrab_all()?;
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
            subscribers: RefCell::new(vec![]),
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

        self.ungrab_all()?;
        self.grab_index_0()?;

        Ok(())
    }

    fn grab_abort(&self) {
        keyboard::kbd()
            .grab_many(
                &self
                    .abort
                    .keycodes
                    .iter()
                    .copied()
                    .map(|k| (k, ModMask::from_bits_truncate(0)))
                    .collect::<Vec<_>>(),
            )
            .iter()
            .enumerate()
            .for_each(|(i, e)| {
                if let Err(e) = e {
                    eprintln!(
                        "Failed to grab abort keysym: {}: {}",
                        self.abort.keycodes[i], e
                    );
                }
            });
    }

    fn report_error(&self, error: String) {
        eprintln!("{}", error);
        self.publish(&IpcMessage::Error(error.into()));
    }

    fn grab_index(hotkeys: &[Hotkey], index: usize) {
        let kbd = keyboard::kbd();
        let mut chain_lookup = vec![];

        // Generate a vector of everything we want to grab so it can be used in a batching
        // operation. I measured this to be ~15 times faster than doing every request sequentially
        // TODO: Update cycle implementation in hotkey struct to avoid attempting to grab each
        // cycle
        let grab_set: Vec<_> = hotkeys
            .iter()
            .flat_map(|a| {
                let Some(chain) = a.chain.get(index) else {
                    return vec![];
                };
                kbd.get_keycodes(chain.keysym)
                    .unwrap_or(vec![])
                    .iter()
                    .copied()
                    .map(|k| {
                        chain_lookup.push(chain);
                        (k, xcb::x::ModMask::from(chain.modfield))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        kbd.grab_many(&grab_set)
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| Some((i, e.err()?)))
            .for_each(|(i, e)| match e {
                xcb::ProtocolError::X(ref a, _) => match a {
                    xcb::x::Error::Access(_) => {
                        eprintln!(
                            "'{}' could not be grabbed. Is it grabbed by another program?",
                            chain_lookup[i].repr
                        );
                    }
                    _ => {
                        eprintln!("Unhandled error during grab: {}", e);
                    }
                },
            });
    }

    fn grab_index_0(&mut self) -> Result<()> {
        Self::grab_index(self.config.get_hotkeys(), 0);
        self.grab = true;
        Ok(())
    }
    fn ungrab_all(&mut self) -> Result<()> {
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

    pub fn add_subscriber(&mut self, client: UnixStream, event_mask: Vec<SubscribeEventMask>) {
        self.subscribers
            .borrow_mut()
            .push(Client { client, event_mask })
    }

    /// This updates the grab set to exactly the set of currently valid keys
    fn update_grabset(&mut self) {
        let _ = self.ungrab_all();
        if !self.chain_locked() {
            Self::grab_index(self.config.get_hotkeys(), 0);
        }
        if !self.chain.is_empty() {
            self.grab_abort();
            let matching = self.find_hotkey(&self.chain);
            Self::grab_index(&matching, self.chain.len());
        }
        self.grab = true;
    }

    pub fn delete_bindings(&mut self, mut client: UnixStream, unbind: UnbindCommand) {
        match self.config.delete_bindings(&unbind) {
            Ok(removed) => {
                if !removed.is_empty() {
                    self.update_grabset();
                }
                self.publish(&IpcMessage::BindingRemoved(unbind.clone()));
                for r in removed.into_iter() {
                    let _ = write!(client, "# REMOVE\n{}", r);
                }
            }
            Err(e) => {
                let _ = write!(client, "Failed to parse input: {}", e);
            }
        }
    }

    pub fn clone_hotkeys(&self) -> Vec<Hotkey> {
        self.config.get_hotkeys().clone()
    }

    pub fn add_bindings(&mut self, mut client: UnixStream, bind: BindCommand) {
        match self.config.add_bindings(&bind) {
            Ok(result) => {
                if !result.added.is_empty() || !result.removed.is_empty() {
                    self.publish(&IpcMessage::BindingAdded(bind.clone()));
                    self.update_grabset();
                }
                for added in result.added.into_iter() {
                    let _ = writeln!(client, "# ADD\n{}", &added);
                }
                for removed in result.removed.into_iter() {
                    let _ = writeln!(client, "# REMOVE\n{}", &removed);
                }
                for error in result.errors {
                    match error {
                        AddBindingError::WouldInterfere { current, new } => {
                            let _ = writeln!(
                                client,
                                "Hotkey '{}' not added because it would interfere with '{}'.",
                                current, new
                            );
                        }
                    }
                }
            }
            Err(e) => {
                let _ = writeln!(client, "{}", e);
            }
        }
    }
}
