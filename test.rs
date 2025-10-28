// src/parser.rs

// ... (kode lainnya tetap sama) ...

impl Parser {
    // ... (fungsi lainnya tetap sama) ...

    fn parse_let_statement(&mut self) -> Option<Statement> {
        // Saat fungsi ini dipanggil, current_token adalah 'Let'.
        // Kita perlu memajukan token untuk mendapatkan identifier.
        self.next_token();

        // Sekarang current_token seharusnya adalah identifier (misal: 'x')
        if !self.current_token_is(TokenType::Identifier) {
            self.errors.push(format!("expected next token to be Identifier, got {:?} instead", self.current_token.type));
            return None;
        }
        let name = Identifier { value: self.current_token.literal.clone() };

        // Sekarang kita perlu memastikan token berikutnya adalah '='
        if !self.expect_peek(TokenType::Assign) {
            return None;
        }

        // Lewati token '=' untuk menuju ke awal ekspresi
        self.next_token();

        // Parse ekspresi di sebelah kanan tanda '='
        let value = self.parse_expression(Precedence::Lowest);

        // Lewati semicolon jika ada
        if self.peek_token_is(TokenType::Semicolon) {
            self.next_token();
        }

        Some(Statement::Let(LetStatement { name, value }))
    }

    // ... (fungsi lainnya tetap sama) ...
}