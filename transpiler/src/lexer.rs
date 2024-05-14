use crate::{ErrorSource, TranspilationError};
use crate::parser::{Token, TokenType};

#[derive(Debug)]
pub struct LexingError {
    pub pos: usize,
    pub message: String
}

impl Into<TranspilationError> for &LexingError {
    fn into(self) -> TranspilationError {
        TranspilationError {
            source: ErrorSource::Lexing,
            message: self.message.clone(),
        }
    }
}

struct Lexer<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    errors: Vec<LexingError>,

    start: usize,
    current: usize
}

pub fn scan(source: &str) -> Result<Vec<Token>, Vec<LexingError>> {

    let mut d = Lexer {
        source,
        tokens: Vec::new(),
        errors: Vec::new(),
        start: 0,
        current: 0
    };

    while !d.at_end() {
        d.start = d.current;
        d.scan_token();
    }

    d.tokens.push(Token { typ: TokenType::EOF, lexeme: "".to_string() });

    if d.errors.len() > 0 { Err(d.errors) }
    else { Ok(d.tokens) }
}

impl Lexer<'_> {
    fn scan_token(&mut self) {

        let c = self.advance();
        match c {
            '(' => self.add_token(TokenType::LeftParen),
            ')' => self.add_token(TokenType::RightParen),
            '=' => self.add_token(TokenType::Equal),

            '!' => {
                if self.match_this('=') {
                    self.add_token(TokenType::BangEqual)
                } else { self.error("Unexpected character. Expected '='") };
            },
            '<' => {
                let typ = if self.match_this('=') { TokenType::LessEqual } else { TokenType::Less };
                self.add_token(typ)
            },
            '>' => {
                let typ = if self.match_this('=') { TokenType::GreaterEqual } else { TokenType::Greater };
                self.add_token(typ)
            },

            '\t' => { }
            '\n' => { },
            ' ' => { },

            '"' => self.string(),

            c => {
                if c.is_ascii_digit() { self.number() }
                else if c.is_ascii_alphabetic() { self.identifier() }
                else {
                    self.error("Unexpected character.")
                }
            }
        }
    }

    fn at_end(&self) -> bool {
        self.current >= self.source.chars().count()
    }

    fn advance(&mut self) -> char {
        let c = self.source.chars().nth(self.current).unwrap();
        self.current += 1;
        c
    }

    fn match_this(&mut self, expected: char) -> bool {
        if self.at_end() { return true }
        if !self.source.chars().nth(self.current).is_some_and(|c| { c == expected })  { return false }

        self.current += 1;
        return true
    }

    fn peek(&self) -> char {
        if self.at_end() { return '\0' }
        return self.source.chars().nth(self.current).unwrap()
    }

    fn peek_next(&self) -> char {
        if self.current + 2 > self.source.chars().count() { return '\0'; }
        self.source.chars().nth(self.current + 1).unwrap()
    }

    fn string(&mut self) {
        while self.peek() != '"' && !self.at_end() {
            self.advance();
        }

        if self.at_end() {
            self.error("Unterminated string.");
            return;
        }

        self.advance();

        let str = self.source.chars().skip(self.start + 1).take((self.current - 1) - (self.start + 1)).collect();
        self.add_token(TokenType::String(str))
    }

    fn number(&mut self) {
        let mut float = false;

        while self.peek().is_ascii_digit() { self.advance(); }

        if self.peek() == '.' && self.peek_next().is_ascii_digit() {
            self.advance();

            while self.peek().is_ascii_digit() { self.advance(); }

            float = true;
        }

        let str: String = self.source.chars().skip(self.start).take(self.current - self.start).collect();
        if float {
            let res = str.parse::<f32>();
            if let Ok(f) = res {
                self.add_token(TokenType::Float(f))
            } else {
                self.error("Invalid number literal.");
            }
        } else {
            let res = str.parse::<i32>();
            if let Ok(i) = res {
                self.add_token(TokenType::Int(i))
            } else {
                self.error("Invalid number literal.");
            }
        }
    }

    fn identifier(&mut self) {
        while self.peek().is_ascii_alphanumeric() || self.peek() == '_' {
            self.advance();
        }

        let text: String = self.source.chars().skip(self.start).take(self.current - self.start).collect();

        let typ = match text.as_str() {
            "and" => TokenType::And,
            "false" => TokenType::False,
            "null" => TokenType::Null,
            "or" => TokenType::Or,
            "true" => TokenType::True,
            _ => TokenType::Identifier
        };

        self.add_token(typ)
    }

    fn add_token(&mut self, typ: TokenType) {
        let str: String = self.source.chars().skip(self.start).take(self.current - self.start).collect();
        self.tokens.push(Token { typ, lexeme: str })
    }

    fn error(&mut self, message: &str) {
        self.errors.push(LexingError { pos: self.current, message: message.to_string() })
    }
}