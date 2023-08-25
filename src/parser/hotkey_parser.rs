use std::sync::Arc;

use crate::{keyboard, parser::permutator::Permute};
use thiserror::Error;
pub use xcb::x::ModMask;

use super::*;

use anyhow::{Context, Result};

#[derive(Debug)]
pub struct CommentNode {
    pub tokens: Vec<Token>,
}

#[derive(Debug)]
pub struct ShortcutNode {
    pub tokens: Vec<Token>,
}

#[derive(Debug)]
pub struct CommandNode {
    pub tokens: Vec<Token>,
}

#[derive(Debug, Default)]
pub struct HotkeyParser {
    commands: Vec<GroupableToken>,
    title: Option<Arc<str>>,
    descriptions: Vec<GroupableToken>,
    shortcuts: Vec<GroupableToken>,
    errors: Vec<anyhow::Error>,
    hotkeys: Vec<Hotkey>,
    is_cycle: bool,
}

#[derive(Debug, Clone)]
enum GroupToken {
    Text(String),
    Chain(ChainMode),
    Plus,
    EmptySequence,
}

impl GroupToken {
    fn repr(&self) -> &str {
        match self {
            GroupToken::Text(s) => s,
            GroupToken::Chain(ChainMode::Locking) => " : ",
            GroupToken::Chain(_) => " ; ",
            GroupToken::Plus => " + ",
            GroupToken::EmptySequence => " _ ",
        }
    }

