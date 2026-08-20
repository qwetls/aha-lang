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

/// Check if a statement is a public item (pub function or pub struct).
fn is_pub_item(stmt: &Statement) -> bool {
    match stmt {
        Statement::Expression(ast_expr) => {
            if let crate::ast::Expression::Function(func) = ast_expr {
                func.is_pub
            } else {
                false
            }
        }
        Statement::Struct(s) => s.is_pub,
        _ => false,
    }
}

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
    /// Two-phase: first parse imports to collect struct names, then parse
    /// main file with those names available for struct literal parsing.
    pub fn compile(&self, main_path: &str) -> Result<Program, Vec<CompileError>> {
        let mut visited = HashSet::new();
        let mut all_statements = Vec::new();
        let mut all_struct_names = HashSet::new();
        let mut errors = Vec::new();

        // Phase 1: resolve and parse all imports, collect their struct names
        let resolved_main = self.resolve_path(main_path);
        let main_contents = match std::fs::read_to_string(&resolved_main) {
            Ok(c) => c,
            Err(e) => {
                return Err(vec![CompileError {
                    message: format!("Failed to read file '{}': {}", main_path, e),
                    file: resolved_main.to_string_lossy().to_string(),
                }]);
            }
        };

        // Extract imports from main file (parse it temporarily to get use statements)
        let main_lexer = Lexer::new(main_contents.clone());
        let mut main_pre_parser = Parser::new(main_lexer);
        let main_pre_program = main_pre_parser.parse_program();
        let mut main_imports = Vec::new();
        for stmt in &main_pre_program.statements {
            if let Statement::Import(import) = stmt {
                main_imports.push(import.path.clone());
            }
        }

        // Recursively parse imports and collect their struct names
        for import_path in &main_imports {
            self.compile_file(import_path, &mut visited, &mut all_statements, &mut all_struct_names, &mut errors);
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        // Phase 2: parse main file with known struct names from imports
        let main_lexer2 = Lexer::new(main_contents);
        let mut main_parser = Parser::with_structs(main_lexer2, all_struct_names);
        let main_program = main_parser.parse_program();

        if !main_parser.errors.is_empty() {
            for err in &main_parser.errors {
                errors.push(CompileError {
                    message: err.clone(),
                    file: resolved_main.to_string_lossy().to_string(),
                });
            }
            return Err(errors);
        }

        // Append main file statements (skip imports, already handled)
        for stmt in &main_program.statements {
            if !matches!(stmt, Statement::Import(_)) {
                all_statements.push(stmt.clone());
            }
        }

        Ok(Program { statements: all_statements })
    }

    /// Recursively compile a file and its imports.
    /// Appends non-import statements to `all_statements`.
    /// Collects struct names into `all_struct_names`.
    fn compile_file(
        &self,
        file_path: &str,
        visited: &mut HashSet<String>,
        all_statements: &mut Vec<Statement>,
        all_struct_names: &mut HashSet<String>,
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

        // Parse — use known struct names from previously parsed imports
        let lexer = Lexer::new(contents);
        let mut parser = Parser::with_structs(lexer, all_struct_names.clone());
        let program = parser.parse_program();

        // Collect struct names discovered during parsing
        for name in parser.get_struct_names() {
            all_struct_names.insert(name.clone());
        }

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
            self.compile_file(import_path, visited, all_statements, all_struct_names, errors);
        }

        // Append this file's own statements after imports.
        // Visibility filter: only pub items from imported files are exposed
        // to the importer. Each file's own code still sees all its items
        // because this file was parsed independently before merging.
        for stmt in file_stmts {
            if is_pub_item(&stmt) {
                all_statements.push(stmt);
            }
            // ponytail: non-pub items from imports are dropped. If a pub fn
            // in an imported file calls a non-pub helper in the same file,
            // the helper must also be pub. Add cross-file private access
            // when module-level scoping lands.
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
