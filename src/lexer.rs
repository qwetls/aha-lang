// src/lexer.rs

use crate::ast::{Token, TokenType};

pub struct Lexer {
    input: Vec<char>,
    position: usize,
    read_position: usize,
    ch: char,
    line: usize,
    column: usize,
}

impl Lexer {
    pub fn new(input: String) -> Self {
        let mut l = Lexer {
            input: input.chars().collect(),
            position: 0,
            read_position: 0,
            ch: '\0',
            line: 1,
            column: 0,
        };
        l.read_char();
        l
    }

    // Read the next character from input and advance the position
    fn read_char(&mut self) {
        if self.read_position >= self.input.len() {
            self.ch = '\0'; // Null character for EOF
        } else {
            self.ch = self.input[self.read_position];
        }
        self.position = self.read_position;
        self.read_position += 1;
        self.column += 1;
    }

    // Peek at the next character without advancing the position
    fn peek_char(&self) -> char {
        if self.read_position >= self.input.len() {
            '\0'
        } else {
            self.input[self.read_position]
        }
    }

    // Skip whitespace characters and track line/column
    fn skip_whitespace(&mut self) {
        while self.ch == ' ' || self.ch == '\t' || self.ch == '\r' || self.ch == '\n' {
            if self.ch == '\n' {
                self.line += 1;
                self.column = 0;
            }
            self.read_char();
        }
    }

    // Read an identifier or keyword (alphanumeric + underscore)
    fn read_identifier(&mut self) -> String {
        let position = self.position;
        // First char is alphabetic or underscore (checked by caller)
        // Subsequent chars can also be digits (e.g., my_var2)
        while self.ch.is_alphanumeric() || self.ch == '_' {
            self.read_char();
        }
        self.input[position..self.position].iter().collect()
    }

    // Read an integer literal (digits only, for now)
    fn read_number(&mut self) -> String {
        let position = self.position;
        while self.ch.is_digit(10) {
            self.read_char();
        }
        self.input[position..self.position].iter().collect()
    }

    // Read a string literal with escape sequence support (\n, \t, \\, \", \r, \0)
    fn read_string(&mut self) -> String {
        self.read_char(); // Skip opening quote
        let mut result = String::new();
        while self.ch != '"' && self.ch != '\0' {
            if self.ch == '\\' {
                self.read_char(); // Skip backslash
                match self.ch {
                    'n' => result.push('\n'),
                    't' => result.push('\t'),
                    'r' => result.push('\r'),
                    '\\' => result.push('\\'),
                    '"' => result.push('"'),
                    '0' => result.push('\0'),
                    _ => {
                        // Unknown escape — keep as-is
                        result.push('\\');
                        result.push(self.ch);
                    }
                }
            } else {
                result.push(self.ch);
            }
            self.read_char();
        }
        self.read_char(); // Skip closing quote
        result
    }

    // M-03: Skip multi-line comments /* ... */ (supports nested newlines)
    fn skip_block_comment(&mut self) {
        self.read_char(); // Skip '*' (already consumed '/')
        loop {
            if self.ch == '\0' {
                break; // Unterminated comment — reached EOF
            }
            if self.ch == '\n' {
                self.line += 1;
                self.column = 0;
            }
            if self.ch == '*' && self.peek_char() == '/' {
                self.read_char(); // Skip '*'
                self.read_char(); // Skip '/'
                break;
            }
            self.read_char();
        }
    }

    // Check if an identifier is a reserved keyword
    fn lookup_identifier(&self, ident: &str) -> TokenType {
        match ident {
            "fn" => TokenType::Fn,
            "let" => TokenType::Let,
            "true" => TokenType::True,
            "false" => TokenType::False,
            "if" => TokenType::If,
            "else" => TokenType::Else,
            "return" => TokenType::Return,
            "while" => TokenType::While,
            "for" => TokenType::For,
            "in" => TokenType::In,
            "break" => TokenType::Break,
            "continue" => TokenType::Continue,
            "struct" => TokenType::Struct,
            "use" => TokenType::Use,
            "pub" => TokenType::Pub,
            "actor" => TokenType::Actor,
            "spawn" => TokenType::Spawn,
            _ => TokenType::Identifier,
        }
    }

