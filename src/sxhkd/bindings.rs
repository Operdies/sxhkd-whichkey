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

#[derive(Debug)]
enum Token {
    Comment(String),
    Quoted(String),
    Word(String),
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Token::Comment(c) => c.to_string(),
            Token::Quoted(c) => format!("\"{}\"", c),
            Token::Word(c) => c.to_string(),
        };
        f.write_str(&s)?;
        Ok(())
    }
}

#[derive(Debug)]
struct ParsedComment {
    pairs: Vec<(String, String)>,
    remaining: String,
}

impl Hotkey {
    fn read_word(whitespace: char, it: &mut std::str::Chars<'_>) -> Option<String> {
        let quotes = ['\'', '"'];
        // Skip leading spaces
        let ch = it.find(|c| c.ne(&' '))?;
        if quotes.contains(&ch) {
            let quoted = Self::take_until_escaped(ch, it);
            if it.next().is_some_and(|c| c == whitespace) {
                return Some(quoted);
            } else {
                return None;
            }
        }
        let mut s = String::new();
        s.push(ch);
        while let Some(ch) = it.next() {
            if ch.eq(&'\\') {
                if let Some(c) = it.next() {
                    s.push(c);
                    continue;
                }
            } else if ch.eq(&whitespace) {
                return Some(s);
            } else {
                s.push(ch);
            }
        }
        None
    }

    fn read_pairs(s: &str) -> ParsedComment {
        let mut result = ParsedComment {
            pairs: vec![],
            remaining: Default::default(),
        };
        let mut it = s.chars();
        loop {
            let clone = it.clone();
            let word = Self::read_word(':', &mut it);
            let replacement = Self::read_word(' ', &mut it);
            if word.is_none() || replacement.is_none() {
                result.remaining = clone.collect();
                return result;
            }
            result.pairs.push((word.unwrap(), replacement.unwrap()));
        }
    }

    fn expand_comment(comment: String, tokens: Vec<Token>) -> String {
        println!("Expanding comment: {}", &comment);
        let mut pairs = Self::read_pairs(&comment);
        println!("Found pairs: {:?}", &pairs);
        for _ in 0..2 {
            for (i, v) in tokens.iter().enumerate() {
                let replacement = v.to_string();
                let quotes: &[_] = &['\'', '"'];
                let tr = replacement.trim_matches(quotes);
                let mut mapping: Option<String> = None;
                for (p, r) in pairs.pairs.iter() {
                    if tr.eq(p) {
                        mapping = Some(r.clone());
                        break;
                    }
                }
                if let Some(ref m) = mapping {
                    println!("Mapped {} to {}", replacement, m);
                } else {
                    println!("No mapping for {}", replacement);
                }
                let next = pairs
                    .remaining
                    .replace::<&str>(&format!("$({})", i), &mapping.unwrap_or(replacement));
                println!("Expanded: {} -> {}", pairs.remaining, next);
                pairs.remaining = next;
            }
        }
        pairs.remaining
    }
    fn describe(mut tokens: Vec<Token>) -> String {
        if let Some(Token::Comment(_)) = tokens.last() {
            let comment = tokens.pop().unwrap().to_string();
            Self::expand_comment(comment, tokens)
        } else {
            tokens
                .into_iter()
                .map(|t| t.to_string())
                .collect::<Vec<String>>()
                .join(" ")
        }
    }

    pub fn description(&self) -> String {
        let tokens = Self::tokenize(&self.command);
        Self::describe(tokens)
    }

    fn take_until_escaped(ch: char, it: &mut std::str::Chars<'_>) -> String {
        let mut s = String::new();
        while let Some(c) = it.next() {
            if c == '\\' {
                let _ = it.next(); // Skip next char
            }
            if ch == c {
                break;
            }
            s.push(c);
        }
        s
    }
    fn tokenize(s: &str) -> Vec<Token> {
        let mut result = vec![];
        let quotes = ['\'', '"', '`'];
        let mut it = s.chars();

        while let Some(ch) = it.next() {
            match ch {
                '#' => {
                    result.push(Token::Comment(it.collect::<String>().trim().into()));
                    break;
                }
                _ if quotes.contains(&ch) => {
                    let s = Self::take_until_escaped(ch, &mut it);
                    result.push(Token::Quoted(s));
                }
                ' ' => continue,
                _ => {
                    let mut s = Self::take_until_escaped(' ', &mut it);
                    s.insert(0, ch);
                    result.push(Token::Word(s));
                }
            }
        }
        result
    }
}

pub type Config = Vec<Hotkey>;

impl Rustable<Config> for *mut hotkey_t {
    fn to_rust(self) -> Config {
        let mut result = vec![];
        unsafe {
            let mut head = self;
            while !head.is_null() {
                let h = &*head;
                let command = h.command.convert();
                result.push(Hotkey {
                    chain: h.chain.to_rust(),
                    command,
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
