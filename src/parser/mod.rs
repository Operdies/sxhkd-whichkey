pub mod config;
pub use types::Hotkey;
pub mod types;

use std::fmt::Display;
use std::ops::Range;
mod hotkey_parser;
mod permutator;
mod scanner;
mod token_parser;

pub use types::*;

pub use scanner::Scanner;
pub use token_parser::Parser;

use self::hotkey_parser::ChordParseError;

#[derive(Debug, Clone)]
pub enum RangeError {
    StartGreaterThanEnd,
    MultipleCharsInRange,
    InvalidTokenSequence,
}

#[derive(Debug, Clone)]
pub enum ConfigParseError {
    PermissionError(String, String),
    ParseError(String),
    SyntaxError(String),
    InvalidRange(RangeError),
    GroupMappingMismatch(String),
    InvalidEscape(usize, u8),
    InvalidToken(Token, String, String),
    UnterminatedBinding(Token),
    UnterminatedCommand(Token),
    UnterminatedComment(Token),
    UnterminatedGroup(Token),
    InvalidBinding(Token, String),
}

pub type TokenRange = Range<usize>;

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    StartCommand(TokenRange),
    EndCommand(TokenRange),
    StartBinding(TokenRange),
    EndBinding(TokenRange),
    StartComment(TokenRange),
    ContinueComment(usize),
    EndComment(TokenRange),
    Chain(usize, ChainMode),
    Plus(usize),
    Separator(usize),
    Text(TokenRange),
    StartGroup(TokenRange),
    EndGroup(TokenRange),
    Range(TokenRange, u8, u8),
    EmptyLine(TokenRange),
}

impl Token {
    fn get_string(&self, context: &[u8]) -> String {
        String::from_utf8_lossy(&context[self.get_range()]).to_string()
    }
    fn get_line_info(&self, context: &[u8]) -> LineInfo {
        let start = self.get_range().start;
        let mut line = 1;
        let mut column = 0;
        for ch in &context[0..start] {
            if *ch == b'\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        // This would happen in the case where the token starts on the first line
        LineInfo { line, column }
    }
}

impl Token {
    pub fn get_range(&self) -> TokenRange {
        match self {
            Token::Range(r, _, _) | Token::Text(r) | Token::EmptyLine(r) => r.clone(),
            Token::StartCommand(r)
            | Token::EndCommand(r)
            | Token::StartBinding(r)
            | Token::EndBinding(r)
            | Token::StartGroup(r)
            | Token::EndGroup(r)
            | Token::StartComment(r)
            | Token::EndComment(r) => r.clone(),
            Token::ContinueComment(r)
            | Token::Separator(r)
            | Token::Chain(r, _)
            | Token::Plus(r) => *r..*r + 1,
        }
    }

    /// Returns `true` if the token is [`ContinueComment`].
    ///
    /// [`ContinueComment`]: Token::ContinueComment
    #[must_use]
    pub fn is_continue_comment(&self) -> bool {
        matches!(self, Self::ContinueComment(..))
    }
}

struct LineInfo {
    line: usize,
    column: usize,
}

impl Display for LineInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("{}:{}", self.line, self.column))
    }
}

impl ConfigParseError {
    fn get_token(&self) -> Option<&Token> {
        use ConfigParseError::*;
        match self {
            InvalidToken(t, _, _)
            | UnterminatedBinding(t)
            | UnterminatedCommand(t)
            | UnterminatedComment(t)
            | UnterminatedGroup(t)
            | InvalidBinding(t, _) => Some(t),
            _ => None,
        }
    }

    fn contextualize(&self, context: &[u8]) -> String {
        if let Some(token) = self.get_token() {
            let token_string = token.get_string(context);
            let line_info = token.get_line_info(context);
            format!(
                "{}\n In token '{}' (line {})",
                self, token_string, line_info
            )
        } else {
            self.to_string()
        }
    }
}