    /// Returns `true` if the group token is [`Plus`].
    ///
    /// [`Plus`]: GroupToken::Plus
    #[must_use]
    fn is_plus(&self) -> bool {
        matches!(self, Self::Plus)
    }
    #[must_use]
    fn as_text(&self) -> Option<&str> {
        match self {
            GroupToken::Text(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
enum GroupableToken {
    Single(Token, GroupToken),
    Group(Token, Vec<Vec<GroupToken>>),
}

trait Nther {
    fn get_self(&self) -> &[GroupableToken];
    fn nth_group(&self, n: usize) -> Option<&GroupableToken> {
        self.get_self().iter().filter(|i| i.is_group()).nth(n)
    }
    fn group_counts(&self) -> Vec<usize> {
        self.get_self()
            .iter()
            .filter_map(|g| g.as_group().map(|g| g.len()))
            .collect()
    }
}

impl Nther for [GroupableToken] {
    fn get_self(&self) -> &[GroupableToken] {
        self
    }
}

impl From<&GroupableToken> for Token {
    fn from(val: &GroupableToken) -> Self {
        val.source_token()
    }
}
impl From<GroupableToken> for Token {
    fn from(val: GroupableToken) -> Self {
        val.source_token()
    }
}

impl GroupableToken {
    fn as_group(&self) -> Option<&Vec<Vec<GroupToken>>> {
        if let Self::Group(_, v) = self {
            Some(v)
        } else {
            None
        }
    }
    fn as_single(&self) -> Option<&GroupToken> {
        if let Self::Single(_, s) = self {
            Some(s)
        } else {
            None
        }
    }
    fn get_range(&self) -> TokenRange {
        match self {
            Self::Group(t, _) | Self::Single(t, _) => t.get_range(),
        }
    }
    fn get_string(&self, bytes: &[u8]) -> String {
        String::from_utf8_lossy(&bytes[self.get_range()]).to_string()
    }
    fn source_token(&self) -> Token {
        match self {
            GroupableToken::Single(t, _) | GroupableToken::Group(t, _) => t.clone(),
        }
    }

    /// Returns `true` if the groupable token is [`Group`].
    ///
    /// [`Group`]: GroupableToken::Group
    #[must_use]
    fn is_group(&self) -> bool {
        matches!(self, Self::Group(..))
    }
}

impl HotkeyParser {
    fn parse_chord(&self, tokens: &[GroupableToken]) -> Result<Chord> {
        fn is_junk(t: &GroupableToken) -> bool {
            matches!(t, GroupableToken::Single(_, GroupToken::EmptySequence))
        }

        let mut tokens = tokens
            .iter()
            .filter(|t| !is_junk(t))
            .cloned()
            .collect::<Vec<GroupableToken>>();

        let mut chord = Chord {
            repr: String::new().into(),
            ..Default::default()
        };

        let mut last = tokens.pop().context("No tokens in binding!")?;

        // Handle the chain, if present
        if let GroupableToken::Single(_, GroupToken::Chain(mode)) = last {
            chord.lock_chain = mode;
            last = tokens.pop().context(ConfigParseError::InvalidBinding(
                last.source_token(),
                "Binding did not contain a key.".into(),
            ))?;
        }

        // The last element before the optional chain should be the keysym, or
        // button. Otherwise, this rule is invalid.
        let keysym_repr = match last {
            GroupableToken::Single(source, GroupToken::Text(t)) => {
                let mut t_slice = t.as_str();
                loop {
                    if t_slice.is_empty() {
                        return Err(ConfigParseError::InvalidBinding(
                            source,
                            "Invalid symbol".into(),
                        ))?;
                    }
                    if t_slice.starts_with('@') {
                        chord.event_type = KeyMode::KeyRelease;
                        t_slice = &t_slice[1..];
                    } else if t_slice.starts_with('~') {
                        chord.replay_event = ReplayMode::Replay;
                        t_slice = &t_slice[1..];
                    } else {
                        break;
                    }
                }
                let key = keyboard::symbol_from_string(t_slice)?;
                chord.keysym = key;
                t
            }
            _ => Err(ConfigParseError::InvalidBinding(
                last.source_token(),
                "The last token in a binding must be a key or a button.".into(),
            ))?,
        };

        let mut mod_tokens = vec![];
        for pair in tokens.chunks(2) {
            let mod_token = &pair[0];
            if let Some(plus_token) = pair.get(1) {
                let mod_text = mod_token
                    .as_single()
                    .context(ConfigParseError::InvalidBinding(
                        mod_token.source_token(),
                        "Expected 'single' variant".into(),
                    ))?
                    .as_text()
                    .context(ConfigParseError::InvalidBinding(
                        mod_token.source_token(),
                        "Expected modifier".into(),
                    ))?;
                let token = plus_token
                    .as_single()
                    .context(ConfigParseError::InvalidBinding(
                        plus_token.source_token(),
                        "Expected 'single' variant.".into(),
                    ))?;
                if !token.is_plus() {
                    Err(ConfigParseError::InvalidBinding(
                        plus_token.source_token(),
                        "Expected 'plus'".to_string(),
                    ))?;
                }
                mod_tokens.push(mod_text);
            } else {
                Err(ConfigParseError::InvalidBinding(
                    mod_token.source_token(),
                    "Expected a modifier after this".into(),
                ))?;
            }
        }
        let modfield = keyboard::modfield_from_mods(&mod_tokens)?;
        chord.modfield = modfield.bits().into();
        mod_tokens.push(&keysym_repr);
        chord.repr = mod_tokens.join(" + ").into();
        Ok(chord)
    }

    fn group(tokens: &[Token], context: &[u8]) -> Vec<GroupableToken> {
        let mut result = vec![];
        let mut it = tokens.iter();

        fn translate(token: &Token, context: &[u8]) -> Option<GroupToken> {
            match token {
                Token::Chain(_, mode) => Some(GroupToken::Chain(*mode)),
                Token::Plus(_) => Some(GroupToken::Plus),
                Token::Text(r) if r.len() == 1 && context[r.start] == b'_' => {
                    Some(GroupToken::EmptySequence)
                }
                Token::Text(_) => {
                    let content = token.get_string(context);
                    let mut it = content.chars();
                    let mut chars: Vec<char> = vec![];
                    while let Some(ch) = it.next() {
                        match ch {
                            '\\' => {
                                let peek = it.next();
                                match peek {
                                    Some('\n') => {}
                                    // Since we support escaping these characters inside of groups, we
                                    // should unescape them when extracting the string
                                    Some(',' | '{' | '}' | '-') => chars.push(peek.unwrap()),
                                    Some(ch) => {
                                        chars.push('\\');
                                        chars.push(ch)
                                    }
                                    _ => {}
                                }
                            }
                            ch => chars.push(ch),
                        }
                    }
                    Some(GroupToken::Text(chars.iter().collect()))
                }

                _ => None,
            }
        }
        while let Some(token) = it.next() {
            match token {
                Token::StartGroup(_) => {
                    let start_group_token = token.clone();
                    let mut groups = vec![];
                    let mut group = vec![];
                    for token in it.by_ref() {
                        match token {
                            Token::EndGroup(_) => {
                                if !group.is_empty() {
                                    groups.push(group);
                                }
                                result.push(GroupableToken::Group(start_group_token, groups));
                                break;
                            }
                            Token::Separator(_) => {
                                if !group.is_empty() {
                                    groups.push(group);
                                    group = vec![];
                                }
                            }
                            Token::Range(_, first_char, last_char) => {
                                let start: char = *first_char as char;
                                let end: char = *last_char as char;
                                for c in start..=end {
                                    groups.push(vec![GroupToken::Text(c.to_string())]);
                                }
                            }
                            _ => {
                                if let Some(gt) = translate(token, context) {
                                    group.push(gt);
                                }
                            }
                        }
                    }
                }
                _ => {
                    if let Some(gt) = translate(token, context) {
                        result.push(GroupableToken::Single(token.clone(), gt))
                    }
                }
            }
        }
        result
    }

    fn group_counts(v: &[GroupableToken]) -> Vec<usize> {
        v.iter()
            .filter_map(|g| g.as_group().map(|g| g.len()))
            .collect()
    }

    fn get_item(
        v: &[GroupableToken],
        group_index: usize,
        item_index: usize,
    ) -> Option<&Vec<GroupToken>> {
        let opt = v
            .iter()
            .filter_map(|g| g.as_group())
            .nth(group_index)
            .map(|g| g.get(item_index));
        opt?
    }

    fn split_comment(
        comment: Option<CommentNode>,
        context: &[u8],
    ) -> (Option<Token>, Vec<GroupableToken>) {
        if let Some(comment) = comment {
            let first_continue = comment.tokens.iter().position(|c| c.is_continue_comment());
            let (title_slice, description_slice) =
                comment.tokens.split_at(first_continue.unwrap_or(0));
            let groups = Self::group(description_slice, context);
            let title = title_slice.iter().find(|c| matches!(c, Token::Text(_)));
            (title.cloned(), groups)
        } else {
            (None, vec![])
        }
    }

    fn get_group_mapping_errors(&self, context: &[u8]) -> Option<String> {
        let count_shortcuts = Self::group_counts(&self.shortcuts);
        let count_commands = Self::group_counts(&self.commands);

        let mut err_string = String::new();
        if count_shortcuts.len() != count_commands.len() {
            err_string.push_str(&format!(
                "There are {} groups in the shortcut, and {} groups in the command.",
                count_shortcuts.len(),
                count_commands.len()
            ));
        }

        for (idx, (shortcut_count, command_count)) in
            count_shortcuts.iter().zip(&count_commands).enumerate()
        {
            if *shortcut_count != *command_count {
                // let shortcuts = nth_group(idx, &self.shortcuts).get_string(context);
                let shortcuts = self.shortcuts.nth_group(idx).unwrap().get_string(context);
                let commands = self.commands.nth_group(idx).unwrap().get_string(context);
                err_string.push_str(&format!(
                    "Group {} has {} bindings and {} commands:.\n  Bindings: {}\n  Shortcuts: {}",
                    idx + 1,
                    shortcut_count,
                    command_count,
                    shortcuts,
                    commands
                ))
            }
        }

        if !err_string.is_empty() {
            let line_info = self.shortcuts[0].source_token().get_line_info(context);
            Some(format!("{}\n  Starting at line {}", err_string, line_info))
        } else {
            None
        }
    }

    fn select_variant(
        tokens: &[GroupableToken],
        variant: Vec<Vec<GroupToken>>,
    ) -> Vec<GroupableToken> {
        let mut result: Vec<GroupableToken> = vec![];
        let mut idx = 0;
        for token in tokens {
            match token {
                GroupableToken::Single(_, _) => result.push(token.clone()),
                GroupableToken::Group(t, _) => {
                    let this_variant = variant[idx].clone();
                    idx += 1;
                    for token in this_variant.into_iter() {
                        result.push(GroupableToken::Single(t.clone(), token))
                    }
                }
            }
        }

        fn as_text(token: &GroupableToken) -> Option<String> {
            if let GroupableToken::Single(_, GroupToken::Text(s)) = token {
                Some(s.clone())
            } else {
                None
            }
        }

        // Merge adjacent text tokens into a single token. This is needed to support keys in
        // bindings such as 'bracket{left,right}'
        // Any number of text tokens could be adjacent, and they should all be merged
        // let groups: Vec<_> = result.split_inclusive(|t| !matches!(t, GroupableToken::Single(_, GroupToken::Text(_)))).collect();

        let mut it = result.into_iter();
        let mut result = vec![];
        while let Some(next) = it.next() {
            let GroupableToken::Single(source_token, GroupToken::Text(token_text)) = next else {
                result.push(next);
                continue;
            };
            // The current token is a text token -- it should be merged with all consecutive text
            let mut consecutive = vec![token_text];
            let mut pushed = false;
            for t in it.by_ref() {
                if let Some(s) = as_text(&t) {
                    consecutive.push(s);
                } else {
                    // No longer consecutive -- just push this token and continue the outer loop
                    result.push(t);
                    pushed = true;
                    break;
                }
            }
            let merged_token =
                GroupableToken::Single(source_token, GroupToken::Text(consecutive.join("")));
            if pushed {
                // Insert before the element that was just pushed
                let last = result.pop().unwrap();
                result.push(merged_token);
                result.push(last);
            } else {
                result.push(merged_token);
            }
        }

        result
    }

    fn string_variant(tokens: &[GroupableToken]) -> String {
        let mut s = String::new();

        for token in tokens {
            match token {
                GroupableToken::Single(_, t) if !matches!(t, GroupToken::EmptySequence) => {
                    s.push_str(t.repr())
                }
                GroupableToken::Group(_, _) => panic!("Should not be possible"),
                _ => {}
            }
        }
        s
    }

    fn make_chain(&mut self, shortcut: &[GroupableToken]) -> Result<Vec<Chord>> {
        let mut chain = vec![];
        let chords: Vec<_> = shortcut
            .split_inclusive(|t| matches!(t, GroupableToken::Single(_, GroupToken::Chain(_))))
            .collect();
        for chord_tokens in chords {
            chain.push(self.parse_chord(chord_tokens)?);
        }
        Ok(chain)
    }

    fn is_cycle(shortcuts: &[usize], commands: &[usize]) -> bool {
        shortcuts.is_empty() && commands.iter().product::<usize>() > 1
    }

    // Populate errors and hotkeys
    fn populate_errors_and_hotkeys(mut self, context: &[u8]) -> Self {
        // If this is a cycle, the mapping check is not needed
        if !self.is_cycle {
            if let Some(err_string) = self.get_group_mapping_errors(context) {
                self.errors
                    .push(ConfigParseError::GroupMappingMismatch(err_string).into());
            }
        }

        let count_shortcuts = Self::group_counts(&self.shortcuts);
        let count_commands = Self::group_counts(&self.commands);

        let count: Vec<_> = if self.is_cycle {
            count_commands
        } else {
            if count_shortcuts.len() != count_commands.len() {
                return self; // unrecoverable
            }
            // Special case for cycles
            count_shortcuts
                .into_iter()
                .zip(count_commands)
                .map(|(a, b)| std::cmp::min(a, b))
                .collect()
        };

        let variants = if self.is_cycle {
            Permute::front_first(&count)
        } else {
            Permute::back_first(&count)
        };

        let units: Vec<Unit> = Unit::make(
            &self.commands,
            &self.shortcuts,
            &self.descriptions,
            &variants,
            self.is_cycle,
        );

        let period = units.len() as i32;
        let mut delay = 0;

        for unit in units {
            let shortcut = &unit.shortcut;
            let command = &unit.command;
            let mut command_string = Self::string_variant(command).trim().to_string();
            let sync = command_string.starts_with(';');
            if sync {
                command_string.remove(0);
            }

            let command_string = command_string.trim().to_string();

            let chain = match self.make_chain(shortcut) {
                Ok(chain) => chain,
                Err(e) => {
                    self.errors.push(e);
                    continue;
                }
            };

            let cycle = if self.is_cycle {
                Some(Cycle { period, delay })
            } else {
                Default::default()
            };
            delay += 1;

            let hotkey = Hotkey {
                chain: chain.into(),
                command: command_string.into(),
                sync,
                cycle,
                title: self.title.clone(),
                description: unit.description.map(|d| Self::string_variant(&d).into()),
            };

            self.hotkeys.push(hotkey);
        }

        self
    }

    pub fn expand(
        shortcut: ShortcutNode,
        command: CommandNode,
        comment: Option<CommentNode>,
        context: &[u8],
    ) -> (Vec<Hotkey>, Vec<anyhow::Error>) {
        let command_groups = Self::group(&command.tokens, context);
        let (title, comment_groups) = Self::split_comment(comment, context);
        let shortcut_groups = Self::group(&shortcut.tokens, context);
        let is_cycle = Self::is_cycle(
            &Self::group_counts(&shortcut_groups),
            &Self::group_counts(&command_groups),
        );
        let instance = Self {
            shortcuts: shortcut_groups,
            title: title.map(|t| t.get_string(context).into()),
            commands: command_groups,
            descriptions: comment_groups,
            hotkeys: vec![],
            errors: vec![],
            is_cycle,
        };
        let result = instance.populate_errors_and_hotkeys(context);
        (result.hotkeys, result.errors)
    }
}

#[derive(Error, Debug)]
pub enum ChordParseError {
    #[error("{0}")]
    Contextual(String),
}

pub fn chord_from_tokens(tokens: &[Token], context: &[u8]) -> Result<Vec<Chord>> {
    let mut parser = HotkeyParser::default();
    let groups = HotkeyParser::group(tokens, context);
    match parser.make_chain(&groups) {
        Ok(e) => Ok(e),
        Err(err) => match err.downcast_ref::<ConfigParseError>() {
            Some(e) => {
                if let Some(t) = e.get_token() {
                    let s = t.get_string(context);
                    return Err(ChordParseError::Contextual(format!(
                        "{} - in text '{}'",
                        err, s
                    )))?;
                }
                Err(err)
            }
            None => Err(err),
        },
    }
}

#[derive(Debug)]
struct Unit {
    shortcut: Vec<GroupableToken>,
    command: Vec<GroupableToken>,
    description: Option<Vec<GroupableToken>>,
}
impl Unit {
    fn make(
        commands: &[GroupableToken],
        shortcuts: &[GroupableToken],
        descriptions: &[GroupableToken],
        variants: &Vec<Vec<usize>>,
        is_cycle: bool,
    ) -> Vec<Unit> {
        let mut result = vec![];
        'outer: for variant in variants {
            let mut st = vec![];
            let mut ct = vec![];
            let mut dt = vec![];
            for (g, i) in variant.iter().cloned().enumerate() {
                if !is_cycle {
                    if let Some(shortcut) = HotkeyParser::get_item(shortcuts, g, i) {
                        st.push(shortcut.clone());
                    } else {
                        continue 'outer;
                    }
                }
                let command = HotkeyParser::get_item(commands, g, i);
                if let Some(command) = command {
                    ct.push(command.clone());
                    dt.push(HotkeyParser::get_item(descriptions, g, i).cloned());
                } else {
                    continue 'outer;
                }
            }

            let command = HotkeyParser::select_variant(commands, ct);
            let description = if !descriptions.is_empty() && dt.iter().all(|d| d.is_some()) {
                Some(HotkeyParser::select_variant(
                    descriptions,
                    dt.into_iter().map(|o| o.unwrap()).collect(),
                ))
            } else {
                None
            };
            if is_cycle {
                st.push(vec![]);
            }
            let shortcut = HotkeyParser::select_variant(shortcuts, st);
            result.push(Unit {
                shortcut,
                command,
                description,
            })
        }
        result
    }
}
