use super::{ChainMode, ConfigParseError, Token, TokenRange};
use anyhow::Result;

pub struct Scanner<'a> {
    cursor: usize,
    input: &'a [u8],
}

impl<'a> Scanner<'a> {
    pub fn scan(input: &'a [u8]) -> Result<Vec<Token>> {
        Scanner { cursor: 0, input }.get_token_stream()
    }

    fn advance_while<F>(&mut self, p: F)
    where
        F: Fn(&u8) -> bool,
    {
        while self.input.get(self.cursor).is_some_and(&p) {
            self.cursor += 1;
        }
    }
    // Increment cursor while the predicate is matched. When a backslash is encountered, the
    // backslash and the following character is skipped.
    fn escaped_advance_while<F>(&mut self, predicate: F)
    where
        F: Fn(u8) -> bool,
    {
        while self.cursor < self.input.len() {
            let ch = self.input[self.cursor];
            if ch == b'\\' {
                self.cursor += 2;
                continue;
            }
            if !predicate(ch) {
                break;
            }
            self.cursor += 1;
        }
        // Handle the edge case where the last character in the string is a backslash
        if self.cursor > self.input.len() {
            self.cursor = self.input.len()
        }
    }

    // Shorthand for adding the single character token under the cursor
    fn add_unit_and_advance(&mut self) -> Result<Token> {
        let range = self.cursor;
        self.cursor += 1;
        match self.input[self.cursor - 1] {
            b':' => Ok(Token::Chain(range, ChainMode::Locking)),
            b';' => Ok(Token::Chain(range, ChainMode::Once)),
            b'+' => Ok(Token::Plus(range)),
            x => Err(ConfigParseError::ParseError(format!(
                "Expected unit token, found {}",
                x
            )))?,
        }
    }
    fn parse_group<F>(&mut self, maker: F) -> Result<Vec<Token>>
    where
        F: Fn(TokenRange, &[u8]) -> Result<Vec<Token>>,
    {
        let mut tokens = vec![];
        let start = self.cursor;
        self.cursor += 1;
        while self.cursor < self.input.len() {
            match self.input[self.cursor] {
                b'\\' => self.cursor += 2,
                b',' => {
                    tokens.push(Token::Separator(self.cursor));
                    self.cursor += 1
                }
                b'}' => {
                    self.cursor += 1;
                    tokens.insert(0, Token::StartGroup(start..self.cursor));
                    tokens.push(Token::EndGroup(start..self.cursor));
                    return Ok(tokens);
                }
                _ => {
                    let start = self.cursor;
                    self.escaped_advance_while(|c| !matches!(c, b'}' | b','));
                    tokens.extend(maker(start..self.cursor, self.input)?);
                }
            }
        }
        Err(ConfigParseError::UnterminatedGroup(
            tokens
                .get(0)
                .cloned()
                .unwrap_or_else(|| Token::StartGroup(start..self.cursor)),
        ))?
    }

    fn parse_command(&mut self) -> Result<Vec<Token>> {
        // Command text will be sent directly to the shell after any substitution has been
        // performed. Whitespace may or may not be significant, depending on the shell. We should
        // make no assumptions here.
        let mut tokens = vec![];
        while self.cursor < self.input.len() {
            match self.input[self.cursor] {
                b'\\' => self.cursor += 2,
                b'\n' => break,
                b'{' => {
                    tokens.extend(self.parse_group(|range, context| {
                        match context[range.clone()] {
                            [from, b'-', to] => Ok(vec![Token::Range(range, from, to)]),
                            _ => Ok(vec![Token::Text(range)]),
                        }
                    })?);
                }
                _ => {
                    let start = self.cursor;
                    self.escaped_advance_while(|c| c != b'\n' && c != b'{');
                    let end = self.cursor;
                    tokens.push(Token::Text(start..end));
                }
            }
        }
        Ok(tokens)
    }