    // Main function: get the next token from input
    pub fn next_token(&mut self) -> Token {
        let tok: Token;

        self.skip_whitespace();

        let line = self.line;
        let column = self.column;

        match self.ch {
            '=' => {
                if self.peek_char() == '=' {
                    self.read_char();
                    tok = Token::new(TokenType::Eq, "==".to_string(), line, column);
                } else {
                    tok = Token::new(TokenType::Assign, self.ch.to_string(), line, column);
                }
            }
            '!' => {
                if self.peek_char() == '=' {
                    self.read_char();
                    tok = Token::new(TokenType::NotEq, "!=".to_string(), line, column);
                } else {
                    tok = Token::new(TokenType::Bang, self.ch.to_string(), line, column);
                }
            }
            '+' => tok = Token::new(TokenType::Plus, self.ch.to_string(), line, column),
            '-' => {
                if self.peek_char() == '>' {
                    self.read_char();
                    tok = Token::new(TokenType::Arrow, "->".to_string(), line, column);
                } else {
                    tok = Token::new(TokenType::Minus, self.ch.to_string(), line, column);
                }
            }
            '*' => tok = Token::new(TokenType::Asterisk, self.ch.to_string(), line, column),
            '%' => tok = Token::new(TokenType::Percent, self.ch.to_string(), line, column),
            '&' => {
                if self.peek_char() == '&' {
                    self.read_char();
                    tok = Token::new(TokenType::And, "&&".to_string(), line, column);
                } else {
                    tok = Token::new(TokenType::Illegal, self.ch.to_string(), line, column);
                }
            }
            '|' => {
                if self.peek_char() == '|' {
                    self.read_char();
                    tok = Token::new(TokenType::Or, "||".to_string(), line, column);
                } else {
                    tok = Token::new(TokenType::Illegal, self.ch.to_string(), line, column);
                }
            }
            '/' => {
                if self.peek_char() == '/' {
                    // Single-line comment: skip until end of line
                    while self.ch != '\n' && self.ch != '\0' {
                        self.read_char();
                    }
                    return self.next_token();
                } else if self.peek_char() == '*' {
                    // M-03: Multi-line comment /* ... */
                    self.read_char(); // Skip '/'
                    self.skip_block_comment();
                    return self.next_token();
                } else {
                    tok = Token::new(TokenType::Slash, self.ch.to_string(), line, column);
                }
            }
            '<' => {
                if self.peek_char() == '=' {
                    self.read_char();
                    tok = Token::new(TokenType::LtEq, "<=".to_string(), line, column);
                } else {
                    tok = Token::new(TokenType::LT, self.ch.to_string(), line, column);
                }
            }
            '>' => {
                if self.peek_char() == '=' {
                    self.read_char();
                    tok = Token::new(TokenType::GtEq, ">=".to_string(), line, column);
                } else {
                    tok = Token::new(TokenType::GT, self.ch.to_string(), line, column);
                }
            }
            '.' => {
                if self.peek_char() == '.' {
                    self.read_char();
                    tok = Token::new(TokenType::DotDot, "..".to_string(), line, column);
                } else {
                    tok = Token::new(TokenType::Dot, ".".to_string(), line, column);
                }
            }
            ',' => tok = Token::new(TokenType::Comma, self.ch.to_string(), line, column),
            ';' => tok = Token::new(TokenType::Semicolon, self.ch.to_string(), line, column),
            ':' => {
                if self.peek_char() == ':' {
                    self.read_char();
                    tok = Token::new(TokenType::ColonColon, "::".to_string(), line, column);
                } else {
                    tok = Token::new(TokenType::Colon, self.ch.to_string(), line, column);
                }
            }
            '(' => tok = Token::new(TokenType::LeftParen, self.ch.to_string(), line, column),
            ')' => tok = Token::new(TokenType::RightParen, self.ch.to_string(), line, column),
            '{' => tok = Token::new(TokenType::LeftBrace, self.ch.to_string(), line, column),
            '}' => tok = Token::new(TokenType::RightBrace, self.ch.to_string(), line, column),
            '[' => tok = Token::new(TokenType::LeftBracket, self.ch.to_string(), line, column),
            ']' => tok = Token::new(TokenType::RightBracket, self.ch.to_string(), line, column),
            '\0' => tok = Token::new(TokenType::Eof, "".to_string(), line, column),
            '"' => {
                let literal = self.read_string();
                return Token::new(TokenType::String, literal, line, column);
            }
            _ => {
                if self.ch.is_alphabetic() || self.ch == '_' {
                    let literal = self.read_identifier();
                    let token_type = self.lookup_identifier(&literal);
                    return Token::new(token_type, literal, line, column);
                } else if self.ch.is_digit(10) {
                    let literal = self.read_number();
                    return Token::new(TokenType::Integer, literal, line, column);
                } else {
                    tok = Token::new(TokenType::Illegal, self.ch.to_string(), line, column);
                }
            }
        }

        self.read_char();
        tok
    }
}