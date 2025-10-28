// src/main.rs

use clap::Parser;
use std::fs;
use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

/// Kompiler untuk bahasa pemrograman AHA!
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// File sumber AHA! yang akan dikompilasi
    #[arg(short, long)]
    file: String,
}

fn main() {
    let args = Args::parse();
    println!("--- KOMPILER AHA! ---");
    println!("Membaca file: {}", args.file);

    let contents = fs::read_to_string(&args.file)
        .expect("Gagal membaca file.");

    // 1. LEXING
    println!("\n[1] LEXING...");
    let lexer = Lexer::new(contents);

    // 2. PARSING
    println!("[2] PARSING...");
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    if !parser.errors.is_empty() {
        eprintln!("\n[ERROR] Parsing gagal dengan {} error:", parser.errors.len());
        for error in parser.errors {
            eprintln!("- {}", error);
        }
        return;
    }
    println!("Parsing berhasil!");

    // 3. CODE GENERATION
    println!("[3] CODE GENERATION...");
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    
    if let Err(e) = codegen.compile(&program) {
        eprintln!("\n[ERROR] Code generation gagal: {}", e);
        return;
    }
    println!("Kode LLVM IR berhasil dihasilkan!\n");

    // 4. OUTPUT
    println!("--- LLVM IR OUTPUT ---");
    codegen.print_llvm_ir();
    println!("----------------------\n");

    // 5. EKSEKUSI (JIT)
    println!("[4] EKSEKUSI (JIT)...");
    match codegen.run_jit() {
        Ok(result) => println!("Program berhasil dijalankan. Hasil: {}", result),
        Err(e) => eprintln!("[ERROR] Gagal menjalankan program: {}", e),
    }
}