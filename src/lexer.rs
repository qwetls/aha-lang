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

    // Baca karakter berikutnya dari input
    fn read_char(&mut self) {
        if self.read_position >= self.input.len() {
            self.ch = '\0'; // Karakter null untuk EOF
        } else {
            self.ch = self.input[self.read_position];
        }
        self.position = self.read_position;
        self.read_position += 1;
        self.column += 1;
    }

    // Lihat karakter di posisi berikutnya tanpa memajukan pointer
    fn peek_char(&self) -> char {
        if self.read_position >= self.input.len() {
            '\0'
        } else {
            self.input[self.read_position]
        }
    }

    // Lewati whitespace dan update line/column
    fn skip_whitespace(&mut self) {
        while self.ch == ' ' || self.ch == '\t' || self.ch == '\r' || self.ch == '\n' {
            if self.ch == '\n' {
                self.line += 1;
                self.column = 0;
            }
            self.read_char();
        }
    }

    // Baca identifier atau keyword
    fn read_identifier(&mut self) -> String {
        let position = self.position;
        while self.ch.is_alphabetic() || self.ch == '_' {
            self.read_char();
        }
        self.input[position..self.position].iter().collect()
    }

    // Baca angka (hanya integer untuk saat ini)
    fn read_number(&mut self) -> String {
        let position = self.position;
        while self.ch.is_digit(10) {
            self.read_char();
        }
        self.input[position..self.position].iter().collect()
    }

    // Cek apakah identifier adalah keyword
    fn lookup_identifier(&self, ident: &str) -> TokenType {
        match ident {
            "fn" => TokenType::Fn,
            "let" => TokenType::Let,
            "true" => TokenType::True, // Akan kita tambahkan di ast.rs
            "false" => TokenType::False, // Akan kita tambahkan di ast.rs
            "if" => TokenType::If, // Akan kita tambahkan di ast.rs
            "else" => TokenType::Else, // Akan kita tambahkan di ast.rs
            "return" => TokenType::Return, // Akan kita tambahkan di ast.rs
            _ => TokenType::Identifier,
        }
    }

    // Fungsi utama untuk mendapatkan token berikutnya
    pub fn next_token(&mut self) -> Token {
        let tok: Token;

        self.skip_whitespace();

        let line = self.line;
        let column = self.column;

        match self.ch {
            '=' => {
                if self.peek_char() == '=' {
                    self.read_char();
                    let literal = format!("{}{}", self.ch, self.ch);
                    tok = Token::new(TokenType::Eq, literal, line, column);
                } else {
                    tok = Token::new(TokenType::Assign, self.ch.to_string(), line, column);
                }
            }
            '!' => {
                if self.peek_char() == '=' {
                    self.read_char();
                    let literal = format!("{}{}", self.ch, self.ch);
                    tok = Token::new(TokenType::NotEq, literal, line, column);
                } else {
                    tok = Token::new(TokenType::Bang, self.ch.to_string(), line, column);
                }
            }
            '+' => tok = Token::new(TokenType::Plus, self.ch.to_string(), line, column),
            '-' => tok = Token::new(TokenType::Minus, self.ch.to_string(), line, column),
            '*' => tok = Token::new(TokenType::Asterisk, self.ch.to_string(), line, column),
            '/' => tok = Token::new(TokenType::Slash, self.ch.to_string(), line, column),
            '<' => tok = Token::new(TokenType::LT, self.ch.to_string(), line, column),
            '>' => tok = Token::new(TokenType::GT, self.ch.to_string(), line, column),
            ',' => tok = Token::new(TokenType::Comma, self.ch.to_string(), line, column),
            ';' => tok = Token::new(TokenType::Semicolon, self.ch.to_string(), line, column),
            '(' => tok = Token::new(TokenType::LeftParen, self.ch.to_string(), line, column),
            ')' => tok = Token::new(TokenType::RightParen, self.ch.to_string(), line, column),
            '{' => tok = Token::new(TokenType::LeftBrace, self.ch.to_string(), line, column),
            '}' => tok = Token::new(TokenType::RightBrace, self.ch.to_string(), line, column),
            '\0' => tok = Token::new(TokenType::Eof, "".to_string(), line, column),
            _ => {
                if self.ch.is_alphabetic() {
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