    fn parse_binding(&mut self) -> Result<Vec<Token>> {
        let mut tokens = vec![];
        let unit_tokens: &[u8] = &[b'{', b'}', b':', b';', b'+'];
        while self.cursor < self.input.len() {
            match self.input[self.cursor] {
                b'\\' => self.cursor += 2,
                b'{' => tokens.extend(self.parse_group(|range, context| {
                    match self.input[range.start..range.end] {
                        [range_start, b'-', range_end] => Ok(vec![Token::Range(
                            range.start..range.end,
                            range_start,
                            range_end,
                        )]),
                        _ => {
                            let mut tokens = vec![];
                            let (mut idx, end) = (range.start, range.end);
                            while idx < end {
                                match context[idx] {
                                    b'\\' => {
                                        idx += 2;
                                    }
                                    b':' => {
                                        tokens.push(Token::Chain(idx, ChainMode::Locking));
                                        idx += 1;
                                    }
                                    b';' => {
                                        tokens.push(Token::Chain(idx, ChainMode::Once));
                                        idx += 1;
                                    }
                                    b'+' => {
                                        tokens.push(Token::Plus(idx));
                                        idx += 1;
                                    }
                                    x if x.is_ascii_whitespace() => idx += 1,
                                    _ => {
                                        let from = idx;
                                        let to = context[from..end].iter().position(|c| {
                                            matches!(c, b':' | b';' | b' ' | b'+')
                                                || c.is_ascii_whitespace()
                                        });
                                        let to = if let Some(to) = to { to + from } else { end };
                                        tokens.push(Token::Text(from..to));
                                        idx = to;
                                    }
                                }
                            }
                            Ok(tokens)
                        }
                    }
                })?),
                x if unit_tokens.contains(&x) => tokens.push(self.add_unit_and_advance()?),
                b'\n' => break,
                x if x.is_ascii_whitespace() => self.cursor += 1,
                _ => {
                    let start = self.cursor;
                    self.advance_while(|c| !c.is_ascii_whitespace() && !unit_tokens.contains(c));
                    tokens.push(Token::Text(start..self.cursor));
                }
            }
        }
        Ok(tokens)
    }

    fn parse_comment(&mut self) -> Result<Vec<Token>> {
        let mut result = vec![];
        loop {
            // Consume any leading whitespace
            self.advance_while(|c| !c.eq(&b'\n') && c.is_ascii_whitespace());
            let tokens = self.parse_command()?;
            result.extend(tokens);
            // If the next line is also a line comment, append it to the current comment
            match self.input.get(self.cursor + 1) {
                Some(b'#') => {
                    // Skip the newline and the '#'
                    self.cursor += 2;
                    result.push(Token::ContinueComment(self.cursor));
                }
                _ => break,
            }
        }
        Ok(result)
    }

    fn get_token_stream(mut self) -> Result<Vec<Token>> {
        self.cursor = 0;
        let mut result = vec![];
        while self.cursor < self.input.len() {
            match self.input[self.cursor] {
                b'\n' => {
                    let start = self.cursor;
                    self.advance_while(|c| c.is_ascii_whitespace());
                    result.push(Token::EmptyLine(start..self.cursor));
                }
                c => {
                    self.advance_while(|c| !c.eq(&b'\n') && c.is_ascii_whitespace());
                    if c == b'#' {
                        // Skip leading '#'
                        let start = self.cursor;
                        self.cursor += 1;
                        let inner = self.parse_comment()?;
                        result.push(Token::StartComment(start..self.cursor));
                        result.extend(inner);
                        result.push(Token::EndComment(start..self.cursor));
                    } else if self.cursor == 0 || self.input[self.cursor - 1] == b'\n' {
                        let start = self.cursor;
                        let binding_tokens = self.parse_binding()?;
                        result.push(Token::StartBinding(start..self.cursor));
                        result.extend(binding_tokens);
                        result.push(Token::EndBinding(start..self.cursor));
                    } else {
                        let start = self.cursor;
                        let inner = self.parse_command()?;
                        result.push(Token::StartCommand(start..self.cursor));
                        result.extend(inner);
                        result.push(Token::EndCommand(start..self.cursor));
                    };
                    self.cursor += 1;
                    self.advance_while(|c| !c.eq(&b'\n') && c.is_ascii_whitespace());
                }
            }
        }
        Ok(result)
    }
}
