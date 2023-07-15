#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::ffi::CString;

trait Rustable<T2> {
    fn to_rust(self) -> T2;
}

type xcb_keysym_t = u32;
type xcb_button_t = u8;

pub fn get_config(file: Option<String>) -> Config {
    unsafe {
        let c_file = CString::new(file.unwrap_or(String::from(""))).unwrap();
        init_globals(c_file.into_raw());
        hotkeys_head.to_rust()
    }
}

#[repr(C)]
struct cycle_t {
    period: ::std::os::raw::c_int,
    delay: ::std::os::raw::c_int,
}

#[derive(Debug, Clone)]
pub struct Cycle {
    pub period: i32,
    pub delay: i32,
}

impl Rustable<Option<Cycle>> for *mut cycle_t {
    fn to_rust(self) -> Option<Cycle> {
        match self.is_null() {
            true => None,
            false => Some({
                let s = unsafe { &*self };
                Cycle {
                    period: s.period,
                    delay: s.delay,
                }
            }),
        }
    }
}

#[repr(C)]
struct chord_t {
    repr: [u8; 256usize],
    keysym: xcb_keysym_t,
    button: xcb_button_t,
    modfield: u16,
    event_type: u8,
    replay_event: bool,
    lock_chain: bool,
    next: *mut chord_t,
    more: *mut chord_t,
}

#[derive(Debug, Clone)]
pub struct Chord {
    pub repr: String,
    pub keysym: u32,
    pub button: u8,
    pub modfield: u16,
    pub event_type: u8,
    pub replay_event: bool,
    pub lock_chain: bool,
    // TODO: Figur eout what this is used for
    pub more: Option<Vec<Chord>>,
}

impl Rustable<Vec<Chord>> for *mut chord_t {
    fn to_rust(self) -> Vec<Chord> {
        let mut chord = self;
        let mut result = vec![];
        while !chord.is_null() {
            let ch = unsafe { &*chord };

            let newChord = Chord {
                repr: ch.repr.convert(),
                keysym: ch.keysym,
                button: ch.button,
                modfield: ch.modfield,
                event_type: ch.event_type,
                replay_event: ch.replay_event,
                lock_chain: ch.lock_chain,
                more: match ch.more.is_null() {
                    true => None,
                    false => Some(ch.more.to_rust()),
                },
            };
            result.push(newChord);
            chord = ch.next;
        }
        result
    }
}

#[repr(C)]
struct chain_t {
    head: *mut chord_t,
    tail: *mut chord_t,
    state: *mut chord_t,
}

impl Rustable<Vec<Chord>> for *mut chain_t {
    fn to_rust(self) -> Vec<Chord> {
        unsafe { (*self).head.to_rust() }
    }
}

#[repr(C)]
struct hotkey_t {
    chain: *mut chain_t,
    command: [u8; 512usize],
    sync: bool,
    cycle: *mut cycle_t,
    next: *mut hotkey_t,
    prev: *mut hotkey_t,
}

#[derive(Debug, Clone)]
pub struct Hotkey {
    pub chain: Vec<Chord>,
    pub command: String,
    pub sync: bool,
    pub cycle: Option<Cycle>,
}

pub type Config = Vec<Hotkey>;

impl Rustable<Config> for *mut hotkey_t {
    fn to_rust(self) -> Config {
        let mut result = vec![];
        unsafe {
            let mut head = self;
            while !head.is_null() {
                let h = &*head;
                result.push(Hotkey {
                    chain: h.chain.to_rust(),
                    command: h.command.convert(),
                    sync: h.sync,
                    cycle: h.cycle.to_rust(),
                });
                head = h.next;
            }
        }
        result
    }
}

trait NullCheckable {
    fn not_null(&self) -> bool;
    fn ok(&self) -> Option<&Self>;
}

impl<T> NullCheckable for T {
    fn not_null(&self) -> bool {
        !(self as *const T).is_null()
    }

    fn ok(&self) -> Option<&Self> {
        match self.not_null() {
            true => Some(self),
            false => None,
        }
    }
}

pub trait Stringable {
    fn convert(&self) -> String;
}
impl Stringable for [u8] {
    fn convert(&self) -> String {
        conv(self)
    }
}

fn conv(u: &[u8]) -> String {
    let slice: Vec<u8> = u.iter().copied().take_while(|c| *c != 0).collect();
    let s = String::from_utf8_lossy(&slice);
    s.to_string()
}

extern "C" {
    pub fn init_globals(cfg: *mut ::std::os::raw::c_char);
    static mut hotkeys_head: *mut hotkey_t;
    // pub static mut hotkeys_tail: *mut hotkey_t;
}
