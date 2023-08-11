#![allow(unused)]

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

#[derive(Debug, Clone, PartialEq, Default, Copy)]
pub enum ChainMode {
    // Lock the chain in the current state until ABORT_KEYSYM is triggered
    Locking,
    // Release the chain when the state is advanced
    #[default]
    Once,
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chord {
    pub repr: String,
    pub keysym: u32,
    pub button: u8,
    pub modfield: ModMask,
    pub event_type: KeyMode,
    pub replay_event: ReplayMode,
    pub lock_chain: ChainMode,
    // TODO: Figure out what this is used for
    pub more: Option<Vec<Chord>>,
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
pub type Hotkeys = Vec<Hotkey>;
