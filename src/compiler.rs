// src/compiler.rs
//
// Multi-file compilation orchestrator for AHA! Lang.
// Handles `use "file"` statements by recursively parsing imported files
// and merging their ASTs into a single compilation unit.

use crate::ast::{ImportStatement, Program, Statement};
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Errors encountered during multi-file compilation.
#[derive(Debug)]
pub struct CompileError {
    pub message: String,
    pub file: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.file, self.message)
    }
}

/// Multi-file compiler: resolves `use` statements, parses all files,
/// and merges them into a single `Program` for codegen.
pub struct Compiler {
    search_dirs: Vec<PathBuf>,
}

impl Compiler {
    /// Create a new compiler with search directories for resolving imports.
    /// `search_dirs` is typically the directory containing the main file.
    pub fn new(search_dirs: Vec<PathBuf>) -> Self {
        Compiler { search_dirs }
    }

    /// Compile a main file and all its imports into a single merged Program.
    /// Returns the merged Program or a list of errors.
    pub fn compile(&self, main_path: &str) -> Result<Program, Vec<CompileError>> {
        let mut visited = HashSet::new();
        let mut all_statements = Vec::new();
        let mut errors = Vec::new();

        self.compile_file(main_path, &mut visited, &mut all_statements, &mut errors);

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(Program { statements: all_statements })
    }

    /// Recursively compile a file and its imports.
    /// Appends non-import statements to `all_statements`.
    fn compile_file(
        &self,
        file_path: &str,
        visited: &mut HashSet<String>,
        all_statements: &mut Vec<Statement>,
        errors: &mut Vec<CompileError>,
    ) {
        // Resolve the file path
        let resolved = self.resolve_path(file_path);
        let resolved_str = resolved.to_string_lossy().to_string();

        // Cycle detection
        if visited.contains(&resolved_str) {
            return;
        }
        visited.insert(resolved_str.clone());

        // Read the file
        let contents = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => {
                errors.push(CompileError {
                    message: format!("Failed to read file '{}': {}", file_path, e),
                    file: resolved_str,
                });
                return;
            }
        };

        // Parse
        let lexer = Lexer::new(contents);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program();

        if !parser.errors.is_empty() {
            for err in &parser.errors {
                errors.push(CompileError {
                    message: err.clone(),
                    file: resolved_str.clone(),
                });
            }
            return;
        }

        // Extract imports and collect other statements
        let mut imports = Vec::new();
        let mut file_stmts = Vec::new();
        for stmt in &program.statements {
            match stmt {
                Statement::Import(import) => {
                    imports.push(import.path.clone());
                }
                _ => {
                    file_stmts.push(stmt.clone());
                }
            }
        }

        // Recursively compile imported files (imports come first in merge order)
        for import_path in &imports {
            self.compile_file(import_path, visited, all_statements, errors);
        }

        // Append this file's own statements after imports.
        for stmt in file_stmts {
            all_statements.push(stmt);
        }
    }

    /// Resolve a `use` path to an absolute file path.
    /// Tries: `<dir>/<path>.aha`, `<dir>/<path>/mod.aha`
    fn resolve_path(&self, use_path: &str) -> PathBuf {
        let relative = Path::new(use_path);

        for dir in &self.search_dirs {
            // Try `<dir>/<path>.aha`
            let candidate = dir.join(format!("{}.aha", use_path));
            if candidate.exists() {
                return candidate;
            }
            // Try `<dir>/<path>/mod.aha` (directory module)
            let candidate_mod = dir.join(use_path).join("mod.aha");
            if candidate_mod.exists() {
                return candidate_mod;
            }
            // Try as-is (already has extension)
            let candidate_raw = dir.join(relative);
            if candidate_raw.exists() {
                return candidate_raw;
            }
        }

        // Fallback: return first search_dir + path.aha
        if let Some(dir) = self.search_dirs.first() {
            dir.join(format!("{}.aha", use_path))
        } else {
            PathBuf::from(format!("{}.aha", use_path))
        }
    }

    /// Get the directory containing a file path.
    pub fn parent_dir(file_path: &str) -> PathBuf {
        Path::new(file_path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
