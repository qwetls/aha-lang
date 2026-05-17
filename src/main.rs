// src/main.rs

use clap::Parser;
use std::fs;
use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser as AhaParser;
use aha_lang::codegen::CodeGenerator;
use inkwell::context::Context;

/// AHA! Lang Compiler v1.3
#[derive(Parser, Debug)]
#[command(author = "AHA! Lang Team", version = "1.3.0", about = "AHA! Lang Compiler", long_about = None)]
struct Args {
    /// Source file to compile
    #[arg(short, long)]
    file: String,

    /// Save LLVM IR to file
    #[arg(long)]
    emit_ir: Option<String>,
}

fn main() {
    let args = Args::parse();
    println!("--- AHA! COMPILER v1.3 ---");
    println!("Reading file: {}", args.file);

    let contents = fs::read_to_string(&args.file)
        .expect("Failed to read file.");

    // 1. LEXING
    println!("\n[1] LEXING...");
    let lexer = Lexer::new(contents);

    // 2. PARSING
    println!("[2] PARSING...");
    let mut parser = AhaParser::new(lexer);
    let program = parser.parse_program();

    if !parser.errors.is_empty() {
        eprintln!("\n[ERROR] Parsing failed with {} error(s):", parser.errors.len());
        for error in parser.errors {
            eprintln!("- {}", error);
        }
        return;
    }
    println!("Parsing successful!");

    // 3. CODE GENERATION
    println!("[3] CODE GENERATION...");
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);
    
    if let Err(e) = codegen.compile(&program) {
        eprintln!("\n[ERROR] Code generation failed: {}", e);
        return;
    }
    println!("LLVM IR generated successfully!\n");

    // 4. OUTPUT & EMIT IR
    println!("--- LLVM IR OUTPUT ---");
    codegen.print_llvm_ir();
    println!("----------------------\n");

    // Save IR to file if requested
    if let Some(ir_file) = args.emit_ir {
        let ir_string = codegen.get_llvm_ir();
        if let Err(e) = fs::write(&ir_file, ir_string) {
            eprintln!("[ERROR] Failed to save IR: {}", e);
        } else {
            println!("LLVM IR saved to: {}", ir_file);
        }
    }

    // 5. EXECUTION (JIT)
    println!("[4] EXECUTION (JIT)...");
    match codegen.run_jit() {
        Ok(result) => println!("Program executed successfully. Result: {}", result),
        Err(e) => eprintln!("[ERROR] Failed to execute program: {}", e),
    }
}