impl Display for ConfigParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigParseError::PermissionError(file, error) => {
                f.write_fmt(format_args!(
                    "Unable to read config file: at '{file}': {error}"
                ))?;
            }
            ConfigParseError::ParseError(desc) => {
                f.write_fmt(format_args!("Error during parsing: {desc}"))?;
            }
            ConfigParseError::InvalidToken(token, slice, desc) => {
                f.write_fmt(format_args!("{}: {:?}, ({})", desc, token, slice))?;
            }
            ConfigParseError::UnterminatedBinding(_)
            | ConfigParseError::UnterminatedGroup(_)
            | ConfigParseError::UnterminatedCommand(_)
            | ConfigParseError::UnterminatedComment(_) => {
                f.write_str(&format!("Unterminated item: {:?}", self))?
            }
            ConfigParseError::InvalidRange(r) => f.write_str(&format!("{:?}", r))?,
            ConfigParseError::SyntaxError(s) => f.write_str(s)?,
            ConfigParseError::InvalidBinding(_, s) => {
                f.write_str(&format!("Invalid Binding: {}", s))?
            }
            ConfigParseError::GroupMappingMismatch(s) => {
                f.write_str(&format!("Group Mapping Mismatch:\n{}", s))?
            }
            _ => f.write_str(&format!("{:?}", self))?,
        };
        Ok(())
    }
}

pub fn parse_chord_chain(chords: &str) -> anyhow::Result<Vec<Chord>> {
    let bytes = chords.as_bytes();
    match scanner::Scanner::scan(bytes) {
        Ok(tokens) => hotkey_parser::chord_from_tokens(&tokens, bytes),
        Err(e) => match e.downcast_ref::<ConfigParseError>() {
            None => Err(e),
            Some(c) => Err(ChordParseError::Contextual(c.contextualize(bytes)))?,
        },
    }
}

impl std::error::Error for ConfigParseError {}

#[allow(unused)]
mod scanner_test {
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_1() -> anyhow::Result<()> {
        let rule = "super + shift + e ; { 1, 2, 3, 4, 5}
  { ~/.config/bspwm/scripts/i3lock-fancy/i3lock-minimalist.sh #  Lock\
    , bspc quit # 󰗼 Logout\
    , systemctl poweroff #  Shutdown\
    , systemctl reboot # 󰁯 Reboot\
    , ~/.config/bspwm/scripts/i3lock-fancy/i3lock-minimalist.sh; systemctl suspend #  Sleep\
  }
"
        .as_bytes();
        let _ = Scanner::scan(rule);
        Ok(())
    }
    #[test]
    fn test_token_stream() -> anyhow::Result<()> {
        let rule = "super + F3 ;\
{ a,\
b , \
c :\
e }
  echo {1,a-e, 2}
"
        .as_bytes();
        let tokens = Scanner::scan(rule)?;
        let expected = vec![
            Token::StartBinding(0..26),
            Token::Text(0..5),
            Token::Plus(6),
            Token::Text(8..10),
            Token::Chain(11, ChainMode::Once),
            Token::StartGroup(12..26),
            Token::Text(14..15),
            Token::Separator(15),
            Token::Text(16..17),
            Token::Separator(18),
            Token::Text(20..21),
            Token::Chain(22, ChainMode::Locking),
            Token::Text(23..24),
            Token::EndGroup(12..26),
            Token::EndBinding(0..26),
            Token::StartCommand(29..44),
            Token::Text(29..34),
            Token::StartGroup(34..44),
            Token::Text(35..36),
            Token::Separator(36),
            Token::Range(37..40, b'a', b'e'),
            Token::Separator(40),
            Token::Text(41..43),
            Token::EndGroup(34..44),
            Token::EndCommand(29..44),
        ];
        // print_tokens(rule, &tokens);
        assert_eq!(tokens.len(), expected.len());
        for (l, r) in tokens.into_iter().zip(expected) {
            assert_eq!(l, r);
        }
        Ok(())
    }

