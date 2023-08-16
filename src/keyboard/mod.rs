use std::collections::HashMap;
use xcb::x;
pub use xcb::x::ModMask;

mod keysyms;
use x::Allow::*;

use anyhow::{Context, Result};

lazy_static! {
    static ref KEYBOARD: Keyboard = Keyboard::new().unwrap();
}

pub struct Keyboard {
    root: xcb::x::Window,
    conn: xcb::Connection,
    keycode_lookup: HashMap<u32, Vec<u8>>,
    mods: x::GetModifierMappingReply,
}

impl Keyboard {
    fn allow_events(&self, mode: x::Allow) -> xcb::Result<()> {
        self.conn.send_and_check_request(&x::AllowEvents {
            mode,
            time: x::CURRENT_TIME,
        })?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn poll_event(&self) -> Result<Option<xcb::Event>, xcb::Error> {
        self.conn.poll_for_event()
    }
    pub fn next_event(&self) -> xcb::Result<xcb::Event> {
        self.conn.wait_for_event()
    }

    pub fn sync_pointer(&self) -> xcb::Result<()> {
        self.allow_events(SyncPointer)
    }
    pub fn sync_keyboard(&self) -> xcb::Result<()> {
        self.allow_events(SyncKeyboard)
    }
    pub fn replay_keyboard(&self) -> xcb::Result<()> {
        self.allow_events(ReplayKeyboard)
    }
    pub fn replay_pointer(&self) -> xcb::Result<()> {
        self.allow_events(ReplayPointer)
    }

    pub fn keysym_from_keycode(&self, keycode: u8) -> Option<&'static str> {
        self.keycode_lookup
            .iter()
            .find(|i| i.1.contains(&keycode))
            .map(|i| i.0)
            .copied()
            .map(keysyms::keycode_to_string)
            .unwrap_or(None)
    }

    fn modfield_from_keycode(&self, keycode: u8) -> u32 {
        if keycode == 0 {
            return 0;
        }
        let mut modfield = 0;
        let mod_keycodes = self.mods.keycodes();
        let num_mods = self.mods.length() as usize;
        let keycodes_per_modifier = mod_keycodes.len() / num_mods;

        for i in 0..num_mods {
            for j in 0..keycodes_per_modifier {
                let kc = mod_keycodes[i * keycodes_per_modifier + j];
                if kc == keycode {
                    modfield |= 1 << i;
                }
            }
        }
        modfield
    }
    fn modfield_from_keysym(&self, keysym: &str) -> u32 {
        let mut modfield = 0;
        if let Ok(keycodes) = self.get_keycodes_from_string(keysym) {
            for keycode in keycodes {
                modfield |= self.modfield_from_keycode(keycode);
            }
        }
        modfield
    }

    fn modfield_from_mods(&self, modifiers: &[&str]) -> anyhow::Result<ModMask> {
        let mut modmask = 0;
        for modifier in modifiers {
            modmask |= self
                .modifier_from_string(modifier)
                .with_context(|| {
                    let similar = if let Some(similar) = keysyms::get_closest_modifier(modifier) {
                        format!(", but '{}' is similar", similar)
                    } else {
                        "".into()
                    };
                    format!("Unknown modifier '{}'{}.", modifier, similar)
                })?
                .bits();
        }
        ModMask::from_bits(modmask).context(format!(
            "Failed to create modmask from modfield 0x{:X}",
            modmask
        ))
    }

    fn modifier_from_string(&self, s: &str) -> Result<ModMask> {
        let opt = match s {
            "shift" => ModMask::SHIFT.into(),
            "control" | "ctrl" => ModMask::CONTROL.into(),
            "alt" => ModMask::from_bits(
                self.modfield_from_keysym("Alt_L") | self.modfield_from_keysym("Alt_R"),
            ),
            "super" => ModMask::from_bits(
                self.modfield_from_keysym("Super_L") | self.modfield_from_keysym("Super_R"),
            ),
            "hyper" => ModMask::from_bits(
                self.modfield_from_keysym("Hyper_L") | self.modfield_from_keysym("Hyper_R"),
            ),
            "meta" => ModMask::from_bits(
                self.modfield_from_keysym("Meta_L") | self.modfield_from_keysym("Meta_R"),
            ),
            "mode_switch" => ModMask::from_bits(self.modfield_from_keysym("Mode_switch")),
            "mod1" => ModMask::N1.into(),
            "mod2" => ModMask::N2.into(),
            "mod3" => ModMask::N3.into(),
            "mod4" => ModMask::N4.into(),
            "mod5" => ModMask::N5.into(),
            "lock" => ModMask::LOCK.into(),
            "any" => ModMask::ANY.into(),
            _ => Err(KeyError::UnknownModifier(
                s.to_string(),
                keysyms::get_closest_modifier(s).map(|t| t.to_string()),
            ))?,
        };
        let mask = opt.with_context(|| format!("Failed to create modmask from '{}'", s))?;
        Ok(mask)
    }

