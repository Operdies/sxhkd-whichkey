#![allow(unused)]

use std::fmt::Display;

#[derive(Debug, Clone, PartialEq)]
pub struct Cycle {
    pub period: i32,
    pub delay: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ReplayMode {
    // Propagate the key event to clients by replaying it
    Replay,
    // Prevent the key event from propagating to clients
    #[default]
    Supress,
}

impl ReplayMode {
    /// Returns `true` if the replay mode is [`Replay`].
    ///
    /// [`Replay`]: ReplayMode::Replay
    #[must_use]
    pub fn is_replay(&self) -> bool {
        matches!(self, Self::Replay)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Copy)]
pub enum ChainMode {
    // Lock the chain in the current state until ABORT_KEYSYM is triggered
    Locking,
    // Release the chain when the state is advanced
    #[default]
    Once,
}

impl Chord {
    /// Returns `true` if the chain mode is [`Locking`].
    ///
    /// [`Locking`]: ChainMode::Locking
    #[must_use]
    pub fn is_locking(&self) -> bool {
        self.lock_chain.is_locking()
    }
}

impl Display for Chord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.repr)
    }
}

impl ChainMode {
    /// Returns `true` if the chain mode is [`Locking`].
    ///
    /// [`Locking`]: ChainMode::Locking
    #[must_use]
    pub fn is_locking(&self) -> bool {
        matches!(self, Self::Locking)
    }
}

// From xcb header files:
// #define XCB_KEY_PRESS 2
// #define XCB_KEY_RELEASE 3
const XCB_KEY_PRESS: u8 = 2;
const XCB_KEY_RELEASE: u8 = 3;
#[derive(Debug, Clone, PartialEq, Default)]
pub enum KeyMode {
    #[default]
    KeyPress,
    KeyRelease,
}

impl KeyMode {
    /// Returns `true` if the key mode is [`KeyPress`].
    ///
    /// [`KeyPress`]: KeyMode::KeyPress
    #[must_use]
    pub fn is_key_press(&self) -> bool {
        matches!(self, Self::KeyPress)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Copy)]
pub struct ModMask {
    mods: u32,
}

impl From<ModMask> for xcb::x::ModMask {
    fn from(val: ModMask) -> Self {
        xcb::x::ModMask::from_bits(val.mods).unwrap()
    }
}

impl From<u32> for ModMask {
    fn from(mods: u32) -> Self {
        Self { mods }
    }
}

impl From<ModMask> for u32 {
    fn from(value: ModMask) -> Self {
        value.mods
    }
}

impl ModMask {
    pub fn bits(&self) -> u32 {
        self.mods
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chord {
    pub repr: String,
    pub keysym: u32,
    pub button: u8,
    pub modfield: ModMask,
    pub event_type: KeyMode,
    pub replay_event: ReplayMode,
    pub lock_chain: ChainMode,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hotkey {
    pub chain: Vec<Chord>,
    pub command: String,
    pub sync: bool,
    pub cycle: Option<Cycle>,
    pub title: Option<String>,
    pub description: Option<String>,
}

impl Hotkey {
    pub fn description(&self) -> String {
        let matches: &[_] = &['\\', ' ', '\n'];
        self.description
            .clone()
            .unwrap_or_else(|| self.command.trim_matches(matches).into())
    }
}

impl Display for Hotkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref title) = self.title {
            f.write_str(&format!("# {}\n", title))?;
        }
        if let Some(ref description) = self.description {
            f.write_str(&format!("# {}\n", description))?;
        }
        for item in &self.chain {
            f.write_str(&item.repr)?;
            f.write_str(" ")?;
        }
        f.write_str("\n  ")?;
        f.write_str(&self.command)
    }
}