    #[test]
    fn test_comment_expansion() -> anyhow::Result<()> {
        let rule = "
# This rule rules
# Documentation { with,expansion} {1-3}
super + {a,b} ; {1-3}
  echo {some, replacement} {1-3}

"
        .as_bytes();
        let tokens = Scanner::scan(rule)?;
        // print_tokens(rule, &tokens);
        assert!(matches!(
            tokens[..],
            [
                Token::EmptyLine(_),
                Token::StartComment(_),
                Token::Text(_),
                Token::ContinueComment(_),
                Token::Text(_),
                Token::StartGroup(_),
                Token::Text(_),
                Token::Separator(_),
                Token::Text(_),
                Token::EndGroup(_),
                Token::Text(_),
                Token::StartGroup(_),
                Token::Range(_, _, _),
                Token::EndGroup(_),
                Token::EndComment(_),
                Token::StartBinding(_),
                Token::Text(_),
                Token::Plus(_),
                Token::StartGroup(_),
                Token::Text(_),
                Token::Separator(_),
                Token::Text(_),
                Token::EndGroup(_),
                Token::Chain(_, ChainMode::Once),
                Token::StartGroup(_),
                Token::Range(_, _, _),
                Token::EndGroup(_),
                Token::EndBinding(_),
                Token::StartCommand(_),
                Token::Text(_),
                Token::StartGroup(_),
                Token::Text(_),
                Token::Separator(_),
                Token::Text(_),
                Token::EndGroup(_),
                Token::Text(_),
                Token::StartGroup(_),
                Token::Range(_, _, _),
                Token::EndGroup(_),
                Token::EndCommand(_),
                Token::EmptyLine(_),
            ]
        ));
        let tree = super::token_parser::Parser::build(rule, &tokens)?;
        let (hotkeys, errors) = tree.get_hotkeys();
        assert_eq!(0, errors.len());
        assert_eq!(6, hotkeys.len());
        for hk in hotkeys {
            assert_eq!(hk.chain.len(), 2);
        }
        let expected_reprs: &[_] = &[
            ("super + a", "1"),
            ("super + a", "2"),
            ("super + a", "3"),
            ("super + b", "1"),
            ("super + b", "2"),
            ("super + b", "3"),
        ];
        for (hk, (first, second)) in hotkeys.iter().zip(expected_reprs) {
            assert_eq!(hk.title, Some("This rule rules".into()));
            assert_eq!(hk.chain[0].repr.to_string(), *first);
            assert_eq!(hk.chain[1].repr.to_string(), *second);
        }
        assert_eq!(&hotkeys[0].command.to_string(), "echo some 1");
        assert_eq!(&hotkeys[5].command.to_string(), "echo  replacement 3");
        assert_eq!(
            hotkeys[3].description,
            Some("Documentation expansion 1".into())
        );
        assert_eq!(hotkeys[2].description, Some("Documentation  with 3".into()));
        Ok(())
    }

    #[test]
    fn test_comment_no_continue() -> anyhow::Result<()> {
        let rule = b"# This rule rules

# Documentation {with,expansion}
";
        let tokens = Scanner::scan(rule)?;
        assert!(matches!(
            tokens[..],
            [
                Token::StartComment(_),
                Token::Text(_),
                Token::EndComment(_),
                Token::EmptyLine(_),
                Token::StartComment(_),
                Token::Text(_),
                Token::StartGroup(_),
                Token::Text(_),
                Token::Separator(_),
                Token::Text(_),
                Token::EndGroup(_),
                Token::EndComment(_),
            ]
        ));
        Ok(())
    }

