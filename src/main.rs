// src/main.rs

use clap::Parser;
use std::fs;
use aha_lang::lexer::Lexer;
use aha_lang::parser::Parser as AhaParser;
use aha_lang::codegen::CodeGenerator;
use aha_lang::compiler::Compiler;
use inkwell::context::Context;

/// AHA! Lang Compiler v1.5
#[derive(Parser, Debug)]
#[command(author = "AHA! Lang Team", version = "1.5.0", about = "AHA! Lang Compiler", long_about = None)]
struct Args {
    /// Source file to compile
    #[arg(short, long)]
    file: String,

    /// Save LLVM IR to file
    #[arg(long)]
    emit_ir: Option<String>,

    /// Compile to native executable (AOT)
    #[arg(long)]
    emit_exe: Option<String>,
}

fn main() {
    let args = Args::parse();
    println!("--- AHA! COMPILER v1.5 ---");
    println!("Reading file: {}", args.file);

    // 0. RESOLVE IMPORTS (multi-file compilation)
    println!("\n[0] RESOLVING IMPORTS...");
    let search_dir = Compiler::parent_dir(&args.file);
    let compiler = Compiler::new(vec![search_dir]);

    let program = match compiler.compile(&args.file) {
        Ok(program) => {
            println!("Imports resolved!");
            program
        }
        Err(errors) => {
            eprintln!("\n[ERROR] Compilation failed with {} error(s):", errors.len());
            for error in &errors {
                eprintln!("- {}", error);
            }
            return;
        }
    };

    // 1. LEXING & PARSING (already done during import resolution)
    println!("[1] LEXING & PARSING...");

    // For single-file programs (no imports), parse directly for better error messages
    // For multi-file programs, the Compiler already parsed everything
    let program = if program.statements.is_empty() {
        // This shouldn't happen, but handle it
        program
    } else {
        program
    };

    println!("Parsing successful!");

    // 2. CODE GENERATION
    println!("[2] CODE GENERATION...");
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context);

    if let Err(e) = codegen.compile(&program) {
        eprintln!("\n[ERROR] Code generation failed: {}", e);
        return;
    }
    println!("LLVM IR generated successfully!\n");

    // 3. OUTPUT & EMIT IR
    println!("--- LLVM IR OUTPUT ---");
    codegen.print_llvm_ir();
    println!("----------------------\n");

    // Save IR to file if requested
    if let Some(ref ir_file) = args.emit_ir {
        let ir_string = codegen.get_llvm_ir();
        if let Err(e) = fs::write(ir_file, ir_string) {
            eprintln!("[ERROR] Failed to save IR: {}", e);
        } else {
            println!("LLVM IR saved to: {}", ir_file);
        }
    }

    // 4. AOT COMPILATION (--emit-exe)
    if let Some(ref output_path) = args.emit_exe {
        println!("[3] AOT COMPILATION...");

        // Rename user's main → __aha_main, add C-compatible wrapper
        codegen.rename_main("__aha_main");
        codegen.add_c_main_wrapper();

        // Emit object file to temp location
        let obj_path = std::env::temp_dir().join("aha_output.o");
        if let Err(e) = codegen.emit_object_file(&obj_path) {
            eprintln!("[ERROR] Failed to emit object file: {}", e);
            return;
        }
        println!("Object file: {}", obj_path.display());

        // Link with cc
        let status = std::process::Command::new("cc")
            .arg("-o").arg(output_path)
            .arg(&obj_path)
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("Native executable: {}", output_path);
                let _ = fs::remove_file(&obj_path);
            }
            Ok(s) => {
                eprintln!("[ERROR] Linker failed with status: {}", s);
                eprintln!("Object file preserved at: {}", obj_path.display());
            }
            Err(e) => {
                eprintln!("[ERROR] Failed to run linker (cc): {}", e);
                eprintln!("Object file preserved at: {}", obj_path.display());
            }
        }
        return;
    }

    // 5. EXECUTION (JIT)
    println!("[3] EXECUTION (JIT)...");
    match codegen.run_jit() {
        Ok(result) => println!("Program executed successfully. Result: {}", result),
        Err(e) => eprintln!("[ERROR] Failed to execute program: {}", e),
    }
}
