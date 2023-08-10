#![allow(unused)]
use super::hotkey_parser::*;
use super::{ChainMode, ConfigParseError, Token, TokenRange};
use crate::{
    keyboard::keysyms,
    sxhkd::types::{Chord, Hotkey},
};
use anyhow::{anyhow, Result};
use std::convert::TryInto;

pub struct Parser<'a> {
    tokens: &'a [Token],
    context: &'a [u8],
    cursor: usize,
    hotkeys: Vec<Hotkey>,
    pub errors: Vec<anyhow::Error>,
}

type ConcreteCommand = String;

impl<'a> Parser<'a> {
    pub fn build(context: &'a [u8], tokens: &'a [Token]) -> Result<Self> {
        let tree = Parser {
            context,
            tokens,
            cursor: Default::default(),
            hotkeys: vec![],
            errors: vec![],
        };
        tree.populate()
    }

    fn read_binding(&mut self, tok: &Token) -> Result<ShortcutNode> {
        let mut node = ShortcutNode {
            tokens: vec![tok.clone()],
        };
        while let Some(token) = self.tokens.get(self.cursor) {
            self.advance_cursor();
            if matches!(token, Token::EndBinding(_)) {
                node.tokens.push(token.clone());
                return Ok(node);
            }
            match token {
                Token::StartBinding(_)
                | Token::Chain(_, _)
                | Token::Plus(_)
                | Token::Text(_)
                | Token::Range(_, _, _)
                | Token::Separator(_)
                | Token::StartGroup(_)
                | Token::EndGroup(_) => node.tokens.push(token.clone()),
                _ => Err(ConfigParseError::InvalidToken(
                    token.clone(),
                    token.get_string(self.context),
                    String::from("While parsing binding"),
                ))?,
            }
        }
        Err(ConfigParseError::UnterminatedBinding(
            node.tokens.swap_remove(0),
        ))?
    }
    fn read_command(&mut self, token: &Token) -> Result<CommandNode> {
        let mut node = CommandNode {
            tokens: vec![token.clone()],
        };
        while let Some(token) = self.tokens.get(self.cursor) {
            self.advance_cursor();
            if matches!(token, Token::EndCommand(_)) {
                node.tokens.push(token.clone());
                return Ok(node);
            }
            match token {
                Token::StartCommand(_)
                | Token::EndCommand(_)
                | Token::Text(_)
                | Token::Range(_, _, _)
                | Token::StartGroup(_)
                | Token::Separator(_)
                | Token::EndGroup(_) => node.tokens.push(token.clone()),
                _ => Err(ConfigParseError::InvalidToken(
                    token.clone(),
                    token.get_string(self.context),
                    String::from("While parsing command"),
                ))?,
            }
        }
        Err(ConfigParseError::UnterminatedCommand(
            node.tokens.swap_remove(0),
        ))?
    }
    fn read_comment(&mut self, token: &Token) -> anyhow::Result<CommentNode> {
        let mut node = CommentNode {
            tokens: vec![token.clone()],
        };
        while let Some(token) = self.tokens.get(self.cursor) {
            self.advance_cursor();
            if matches!(token, Token::EndComment(_)) {
                node.tokens.push(token.clone());
                return Ok(node);
            }
            match token {
                Token::StartComment(_)
                | Token::Text(_)
                | Token::StartGroup(_)
                | Token::ContinueComment(_)
                | Token::Range(_, _, _)
                | Token::Separator(_)
                | Token::EndGroup(_)
                | Token::EndComment(_) => node.tokens.push(token.clone()),
                _ => Err(ConfigParseError::InvalidToken(
                    token.clone(),
                    token.get_string(self.context),
                    String::from("While parsing comment"),
                ))?,
            }
        }
        Err(ConfigParseError::UnterminatedComment(
            node.tokens.swap_remove(0),
        ))?
    }

    fn advance_cursor(&mut self) {
        self.cursor += 1;
    }

    fn populate(mut self) -> anyhow::Result<Self> {
        let mut comment: Option<CommentNode> = None;
        let mut shortcut: Option<ShortcutNode> = None;
        while let Some(token) = self.tokens.get(self.cursor) {
            self.advance_cursor();
            match token {
                Token::EmptyLine(_) => {
                    comment = None;
                    shortcut = None;
                }
                Token::StartComment(_) => match self.read_comment(token) {
                    Ok(s) => comment = Some(s),
                    Err(e) => self.errors.push(e),
                },
                Token::StartBinding(_) => match self.read_binding(token) {
                    Ok(s) => shortcut = Some(s),
                    Err(e) => self.errors.push(e),
                },
                Token::StartCommand(_) => match shortcut.take() {
                    Some(shortcut) => match self.read_command(token) {
                        Ok(command) => {
                            let (hotkeys, errors) = HotkeyParser::expand(
                                shortcut,
                                command,
                                comment.take(),
                                self.context,
                            );
                            self.hotkeys.extend(hotkeys);
                            self.errors.extend(errors);
                        }
                        Err(e) => self.errors.push(e),
                    },
                    None => {
                        self.errors.push(anyhow!(ConfigParseError::SyntaxError(
                            "Encountered command without binding!".into(),
                        )));
                    }
                },
                _ => {
                    Err(ConfigParseError::InvalidToken(
                        token.clone(),
                        token.get_string(self.context),
                        String::from("Unexpected token"),
                    ))?;
                }
            }
        }
        Ok(self)
    }

    pub fn get_hotkeys(&self) -> (&Vec<Hotkey>, &Vec<anyhow::Error>) {
        (&self.hotkeys, &self.errors)
    }
}

mod parser_tests {
    use super::super::*;
    use super::*;
    #[test]
    fn chains_in_groups() {
        let rule = b"super + z ; {a;b,b;c} ; {1-3}
  echo {some, replacement} {1-3}
";
        let tokens = Scanner::scan(rule).unwrap();
        let parser = Parser::build(rule, &tokens);
    }
}