    #[allow(unused)]
    fn print_tokens(context: &[u8], tokens: &Vec<Token>) {
        for token in tokens {
            let range = token.get_range();
            let slice = &context[range.to_owned()];
            let slice = String::from_utf8_lossy(slice);
            println!("{:?}: '{}'", token, slice);
        }
    }
    fn print_errors(errors: &[anyhow::Error], context: &[u8]) {
        for error in errors {
            if let Some(e) = error.downcast_ref::<ConfigParseError>() {
                println!("{}", e.contextualize(context));
            } else {
                println!("{}", error);
            }
        }
    }
    #[test]
    fn test_underscore_in_binding() -> anyhow::Result<()> {
        let rule = b"# focus the {next,previous} window in the current desktop
super + {_,shift +}c
	bspc node -f {next,prev}.local.!hidden.window
";
        let tokens = Scanner::scan(rule)?;
        let tree = super::token_parser::Parser::build(rule, &tokens)?;
        let (hotkeys, errors) = tree.get_hotkeys();
        print_errors(errors, rule);
        assert_eq!(0, errors.len());
        assert_eq!(2, hotkeys.len());
        {
            let rule = &hotkeys[0];
            assert_eq!(
                rule.description,
                Some("focus the next window in the current desktop".into())
            );
            assert_eq!(1, rule.chain.len());
            assert_eq!("super + c", rule.chain[0].repr.to_string());
            assert_eq!("bspc node -f next.local.!hidden.window", rule.command.to_string());
        }
        {
            let rule = &hotkeys[1];
            assert_eq!(
                rule.description,
                Some("focus the previous window in the current desktop".into())
            );
            assert_eq!(1, rule.chain.len());
            assert_eq!("super + shift + c", rule.chain[0].repr.to_string());
            assert_eq!("bspc node -f prev.local.!hidden.window", rule.command.to_string());
        }
        Ok(())
    }