    pub fn get_keycodes(&self, o: u32) -> Option<Vec<u8>> {
        self.keycode_lookup.get(&o).cloned()
    }

    pub fn get_keycodes_from_string(&self, s: &str) -> Result<Vec<u8>> {
        keysyms::symbol_from_string(s)
            .and_then(|o| self.get_keycodes(o))
            .with_context(|| {
                let similar = if let Some(similar) = keysyms::get_closest_key(s) {
                    format!(", but '{}' is similar", similar)
                } else {
                    "".into()
                };
                format!("Unknown key '{}'{}.", s, similar)
            })
    }

    pub fn grab(&self, key: u8, modifiers: xcb::x::ModMask) -> Result<()> {
        let request = xcb::x::GrabKey {
            owner_events: true,
            grab_window: self.root,
            modifiers,
            key,
            pointer_mode: xcb::x::GrabMode::Async,
            keyboard_mode: xcb::x::GrabMode::Sync,
        };
        self.conn
            .send_and_check_request(&request)
            .context("Key is already grabbed")
    }

    pub fn ungrab(&self, key: u8, modifiers: xcb::x::ModMask) -> xcb::Result<()> {
        let request = xcb::x::UngrabKey {
            key,
            grab_window: self.root,
            modifiers,
        };
        self.conn.send_and_check_request(&request)?;
        Ok(())
    }
    pub fn ungrab_all(&self) -> xcb::Result<()> {
        self.ungrab(xcb::x::GRAB_ANY, xcb::x::ModMask::ANY)?;
        Ok(())
    }
}

impl Keyboard {
    pub fn connection(&self) -> &xcb::Connection {
        &self.conn
    }
    pub fn new() -> anyhow::Result<Keyboard> {
        let (conn, screen_num) = xcb::Connection::connect(None)?;
        let setup = conn.get_setup();
        let root = setup.roots().nth(screen_num as usize).unwrap().root();

        let min_kc = setup.min_keycode();
        let max_kc = setup.max_keycode();

        let mapping = x::GetKeyboardMapping {
            first_keycode: min_kc,
            count: max_kc - min_kc,
        };

        let mapping = conn.send_request(&mapping);
        let keyboard_mapping = conn.wait_for_reply(mapping)?;

        let keysyms = keyboard_mapping.keysyms();
        let n_keysyms = keyboard_mapping.keysyms().len();
        let kpk = keyboard_mapping.keysyms_per_keycode() as usize;

        let n_keycodes = n_keysyms / kpk;

        let mut keycode_lookup: HashMap<u32, Vec<u8>> = Default::default();
        for keycode_idx in 0..n_keycodes {
            let keycode = keycode_idx + (min_kc as usize);
            // print!("0x{:<3X} {}", keycode, keycode);
            for keysym_idx in 0..kpk {
                let sym = keysyms[keycode_idx * kpk + keysym_idx];
                if sym != 0 {
                    // print!(" | 0x{:<8X}", sym);
                    if let Some(v) = keycode_lookup.get_mut(&sym) {
                        let keycode = keycode as u8;
                        if !v.iter().any(|e| e.eq(&keycode)) {
                            v.push(keycode);
                        }
                    } else {
                        let v: Vec<u8> = vec![keycode as u8];
                        keycode_lookup.insert(sym, v);
                    }
                }
            }
            // println!();
        }

        let mods = x::GetModifierMapping {};
        let mods = conn.send_request(&mods);
        let mods = conn.wait_for_reply(mods)?;

        Ok(Keyboard {
            conn,
            root,
            keycode_lookup,
            mods,
        })
    }
}

pub fn kbd() -> &'static Keyboard {
    &KEYBOARD
}

#[derive(Debug)]
pub enum KeyError {
    UnknownKey(String, Option<String>),
    UnknownModifier(String, Option<String>),
}

impl std::error::Error for KeyError {}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn txt(thing: &str, key: &str, similar: Option<&str>) -> String {
            let exp = if let Some(s) = similar {
                format!(", but '{}' is similar", s)
            } else {
                "".into()
            };
            format!("Unrecognized {} '{}'{}.", thing, key, exp)
        }

        match self {
            KeyError::UnknownKey(key, o) => f.write_str(&txt("key", key, o.as_deref())),
            KeyError::UnknownModifier(key, o) => f.write_str(&txt("modifier", key, o.as_deref())),
        }
    }
}

pub fn symbol_from_string(s: &str) -> Result<u32> {
    if let Some(key) = keysyms::symbol_from_string(s) {
        Ok(key)
    } else if let Some(similar) = keysyms::get_closest_key(s) {
        Err(KeyError::UnknownKey(s.into(), Some(similar.into())))?
    } else {
        Err(KeyError::UnknownKey(s.into(), None))?
    }
}

pub fn modfield_from_mods(modifiers: &[&str]) -> anyhow::Result<ModMask> {
    KEYBOARD.modfield_from_mods(modifiers)
}
pub fn modfield_from_keysym(keysym: &str) -> u32 {
    KEYBOARD.modfield_from_keysym(keysym)
}

