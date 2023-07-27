#[derive(Debug)]
enum Token {
    Comment(String),
    Quoted(String),
    Word(String),
}

impl Token {
    fn get_string(&self) -> String {
        match self {
            Token::Comment(c) => c.to_string(),
            Token::Quoted(c) => c.to_string(),
            Token::Word(c) => c.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct Mapping {
    to: String,
    token_index: usize,
}

impl PartialEq for Mapping {
    fn eq(&self, other: &Self) -> bool {
        self.token_index.eq(&other.token_index)
    }
}

#[derive(Clone)]
struct Enricher {
    mappings: Vec<Mapping>,
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
                let s = take_until_escaped(ch, &mut it);
                result.push(Token::Quoted(s));
            }
            ' ' => continue,
            _ => {
                let mut s = take_until_escaped(' ', &mut it);
                s.insert(0, ch);
                result.push(Token::Word(s));
            }
        }
    }
    result
}

fn read_word(whitespace: char, it: &mut std::str::Chars<'_>) -> Option<String> {
    let quotes = ['\'', '"'];
    // Skip leading spaces
    let ch = it.find(|c| c.ne(&' '))?;
    if quotes.contains(&ch) {
        let quoted = take_until_escaped(ch, it);
        let n = it.next();
        if n.is_none() || n.is_some_and(|c| c == whitespace) {
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
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
fn get_mappings(tokens: &[Token], comment: &str) -> (Vec<Mapping>, String) {
    let quotes: &[_] = &['\'', '"'];
    let tokens = tokens
        .iter()
        .map(|t| t.get_string().trim_matches(quotes).to_owned())
        .collect::<Vec<_>>();
    let mut mappings = vec![];
    let mut it = comment.chars();
    loop {
        let clone = it.clone();
        let from = read_word(':', &mut it);
        if let Some(from) = from {
            let to = read_word(' ', &mut it);
            if let Some(to) = to {
                // Ignore mappings which are not part of the token set
                if let Some(token_index) = tokens.iter().position(|t| t.eq(&from)) {
                    mappings.push(Mapping {
                        to,
                        token_index,
                    })
                }
                continue;
            }
        }
        return (mappings, clone.collect());
    }
}

fn get_captures(s: &str) -> Vec<usize> {
    let r = regex::Regex::new("\\$\\((\\d+)\\)").unwrap();
    r.captures_iter(s)
        .map(|c| c.get(1).unwrap().as_str().parse().unwrap())
        .collect()
}

impl Enricher {
    fn new(mappings: Vec<Mapping>) -> Self {
        Self { mappings }
    }
    fn expand_mappings(self, tokens: &[Token]) -> Vec<String> {
        let mut expansion_order: Vec<Mapping> = vec![];

        fn add_dependents(
            mapping: &Mapping,
            mappings: Vec<&Mapping>,
            expansion_order: &mut Vec<Mapping>,
        ) {
            // A mapping should be expanded at least a early as it appears
            if expansion_order.contains(mapping) {
                return;
            }
            // Remove the mapping we just added
            let mappings = mappings
                .into_iter()
                .filter(|m| m.token_index != mapping.token_index)
                .collect::<Vec<_>>();

            // Anything this mapping depends on should also be expanded
            let captures = get_captures(&mapping.to);
            for c in captures {
                if let Some(dep) = mappings.iter().find(|m| m.token_index == c) {
                    add_dependents(dep, mappings.clone(), expansion_order);
                }
            }
            expansion_order.push(mapping.clone());
        }

        for mapping in self.mappings.iter() {
            add_dependents(
                mapping,
                self.mappings.iter().collect(),
                &mut expansion_order,
            );
        }

        let mut expansions: Vec<Option<String>> = vec![None; tokens.len()];

        for mapping in expansion_order.into_iter() {
            let captures = get_captures(&mapping.to);
            let mut new_to = mapping.to;
            for c in captures {
                if c >= expansions.len() {
                    continue;
                }
                let pattern = format!("$({})", c);
                if let Some(ref expanded) = expansions[c] {
                    new_to = new_to.replace(&pattern, expanded);
                } else if let Some(unexpanded) = self.mappings.iter().find(|m| m.token_index == c) {
                    new_to = new_to.replace(&pattern, &unexpanded.to);
                } else {
                    new_to = new_to.replace(&pattern, &tokens[c].get_string());
                }
            }
            expansions[mapping.token_index] = Some(new_to.clone());
        }

        for i in 0..expansions.len() {
            if expansions[i].is_none() {
                expansions[i] = Some(tokens[i].get_string());
            }
        }

        expansions
            .into_iter()
            .map(|option| option.unwrap())
            .collect()
    }
    fn enrich(mappings: Vec<Mapping>, text: String, tokens: &[Token]) -> String {
        let enricher = Self::new(mappings);
        let expansions = enricher.expand_mappings(tokens);
        let mut result = text.clone();
        let r = regex::Regex::new("\\$\\((\\d+)\\)").unwrap();
        let mut captures = r.captures_iter(&text).collect::<Vec<_>>();
        while let Some(c) = captures.pop() {
            let token_index: usize = c.get(1).unwrap().as_str().parse().unwrap();
            if let Some(replacement) = &expansions.get(token_index) {
                result.replace_range(c.get(0).unwrap().range(), replacement);
            }
        }
        result
    }
}

fn expand(tokens: &[Token], comment: &str) -> String {
    let (mappings, text) = get_mappings(tokens, comment);
    // let indexed_pairs: (usize, )
    Enricher::enrich(mappings, text, tokens)
}

// Apply a series of expansion rules to the input to make a nice, representable output string.
pub fn enrich(s: &str) -> String {
    let mut tokens = tokenize(s);
    if let Some(Token::Comment(_)) = tokens.last() {
        let comment = tokens.pop().unwrap().get_string();
        expand(&tokens, &comment)
    } else {
        tokens
            .into_iter()
            .map(|t| t.get_string())
            .collect::<Vec<String>>()
            .join(" ")
    }
}