    #[test]
    fn test_empty_sequence() -> Result<()> {
        let rule = b"super + {_, shift +} {1-9,0}
  bspc {desktop -f, node -d} '^{1-9,10}'
";
        let tokens = Scanner::scan(rule)?;
        let tree = super::token_parser::Parser::build(rule, &tokens)?;
        let (hotkeys, errors) = tree.get_hotkeys();
        print_errors(errors, rule);
        assert_eq!(20, hotkeys.len());
        assert_eq!(0, errors.len());
        for hk in hotkeys {
            assert_eq!(1, hk.chain.len());
        }
        for i in 0..=9 {
            let expected_number = if i < 9 { i + 1 } else { 0 };
            let exp1 = format!("super + {}", expected_number);
            let exp2 = format!("super + shift + {}", expected_number);
            let hk1 = &hotkeys[i];
            let hk2 = &hotkeys[i + 10];
            assert_eq!(exp1, hk1.chain[0].repr.to_string());
            assert_eq!(exp2, hk2.chain[0].repr.to_string());
        }
        Ok(())
    }
    #[test]
    fn test_partial_tokens_with_groups() -> Result<()> {
        let rule = b"super + Tab : bracket{left,right}
	bspc desktop -f {prev,next}.local";
        let tokens = Scanner::scan(rule)?;
        let tree = super::token_parser::Parser::build(rule, &tokens)?;
        let (hotkeys, errors) = tree.get_hotkeys();
        print_errors(errors, rule);

        assert_eq!(2, hotkeys.len());
        assert_eq!(0, errors.len());

        {
            let hk = &hotkeys[0];
            assert_eq!(2, hk.chain.len());
            assert!(hk.chain[0].lock_chain.is_locking());
            assert_eq!(hk.chain[0].repr.to_string(), "super + Tab");
            assert_eq!(hk.chain[1].repr.to_string(), "bracketleft");
            assert_eq!(hk.command.to_string(), "bspc desktop -f prev.local");
        }
        {
            let hk = &hotkeys[1];
            assert_eq!(2, hk.chain.len());
            assert!(hk.chain[0].lock_chain.is_locking());
            assert_eq!(hk.chain[0].repr.to_string(), "super + Tab");
            assert_eq!(hk.chain[1].repr.to_string(), "bracketright");
            assert_eq!(hk.command.to_string(), "bspc desktop -f next.local");
        }

        Ok(())
    }
    #[test]
    fn test_partial_modifiers() -> Result<()> {
        let rule = b"s{hift,uper} + x
  echo {shift,super}
";
        let tokens = Scanner::scan(rule)?;
        let tree = super::token_parser::Parser::build(rule, &tokens)?;
        let (hotkeys, errors) = tree.get_hotkeys();
        print_errors(errors, rule);
        assert_eq!(2, hotkeys.len());
        assert_eq!(0, errors.len());

        {
            let hk = &hotkeys[0];
            assert_eq!(1, hk.chain.len());
            assert_eq!(hk.chain[0].repr.to_string(), "shift + x");
            assert_eq!(hk.command.to_string(), "echo shift");
        }
        {
            let hk = &hotkeys[1];
            assert_eq!(1, hk.chain.len());
            assert_eq!(hk.chain[0].repr.to_string(), "super + x");
            assert_eq!(hk.command.to_string(), "echo super");
        }
        Ok(())
    }
    #[test]
    fn test_cycles() -> Result<()> {
        let rule = b"a
  echo {1,2} {3,4} {5,6}";
        let tokens = Scanner::scan(rule)?;
        let tree = super::token_parser::Parser::build(rule, &tokens)?;
        let (hotkeys, errors) = tree.get_hotkeys();
        print_errors(errors, rule);

        // NOTE: Cycles are expanded front-first, as opposed to regular bindings which are expanded
        // back-first
        let expected_order: &[_] = &[
            (1, 3, 5),
            (2, 3, 5),
            (1, 4, 5),
            (2, 4, 5),
            (1, 3, 6),
            (2, 3, 6),
            (1, 4, 6),
            (2, 4, 6),
        ];
        let expected_count = 8;
        assert_eq!(8, hotkeys.len());
        assert_eq!(0, errors.len());

        let mut delay = 0;
        for (delay, (hk, expected)) in hotkeys.iter().zip(expected_order).enumerate() {
            let expected = format!("echo {} {} {}", expected.0, expected.1, expected.2);
            assert_eq!(hk.command.to_string(), expected);
            let cycle = hk.cycle.clone().unwrap();
            assert_eq!(cycle.period, expected_count);
            assert_eq!(cycle.delay, delay as i32);
        }

        Ok(())
    }
    /// TODO implement this test
    #[test]
    fn name() {
        // super + F3 ; a ; b ; {c,d}
        //   echo {1,2}
        // super + F3 ; a ; b ; c
        //   echo 1
    }
    #[test]
    fn test_command_with_escape_at_end() -> Result<()> {
        let rule = "# Power Menu
# { Lock,󰗼 Logout, Shutdown,󰁯 Reboot, Sleep}
super + shift + e ; { 1, 2, 3, 4, 5}
  { ~/.config/bspwm/scripts/i3lock-fancy/i3lock-minimalist.sh \
    , bspc quit \
    , systemctl poweroff \
    , systemctl reboot \
    , ~/.config/bspwm/scripts/i3lock-fancy/i3lock-minimalist.sh; systemctl suspend \
  }
"
        .as_bytes();
        let tokens = Scanner::scan(rule)?;
        let tree = super::token_parser::Parser::build(rule, &tokens)?;
        let (hotkeys, errors) = tree.get_hotkeys();
        print_errors(errors, rule);
        assert_eq!(0, errors.len());
        assert_eq!(5, hotkeys.len());
        assert_eq!(hotkeys[1].command.to_string(), "bspc quit");
        Ok(())
    }
    #[test]
    fn singles_description() -> Result<()> {
        let rule = b"# tiled
super + t : t
  bsp-layout set tiled ; bspc node -t tiled
";
        let tokens = Scanner::scan(rule)?;
        let tree = super::token_parser::Parser::build(rule, &tokens)?;
        let (hotkeys, errors) = tree.get_hotkeys();
        print_errors(errors, rule);
        assert_eq!(0, errors.len());
        assert_eq!(1, hotkeys.len());
        assert_eq!(
            hotkeys[0].command.to_string(),
            "bsp-layout set tiled ; bspc node -t tiled"
        );
        assert_eq!(hotkeys[0].title, None);
        assert_eq!(hotkeys[0].description, Some("tiled".into()));
        Ok(())
    }

    #[test]
    fn test_no_title_or_description() -> Result<()> {
        let rule = b"
super + F4
  echo {1,2} {3,4}

super + F6 ; a ; b
  echo 123
";
        let tokens = Scanner::scan(rule)?;
        let tree = super::token_parser::Parser::build(rule, &tokens)?;
        let (hotkeys, errors) = tree.get_hotkeys();
        print_errors(errors, rule);
        assert_eq!(0, errors.len());
        assert_eq!(5, hotkeys.len());

        for hk in hotkeys {
            assert_eq!(None, hk.title);
            assert_eq!(None, hk.description);
        }
        Ok(())
    }
}
