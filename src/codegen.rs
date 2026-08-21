// src/codegen.rs

use crate::ast;
use crate::ast::{ActorDefinition, SpawnExpression};
use crate::types::{AhaType, TypedValue};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::values::{PointerValue, BasicValueEnum, FunctionValue, BasicMetadataValueEnum};
use inkwell::types::{IntType, StructType};
use std::collections::HashMap;

/// Variable info stored in scope: LLVM pointer + AHA! type
#[derive(Clone, Debug)]
struct VarInfo<'ctx> {
    ptr: PointerValue<'ctx>,
    var_type: AhaType,
    freed: bool,
    is_param: bool,
}

pub struct CodeGenerator<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    scopes: Vec<HashMap<String, VarInfo<'ctx>>>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    /// Tracks return types of user-defined functions
    fn_types: HashMap<String, AhaType>,
    i64_type: IntType<'ctx>,
    /// String struct type: {i8*, i64} (pointer + length)
    string_type: StructType<'ctx>,
    /// List header struct type: {i8*, i64, i64, i64} (data, len, cap, elem_size)
    list_header_type: StructType<'ctx>,
    /// Map header struct type: {i8*, i64, i64, i64, i64} (data, len, cap, key_size, val_size)
    map_header_type: StructType<'ctx>,
    current_function: Option<FunctionValue<'ctx>>,
    /// Stack of (continue_block, break_block) for nested loops
    loop_stack: Vec<(inkwell::basic_block::BasicBlock<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)>,
    /// Inferred parameter types per function: func_name → vec of AhaType
    param_type_map: HashMap<String, Vec<AhaType>>,
    /// Registered struct definitions: struct name → ordered (field name,
    /// declared AhaType) pairs. Field order defines the LLVM layout.
    struct_defs: HashMap<String, Vec<(String, AhaType)>>,
    /// Pre-scanned variable bindings: var_name → AhaType (for struct
    /// variables, so scan_expr_for_calls can resolve param types).
    struct_var_types: HashMap<String, AhaType>,
    /// Synthetic param scope used while scanning a function body, so
    /// calls inside resolve their args against the function's own params.
    scan_scope: Vec<HashMap<String, AhaType>>,
    /// Generic function definitions: name → cloned FunctionLiteral AST.
    /// Populated during predeclare; bodies are compiled lazily per
    /// concrete type at each call site (monomorphization).
    generic_defs: HashMap<String, ast::FunctionLiteral>,
    /// Active generic type-parameter bindings during monomorphized
    /// body compilation: type param name → concrete AhaType.
    type_param_map: HashMap<String, AhaType>,
    /// Registered enum definitions: enum name → variants with payload types.
    /// Each variant is (name, Vec<AhaType>) — empty vec = unit variant.
    enum_defs: HashMap<String, Vec<(String, Vec<AhaType>)>>,
}

impl<'ctx> CodeGenerator<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let module = context.create_module("aha_module");
        let builder = context.create_builder();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(inkwell::AddressSpace::default());
        // String = {i8*, i64}
        let string_type = context.struct_type(&[i8_ptr_type.into(), i64_type.into()], false);
        // List header = {data: i8*, len: i64, cap: i64, elem_size: i64}
        let list_header_type = context.struct_type(
            &[i8_ptr_type.into(), i64_type.into(), i64_type.into(), i64_type.into()],
            false,
        );
        // Map header = {data: i8*, len: i64, cap: i64, key_size: i64, val_size: i64}
        let map_header_type = context.struct_type(
            &[i8_ptr_type.into(), i64_type.into(), i64_type.into(), i64_type.into(), i64_type.into()],
            false,
        );

        CodeGenerator {
            context,
            module,
            builder,
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            fn_types: HashMap::new(),
            i64_type,
            string_type,
            list_header_type,
            map_header_type,
            current_function: None,
            loop_stack: Vec::new(),
            param_type_map: HashMap::new(),
            struct_defs: HashMap::new(),
            struct_var_types: HashMap::new(),
            scan_scope: Vec::new(),
            generic_defs: HashMap::new(),
            type_param_map: HashMap::new(),
            enum_defs: HashMap::new(),
        }
    }

    // --- Scope Management ---

    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    fn lookup_variable(&self, name: &str) -> Option<&VarInfo<'ctx>> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    fn insert_variable(&mut self, name: String, ptr: PointerValue<'ctx>, var_type: AhaType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, VarInfo { ptr, var_type, freed: false, is_param: false });
        }
    }

    /// Mark a variable as a function parameter (excluded from auto-free).
    fn mark_param(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.is_param = true;
                return;
            }
        }
    }

    /// Mark a variable as freed so automatic cleanup won't double-free.
    fn mark_freed(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.freed = true;
                return;
            }
        }
    }

    /// Check if current scope has any unfreed heap-allocated local variables.
    /// Excludes function parameters — they're owned by the caller.
    fn has_heap_locals(&self) -> bool {
        if let Some(scope) = self.scopes.last() {
            scope.values().any(|v| !v.is_param && !v.freed && matches!(
                v.var_type,
                AhaType::Map(_, _) | AhaType::List(_) | AhaType::String
            ))
        } else {
            false
        }
    }

    /// Insert free calls for all heap-allocated local variables directly
    /// into the current basic block (no new blocks created).
    /// Must be called BEFORE the return terminator is built.
    /// `exclude` — variable names to skip (escaped via return).
    fn insert_cleanup_inline(&mut self, exclude: &std::collections::HashSet<String>) {
        if let Some(scope) = self.scopes.last() {
            for (name, var_info) in scope {
                if var_info.is_param || var_info.freed || exclude.contains(name) { continue; }
                match &var_info.var_type {
                    AhaType::Map(_, _) => {
                        if let Some(f) = self.module.get_function("map_free") {
                            if let Ok(handle) = self.builder.build_load(var_info.ptr, "map_handle") {
                                let _ = self.builder.build_call(f, &[handle.into()], "cleanup");
                            }
                        }
                    }
                    AhaType::List(_) => {
                        if let Some(f) = self.module.get_function("list_free") {
                            if let Ok(handle) = self.builder.build_load(var_info.ptr, "list_handle") {
                                let _ = self.builder.build_call(f, &[handle.into()], "cleanup");
                            }
                        }
                    }
                    // ponytail: string_free not yet declared as builtin —
                    // add when string lifetime management is implemented.
                    _ => {}
                }
            }
        }
    }

    /// Insert a free call for a specific variable.
    fn insert_free_for_var(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last() {
            if let Some(var_info) = scope.get(name) {
                if var_info.is_param || var_info.freed { return; }
                match &var_info.var_type {
                    AhaType::Map(_, _) => {
                        if let Some(f) = self.module.get_function("map_free") {
                            if let Ok(handle) = self.builder.build_load(var_info.ptr, "map_cleanup") {
                                let _ = self.builder.build_call(f, &[handle.into()], "cleanup");
                            }
                        }
                        self.mark_freed(name);
                    }
                    AhaType::List(_) => {
                        if let Some(f) = self.module.get_function("list_free") {
                            if let Ok(handle) = self.builder.build_load(var_info.ptr, "list_cleanup") {
                                let _ = self.builder.build_call(f, &[handle.into()], "cleanup");
                            }
                        }
                        self.mark_freed(name);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Pre-scan: find the last statement index where each heap variable is used.
    /// Returns a map of variable name → last-use statement index.
    fn find_last_uses(body: &[ast::Statement]) -> std::collections::HashMap<String, usize> {
        let mut last_uses: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (idx, stmt) in body.iter().enumerate() {
            Self::scan_stmt_uses(stmt, &mut last_uses, idx);
        }
        last_uses
    }

    /// Escape analysis: find all variable names referenced in an expression.
    /// Used to detect which heap variables escape via return.
    fn find_heap_vars_in_expr(expr: &ast::Expression) -> std::collections::HashSet<String> {
        let mut vars = std::collections::HashSet::new();
        Self::collect_var_names(expr, &mut vars);
        vars
    }

    fn collect_var_names(expr: &ast::Expression, vars: &mut std::collections::HashSet<String>) {
        match expr {
            ast::Expression::Identifier(id) => { vars.insert(id.value.clone()); }
            ast::Expression::Infix(infix) => {
                Self::collect_var_names(&infix.left, vars);
                Self::collect_var_names(&infix.right, vars);
            }
            ast::Expression::Prefix(prefix) => { Self::collect_var_names(&prefix.right, vars); }
            ast::Expression::If(if_expr) => {
                Self::collect_var_names(&if_expr.condition, vars);
                Self::collect_block_vars(&if_expr.consequence, vars);
                if let Some(ref alt) = if_expr.alternative { Self::collect_block_vars(alt, vars); }
            }
            ast::Expression::Call(call) => {
                Self::collect_var_names(&call.function, vars);
                for arg in &call.arguments { Self::collect_var_names(arg, vars); }
            }
            ast::Expression::Index(idx_expr) => {
                Self::collect_var_names(&idx_expr.left, vars);
                Self::collect_var_names(&idx_expr.index, vars);
            }
            ast::Expression::FieldAccess(fa) => { Self::collect_var_names(&fa.object, vars); }
            ast::Expression::Assignment(assign) => { Self::collect_var_names(&assign.value, vars); }
            ast::Expression::StructLiteral(sl) => {
                for (_, val) in &sl.fields { Self::collect_var_names(val, vars); }
            }
            ast::Expression::Array(arr) => {
                for elem in &arr.elements { Self::collect_var_names(elem, vars); }
            }
            ast::Expression::Match(m) => {
                Self::collect_var_names(&m.value, vars);
                for arm in &m.arms { Self::collect_var_names(&arm.body, vars); }
            }
            _ => {}
        }
    }

    fn collect_block_vars(block: &ast::BlockStatement, vars: &mut std::collections::HashSet<String>) {
        for stmt in &block.statements {
            match stmt {
                ast::Statement::Expression(es) => Self::collect_var_names(&es.expression, vars),
                ast::Statement::Let(ls) => Self::collect_var_names(&ls.value, vars),
                ast::Statement::Return(ret) => Self::collect_var_names(&ret.return_value, vars),
                _ => {}
            }
        }
    }

    fn scan_stmt_uses(stmt: &ast::Statement, last_uses: &mut std::collections::HashMap<String, usize>, idx: usize) {
        match stmt {
            ast::Statement::Expression(expr_stmt) => {
                Self::scan_expr_uses(&expr_stmt.expression, last_uses, idx);
            }
            ast::Statement::Let(let_stmt) => {
                Self::scan_expr_uses(&let_stmt.value, last_uses, idx);
            }
            ast::Statement::Return(ret) => {
                Self::scan_expr_uses(&ret.return_value, last_uses, idx);
            }
            _ => {}
        }
    }

    fn scan_expr_uses(expr: &ast::Expression, last_uses: &mut std::collections::HashMap<String, usize>, idx: usize) {
        match expr {
            ast::Expression::Identifier(id) => {
                last_uses.insert(id.value.clone(), idx);
            }
            ast::Expression::Infix(infix) => {
                Self::scan_expr_uses(&infix.left, last_uses, idx);
                Self::scan_expr_uses(&infix.right, last_uses, idx);
            }
            ast::Expression::Prefix(prefix) => {
                Self::scan_expr_uses(&prefix.right, last_uses, idx);
            }
            ast::Expression::If(if_expr) => {
                Self::scan_expr_uses(&if_expr.condition, last_uses, idx);
                Self::scan_block_uses(&if_expr.consequence, last_uses, idx);
                if let Some(ref alt) = if_expr.alternative {
                    Self::scan_block_uses(alt, last_uses, idx);
                }
            }
            ast::Expression::While(while_expr) => {
                Self::scan_expr_uses(&while_expr.condition, last_uses, idx);
                Self::scan_block_uses(&while_expr.body, last_uses, idx);
            }
            ast::Expression::For(for_expr) => {
                Self::scan_expr_uses(&for_expr.iterable, last_uses, idx);
                Self::scan_block_uses(&for_expr.body, last_uses, idx);
            }
            ast::Expression::Call(call) => {
                Self::scan_expr_uses(&call.function, last_uses, idx);
                for arg in &call.arguments {
                    Self::scan_expr_uses(arg, last_uses, idx);
                }
            }
            ast::Expression::Index(idx_expr) => {
                Self::scan_expr_uses(&idx_expr.left, last_uses, idx);
                Self::scan_expr_uses(&idx_expr.index, last_uses, idx);
            }
            ast::Expression::FieldAccess(fa) => {
                Self::scan_expr_uses(&fa.object, last_uses, idx);
            }
            ast::Expression::StructLiteral(sl) => {
                for (_, val) in &sl.fields {
                    Self::scan_expr_uses(val, last_uses, idx);
                }
            }
            ast::Expression::Array(arr) => {
                for elem in &arr.elements {
                    Self::scan_expr_uses(elem, last_uses, idx);
                }
            }
            ast::Expression::Assignment(assign) => {
                Self::scan_expr_uses(&assign.value, last_uses, idx);
            }
            ast::Expression::Match(m) => {
                Self::scan_expr_uses(&m.value, last_uses, idx);
                for arm in &m.arms {
                    Self::scan_expr_uses(&arm.body, last_uses, idx);
                }
            }
            _ => {} // literals, module access — no heap var uses
        }
    }

    fn scan_block_uses(block: &ast::BlockStatement, last_uses: &mut std::collections::HashMap<String, usize>, idx: usize) {
        for stmt in &block.statements {
            Self::scan_stmt_uses(stmt, last_uses, idx);
        }
    }

    /// Pre-pass: walk all statements to find call expressions and infer
    /// parameter types. If a string literal is passed as argument N,
    /// param N is marked as String.
    fn scan_call_sites(&mut self, statements: &[ast::Statement]) {
        for stmt in statements {
            match stmt {
                ast::Statement::Expression(expr_stmt) => {
                    self.scan_expr_for_calls(&expr_stmt.expression);
                }
                ast::Statement::Let(let_stmt) => {
                    // Track struct variable bindings so infer_expr_type
                    // can resolve them when the variable is passed as a
                    // function argument. Prefer the explicit annotation
                    // when present (e.g. `let p: Point = ...`).
                    if let Some(ref hint) = let_stmt.type_annotation {
                        if self.struct_defs.contains_key(hint) {
                            self.struct_var_types.insert(
                                let_stmt.name.value.clone(),
                                AhaType::Struct(hint.clone()),
                            );
                        }
                    } else if let ast::Expression::StructLiteral(sl) = &let_stmt.value {
                        self.struct_var_types.insert(
                            let_stmt.name.value.clone(),
                            AhaType::Struct(sl.name.value.clone()),
                        );
                    } else if let ast::Expression::Call(call) = &let_stmt.value {
                        // Track list bindings so a later call site can infer
                        // the param type: `let xs = list_new(); f(xs)` passes
                        // xs as List<Int>, not Int.
                        if let ast::Expression::Identifier(id) = call.function.as_ref() {
                            if id.value == "list_new" {
                                self.struct_var_types.insert(
                                    let_stmt.name.value.clone(),
                                    AhaType::List(Box::new(AhaType::Int)),
                                );
                            } else if id.value == "list_new_string" {
                                self.struct_var_types.insert(
                                    let_stmt.name.value.clone(),
                                    AhaType::List(Box::new(AhaType::String)),
                                );
                            } else if id.value == "map_new" {
                                self.struct_var_types.insert(
                                    let_stmt.name.value.clone(),
                                    AhaType::Map(Box::new(AhaType::Int), Box::new(AhaType::Int)),
                                );
                            } else if id.value == "map_new_string_key" {
                                self.struct_var_types.insert(
                                    let_stmt.name.value.clone(),
                                    AhaType::Map(Box::new(AhaType::String), Box::new(AhaType::Int)),
                                );
                            } else if id.value == "map_new_string_val" {
                                self.struct_var_types.insert(
                                    let_stmt.name.value.clone(),
                                    AhaType::Map(Box::new(AhaType::Int), Box::new(AhaType::String)),
                                );
                            } else if id.value == "map_new_strings" {
                                self.struct_var_types.insert(
                                    let_stmt.name.value.clone(),
                                    AhaType::Map(Box::new(AhaType::String), Box::new(AhaType::String)),
                                );
                            }
                        }
                    }
                    self.scan_expr_for_calls(&let_stmt.value);
                }
                ast::Statement::Return(ret_stmt) => {
                    self.scan_expr_for_calls(&ret_stmt.return_value);
                }
                ast::Statement::Struct(_) => {}
                ast::Statement::Actor(_) => {}
                ast::Statement::Enum(_) => {}
                ast::Statement::Import(_) => {}
            }
        }
    }

    fn scan_expr_for_calls(&mut self, expr: &ast::Expression) {
        match expr {
            ast::Expression::Call(call) => {
                if let ast::Expression::Identifier(id) = call.function.as_ref() {
                    let types: Vec<AhaType> = call.arguments.iter()
                        .map(|arg| self.infer_expr_type(arg))
                        .collect();
                    self.param_type_map.entry(id.value.clone())
                        .and_modify(|existing| {
                            for (i, t) in types.iter().enumerate() {
                                if i < existing.len() {
                                    existing[i] = existing[i].unify_with(t);
                                }
                            }
                        })
                        .or_insert_with(|| types.clone());
                } else if let ast::Expression::ModuleAccess(ma) = call.function.as_ref() {
                    // module::name(args) — same as Identifier scan
                    let types: Vec<AhaType> = call.arguments.iter()
                        .map(|arg| self.infer_expr_type(arg))
                        .collect();
                    self.param_type_map.entry(ma.name.clone())
                        .and_modify(|existing| {
                            for (i, t) in types.iter().enumerate() {
                                if i < existing.len() {
                                    existing[i] = existing[i].unify_with(t);
                                }
                            }
                        })
                        .or_insert_with(|| types.clone());
                }
                for arg in &call.arguments {
                    self.scan_expr_for_calls(arg);
                }
            }
            ast::Expression::Infix(infix) => {
                self.scan_expr_for_calls(&infix.left);
                self.scan_expr_for_calls(&infix.right);
            }
            ast::Expression::Prefix(prefix) => {
                self.scan_expr_for_calls(&prefix.right);
            }
            ast::Expression::If(if_expr) => {
                self.scan_expr_for_calls(&if_expr.condition);
                self.scan_block_for_calls(&if_expr.consequence);
                if let Some(alt) = &if_expr.alternative {
                    self.scan_block_for_calls(alt);
                }
            }
            ast::Expression::While(while_expr) => {
                self.scan_expr_for_calls(&while_expr.condition);
                self.scan_block_for_calls(&while_expr.body);
            }
            ast::Expression::For(for_expr) => {
                self.scan_expr_for_calls(&for_expr.iterable);
                self.scan_block_for_calls(&for_expr.body);
            }
            ast::Expression::Assignment(assign) => {
                self.scan_expr_for_calls(&assign.target);
                self.scan_expr_for_calls(&assign.value);
            }
            ast::Expression::Function(func) => {
                // Push function params into scan_scope so body calls can
                // resolve param types (e.g., `x2(p)` inside `total(p) { ... }`).
                if let Some(name) = &func.name {
                    let params: HashMap<String, AhaType> = func.parameters.iter().enumerate()
                        .map(|(i, p)| {
                            let t = self.param_type_map.get(&name.value)
                                .and_then(|types| types.get(i).cloned())
                                .unwrap_or(AhaType::Int);
                            (p.value.clone(), t)
                        })
                        .collect();
                    self.scan_scope.push(params);
                    self.scan_block_for_calls(&func.body);
                    self.scan_scope.pop();
                } else {
                    self.scan_block_for_calls(&func.body);
                }
            }
            ast::Expression::Array(arr) => {
                for elem in &arr.elements {
                    self.scan_expr_for_calls(elem);
                }
            }
            ast::Expression::Index(idx) => {
                self.scan_expr_for_calls(&idx.left);
                self.scan_expr_for_calls(&idx.index);
            }
            ast::Expression::StructLiteral(struct_lit) => {
                for (_, value_expr) in &struct_lit.fields {
                    self.scan_expr_for_calls(value_expr);
                }
            }
            ast::Expression::Match(m) => {
                self.scan_expr_for_calls(&m.value);
                for arm in &m.arms {
                    self.scan_expr_for_calls(&arm.body);
                }
            }
            _ => {}
        }
    }

    fn scan_block_for_calls(&mut self, block: &ast::BlockStatement) {
        for stmt in &block.statements {
            self.scan_call_sites(std::slice::from_ref(stmt));
        }
    }

    /// LLVM type for an AhaType (function params, returns, allocas).
    fn aha_type_to_llvm_type(&self, t: &AhaType) -> Result<inkwell::types::BasicTypeEnum<'ctx>, String> {
        match t {
            AhaType::String => Ok(self.string_type.into()),
            AhaType::Struct(name) => Ok(self.struct_llvm_type(name)?.into()),
            AhaType::Enum(name) => Ok(self.enum_llvm_type(name)?.into()),
            _ => Ok(self.i64_type.into()),
        }
    }

    /// Resolve a type hint string to AhaType, checking active generic
    /// type-parameter bindings first, then built-in hints, then struct names.
    fn resolve_hint_type(&self, hint: &str) -> AhaType {
        if let Some(t) = self.type_param_map.get(hint) {
            return t.clone();
        }
        // List<T> with a bound type param inside (e.g. List<T> where T=Int):
        // resolve the inner hint recursively, then wrap.
        if let Some(inner) = hint.strip_prefix("List<").and_then(|s| s.strip_suffix('>')) {
            let inner_type = self.resolve_hint_type(inner);
            return AhaType::List(Box::new(inner_type));
        }
        // Map<K,V> with resolved inner types.
        if let Some(inner) = hint.strip_prefix("Map<").and_then(|s| s.strip_suffix('>')) {
            if let Some((k, v)) = inner.split_once(',') {
                let kt = self.resolve_hint_type(k.trim());
                let vt = self.resolve_hint_type(v.trim());
                return AhaType::Map(Box::new(kt), Box::new(vt));
            }
        }
        if let Some(t) = AhaType::from_hint(hint) {
            return t;
        }
        if self.struct_defs.contains_key(hint) {
            return AhaType::Struct(hint.to_string());
        }
        if self.enum_defs.contains_key(hint) {
            return AhaType::Enum(hint.to_string());
        }
        AhaType::Int
    }

    /// Build a function type from a return type enum and param types.
    fn build_fn_type(
        &self,
        return_type: &AhaType,
        param_types: &[inkwell::types::BasicTypeEnum<'ctx>],
    ) -> Result<inkwell::types::FunctionType<'ctx>, String> {
        let meta: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = param_types.iter()
            .map(|p| (*p).into())
            .collect();
        match return_type {
            AhaType::String => Ok(self.string_type.fn_type(&meta, false)),
            AhaType::Struct(name) => {
                let st = self.struct_llvm_type(name)?;
                Ok(st.fn_type(&meta, false))
            }
            AhaType::Enum(name) => {
                let et = self.enum_llvm_type(name)?;
                Ok(et.fn_type(&meta, false))
            }
            _ => Ok(self.i64_type.fn_type(&meta, false)),
        }
    }

    /// Pre-declare all user functions so forward references work.
    /// Creates the LLVM function value with correct param types but
    /// does NOT compile the body — bodies are compiled later.
    /// Generic functions are stored in generic_defs for lazy monomorphization.
    fn predeclare_functions(&mut self, statements: &[ast::Statement]) {
        for stmt in statements {
            if let ast::Statement::Expression(ast::ExpressionStatement {
                expression: ast::Expression::Function(func),
            }) = stmt
            {
                if let Some(name) = &func.name {
                    let func_name = name.value.clone();
                    // Generic functions are stored for lazy monomorphization
                    if !func.type_params.is_empty() {
                        self.generic_defs.insert(func_name, func.clone());
                        continue;
                    }
                    if self.functions.contains_key(&func_name) {
                        continue;
                    }
                    // Don't predeclare main — compile() creates it as the
                    // implicit entry point; compile_function fills the body.
                    if func_name == "main" {
                        continue;
                    }
                    let param_types: Result<Vec<_>, _> = func.parameters.iter().enumerate()
                        .map(|(i, _)| {
                            let t = self.param_type_map.get(&func_name)
                                .and_then(|types| types.get(i).cloned())
                                .unwrap_or(AhaType::Int);
                            self.aha_type_to_llvm_type(&t)
                        })
                        .collect();
                    let Ok(param_types) = param_types else { continue; };
                    let return_type = self.infer_function_return_type(func, &func_name);
                    let Ok(fn_type) = self.build_fn_type(&return_type, &param_types) else { continue; };
                    let function = self.module.add_function(&func_name, fn_type, None);
                    self.functions.insert(func_name.clone(), function);
                    self.fn_types.insert(func_name, return_type);
                }
            }
        }
    }

    /// Infer the AhaType of an expression for the pre-pass.
    fn infer_expr_type(&self, expr: &ast::Expression) -> AhaType {
        match expr {
            ast::Expression::String(_) => AhaType::String,
            ast::Expression::Integer(_) => AhaType::Int,
            ast::Expression::Boolean(_) => AhaType::Bool,
            ast::Expression::Identifier(id) => {
                // Check scan_scope first (for param types inside function bodies
                // during the pre-pass scan), then live variables, then struct_var_types.
                let from_scan = self.scan_scope.last()
                    .and_then(|scope| scope.get(&id.value).cloned());
                let from_var = self.lookup_variable(&id.value)
                    .map(|info| info.var_type.clone());
                let from_struct = self.struct_var_types.get(&id.value).cloned();
                from_scan.or(from_var).or(from_struct).unwrap_or(AhaType::Int)
            }
            ast::Expression::Infix(infix) => {
                let lt = self.infer_expr_type(&infix.left);
                let rt = self.infer_expr_type(&infix.right);
                match infix.operator.as_str() {
                    "+" if lt == AhaType::String || rt == AhaType::String => AhaType::String,
                    "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" => AhaType::Int,
                    _ => AhaType::Int,
                }
            }
            ast::Expression::Prefix(prefix) => {
                if prefix.operator == "!" {
                    AhaType::Bool
                } else {
                    AhaType::Int
                }
            }
            ast::Expression::Call(call) => {
                if let ast::Expression::Identifier(id) = call.function.as_ref() {
                    // Enum variant constructor: return the enum type.
                    if let Some(enum_name) = self.find_enum_for_variant(&id.value) {
                        return AhaType::Enum(enum_name);
                    }
                    // List builtins: preserve the element type of the first
                    // argument so `let xs = list_new(); list_push(xs, ...)`
                    // keeps xs as List<Int> and list_get(xs, i) is Int.
                    if id.value == "list_push" || id.value == "list_push_string" {
                        if let Some(first) = call.arguments.first() {
                            return self.infer_expr_type(first);
                        }
                    }
                    if id.value == "list_get" || id.value == "list_get_string" {
                        if let Some(first) = call.arguments.first() {
                            let list_type = self.infer_expr_type(first);
                            if let AhaType::List(inner) = list_type {
                                return *inner;
                            }
                        }
                        return AhaType::Int;
                    }
                    // Map get: return the value type of the map.
                    if id.value == "map_get" || id.value == "map_get_string" {
                        if let Some(first) = call.arguments.first() {
                            let map_type = self.infer_expr_type(first);
                            if let AhaType::Map(_, v) = map_type {
                                return *v;
                            }
                        }
                        return AhaType::Int;
                    }
                    // Prefer a known return type (fn_types), then fall back
                    // to the builtin len() = Int.
                    if let Some(rt) = self.fn_types.get(&id.value) {
                        return rt.clone();
                    }
                    if id.value == "len" {
                        return AhaType::Int;
                    }
                }
                AhaType::Int
            }
            ast::Expression::StructLiteral(sl) => {
                AhaType::Struct(sl.name.value.clone())
            }
            ast::Expression::FieldAccess(fa) => {
                // Infer the struct type from the object, then look up
                // the field type. For pre-pass, return Int if unknown.
                let obj_type = self.infer_expr_type(&fa.object);
                if let AhaType::Struct(name) = &obj_type {
                    if let Some(fields) = self.struct_defs.get(name) {
                        for (field_name, ft) in fields {
                            if field_name == &fa.field.value {
                                return ft.clone();
                            }
                        }
                    }
                }
                AhaType::Int
            }
            ast::Expression::Match(m) => {
                // Return type of first arm body (all arms must agree).
                if let Some(arm) = m.arms.first() {
                    self.infer_expr_type(&arm.body)
                } else {
                    AhaType::Int
                }
            }
            _ => AhaType::Int,
        }
    }

    /// Infer a function's return type for the pre-declaration pass.
    /// Walks the body looking for the last expression value or an
    /// explicit `return` statement, then types that expression with
    /// the function's own params in scope (so `a + b` is String when
    /// a and b are string params).
    fn infer_function_return_type(&self, func: &ast::FunctionLiteral, func_name: &str) -> AhaType {
        // Prefer explicit return type annotation (e.g. `fn f() -> int`)
        if let Some(ref hint) = func.return_type_hint {
            // Resolve struct/enum names against registries, then fall back to from_hint.
            let ty = if self.struct_defs.contains_key(hint.as_str()) {
                AhaType::Struct(hint.clone())
            } else if self.enum_defs.contains_key(hint.as_str()) {
                AhaType::Enum(hint.clone())
            } else {
                AhaType::from_hint(hint).unwrap_or(AhaType::Int)
            };
            return ty;
        }

        let param_types = self.infer_param_types_immutable(func_name, &func.parameters, &func.param_type_hints);

        // Build a synthetic scope so infer_expr_type_with_scope can resolve params.
        let scope: HashMap<String, AhaType> = func.parameters.iter().enumerate()
            .map(|(i, p)| (p.value.clone(), param_types.get(i).cloned().unwrap_or(AhaType::Int)))
            .collect();

        for stmt in &func.body.statements {
            if let ast::Statement::Return(ret) = stmt {
                return self.infer_expr_type_with_scope(&ret.return_value, &scope);
            }
        }
        // No explicit return — type the last expression statement.
        for stmt in func.body.statements.iter().rev() {
            if let ast::Statement::Expression(expr_stmt) = stmt {
                return self.infer_expr_type_with_scope(&expr_stmt.expression, &scope);
            }
        }
        AhaType::Int
    }

    /// Immutable variant of infer_param_types for the pre-pass (when we
    /// cannot call the &mut self version). Reads from param_type_map.
    fn infer_param_types_immutable(&self, func_name: &str, params: &[ast::Identifier], hints: &[Option<String>]) -> Vec<AhaType> {
        let mut types = vec![AhaType::Int; params.len()];
        for (i, hint) in hints.iter().enumerate() {
            if i < types.len() {
                if let Some(h) = hint {
                    types[i] = if self.enum_defs.contains_key(h.as_str()) {
                        AhaType::Enum(h.clone())
                    } else if self.struct_defs.contains_key(h.as_str()) {
                        AhaType::Struct(h.clone())
                    } else {
                        AhaType::from_hint(h).unwrap_or(AhaType::Int)
                    };
                }
            }
        }
        if let Some(inferred) = self.param_type_map.get(func_name) {
            for (i, t) in inferred.iter().enumerate() {
                if i < types.len() {
                    types[i] = t.clone();
                }
            }
        }
        types
    }

    /// Like infer_expr_type, but with a synthetic local scope (used by
    /// the pre-declaration pass to resolve function params).
    fn infer_expr_type_with_scope(&self, expr: &ast::Expression, scope: &HashMap<String, AhaType>) -> AhaType {
        match expr {
            ast::Expression::String(_) => AhaType::String,
            ast::Expression::Integer(_) => AhaType::Int,
            ast::Expression::Boolean(_) => AhaType::Bool,
            ast::Expression::Identifier(id) => {
                scope.get(&id.value).cloned().unwrap_or(AhaType::Int)
            }
            ast::Expression::ModuleAccess(ma) => {
                scope.get(&ma.name).cloned().unwrap_or(AhaType::Int)
            }
            ast::Expression::Infix(infix) => {
                let lt = self.infer_expr_type_with_scope(&infix.left, scope);
                let rt = self.infer_expr_type_with_scope(&infix.right, scope);
                match infix.operator.as_str() {
                    "+" if lt == AhaType::String || rt == AhaType::String => AhaType::String,
                    "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" => AhaType::Int,
                    _ => AhaType::Int,
                }
            }
            ast::Expression::Prefix(prefix) => {
                if prefix.operator == "!" {
                    AhaType::Bool
                } else {
                    AhaType::Int
                }
            }
            ast::Expression::Call(call) => {
                let call_name = match call.function.as_ref() {
                    ast::Expression::Identifier(id) => Some(id.value.as_str()),
                    ast::Expression::ModuleAccess(ma) => Some(ma.name.as_str()),
                    _ => None,
                };
                if let Some(name) = call_name {
                    if let Some(enum_name) = self.find_enum_for_variant(name) {
                        return AhaType::Enum(enum_name);
                    }
                    if name == "list_push" || name == "list_push_string" {
                        if let Some(first) = call.arguments.first() {
                            return self.infer_expr_type_with_scope(first, scope);
                        }
                    }
                    if name == "list_get" || name == "list_get_string" {
                        if let Some(first) = call.arguments.first() {
                            let list_type = self.infer_expr_type_with_scope(first, scope);
                            if let AhaType::List(inner) = list_type {
                                return *inner;
                            }
                        }
                        return AhaType::Int;
                    }
                    if name == "len" {
                        return AhaType::Int;
                    }
                    if let Some(rt) = self.fn_types.get(name) {
                        return rt.clone();
                    }
                }
                AhaType::Int
            }
            ast::Expression::If(if_expr) => {
                let cons = self.infer_block_return_type(&if_expr.consequence, scope);
                if let Some(alt) = &if_expr.alternative {
                    let alt_t = self.infer_block_return_type(alt, scope);
                    if alt_t == AhaType::String || cons == AhaType::String {
                        return AhaType::String;
                    }
                }
                cons
            }
            ast::Expression::StructLiteral(sl) => {
                AhaType::Struct(sl.name.value.clone())
            }
            ast::Expression::FieldAccess(fa) => {
                let obj_type = self.infer_expr_type_with_scope(&fa.object, scope);
                if let AhaType::Struct(name) = &obj_type {
                    if let Some(fields) = self.struct_defs.get(name) {
                        for (field_name, ft) in fields {
                            if field_name == &fa.field.value {
                                return ft.clone();
                            }
                        }
                    }
                }
                AhaType::Int
            }
            ast::Expression::Assignment(assign) => {
                self.infer_expr_type_with_scope(&assign.value, scope)
            }
            ast::Expression::Match(m) => {
                if let Some(arm) = m.arms.first() {
                    self.infer_expr_type_with_scope(&arm.body, scope)
                } else {
                    AhaType::Int
                }
            }
            _ => AhaType::Int,
        }
    }

    fn infer_block_return_type(&self, block: &ast::BlockStatement, scope: &HashMap<String, AhaType>) -> AhaType {
        for stmt in &block.statements {
            if let ast::Statement::Return(ret) = stmt {
                return self.infer_expr_type_with_scope(&ret.return_value, scope);
            }
        }
        for stmt in block.statements.iter().rev() {
            if let ast::Statement::Expression(expr_stmt) = stmt {
                return self.infer_expr_type_with_scope(&expr_stmt.expression, scope);
            }
        }
        AhaType::Int
    }

    /// Get i8* pointer type (used frequently for strings)
    fn i8_ptr_type(&self) -> inkwell::types::PointerType<'ctx> {
        self.context.i8_type().ptr_type(inkwell::AddressSpace::default())
    }

    /// DIAGNOSTIC: append a marker to /tmp/aha_diag.log (survives SIGSEGV
    /// where stderr capture is lost). Remove once List<T> lands.
    fn diag_mark(msg: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/aha_diag.log")
        {
            let _ = writeln!(f, "{}", msg);
        }
    }

    pub fn compile(&mut self, program: &ast::Program) -> Result<(), String> {
        let _ = std::fs::remove_file("/tmp/aha_diag.log");
        Self::diag_mark("1: compile start");
        self.declare_printf();
        self.declare_c_runtime();
        Self::diag_mark("2: c runtime declared");
        // Phase 1: actors are pure JIT — no runtime functions needed.
        self.declare_actor_runtime();
        Self::diag_mark("2a: actor runtime declared");
        // String + file builtins depend on C runtime (malloc, snprintf, fopen, etc.)
        self.declare_string_and_file_builtins();
        Self::diag_mark("2b: string/file builtins declared");
        // List builtins depend on malloc/realloc/free from the C runtime.
        self.create_list_builtins();
        Self::diag_mark("3: list builtins created");
        self.create_map_builtins();
        Self::diag_mark("3m: map builtins created");

        // DIAGNOSTIC: verify the module is valid before proceeding, so an
        // invalid-IR bug surfaces as a message instead of a SIGSEGV in
        // print_to_string. Remove once List<T> lands.
        match self.module.verify() {
            Ok(()) => Self::diag_mark("4: verify ok"),
            Err(e) => {
                Self::diag_mark(&format!("4: MODULE VERIFY FAILED: {}", e));
                return Err(format!("LLVM module verification failed: {}", e));
            }
        }

        // Register struct definitions first so struct literals and field
        // access can resolve field layout during codegen.
        self.register_structs(&program.statements);
        // Register enum definitions so constructors and match can resolve
        // variant layout during codegen.
        self.register_enums(&program.statements);

        // Pre-pass: iterate scanning until param types and return types
        // stabilize. A single pass is insufficient: a struct param's
        // type is only known after its call site is scanned, and chained
        // calls like sum(make(20, 22)) need return types to type the
        // inner call's argument. Each iteration only upgrades types
        // (Int → String/Struct), so this terminates quickly.
        for _ in 0..32 {
            let before_params = self.param_type_map.clone();
            let before_fns = self.fn_types.clone();
            self.scan_call_sites(&program.statements);
            // Recompute return types now that param types may have changed.
            for stmt in &program.statements {
                if let ast::Statement::Expression(ast::ExpressionStatement {
                    expression: ast::Expression::Function(func),
                }) = stmt
                {
                    if let Some(name) = &func.name {
                        // Generic functions are monomorphized lazily at call
                        // sites; their "return type" only exists per
                        // instantiation, so skip them here.
                        if !func.type_params.is_empty() {
                            continue;
                        }
                        let rt = self.infer_function_return_type(func, &name.value);
                        self.fn_types.insert(name.value.clone(), rt);
                    }
                }
            }
            if self.param_type_map == before_params && self.fn_types == before_fns {
                break;
            }
        }

        // Pre-declare all user functions so mutual recursion works:
        // is_even can call is_odd before is_odd's body is compiled.
        self.predeclare_functions(&program.statements);

        // ponytail: Create an implicit entry point ("main") that compiles
        // all user statements. When the user defines fn main(),
        // compile_function creates the real @main with the user's body.
        let has_user_main = program.statements.iter().any(|s| {
            matches!(s, ast::Statement::Expression(ast::ExpressionStatement {
                expression: ast::Expression::Function(f), ..
            }) if f.name.as_ref().map(|n| n.value.as_str()) == Some("main"))
        });

        // When user defines fn main(), compile_function creates @main.
        // We must NOT also create an implicit @main — that would give us
        // two @main functions. The implicit @main is only for programs
        // without a user-defined main (e.g. "1 + 2" → returns 3).
        let implicit_entry = if !has_user_main {
            let fn_type = self.i64_type.fn_type(&[], false);
            let function = self.module.add_function("main", fn_type, None);
            self.functions.insert("main".to_string(), function);
            let bb = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(bb);
            Some(bb)
        } else {
            None
        };

        Self::diag_mark(&format!("LOOP: {} statements, has_user_main={}", program.statements.len(), has_user_main));
        let mut last_value: Option<TypedValue<'ctx>> = None;
        for (i, statement) in program.statements.iter().enumerate() {
            let is_last = i == program.statements.len() - 1;
            let stmt_desc = match statement {
                ast::Statement::Enum(e) => format!("Enum({})", e.name.value),
                ast::Statement::Expression(es) => match &es.expression {
                    ast::Expression::Function(f) => format!("Fn({})", f.name.as_ref().map(|n| n.value.as_str()).unwrap_or("?")),
                    other => format!("Expr({:?})", std::mem::discriminant(other)),
                },
                other => format!("{:?}", std::mem::discriminant(other)),
            };
            Self::diag_mark(&format!("LOOP[{}]: is_last={} stmt={}", i, is_last, stmt_desc));
            if is_last {
                if let ast::Statement::Expression(expr_stmt) = statement {
                    if has_user_main {
                        let _val = self.compile_expression(&expr_stmt.expression)?;
                        // compile_function leaves builder inside user's @main
                        // or a callee. Restore to the entry block of the
                        // user's @main so build_return below targets it.
                        if let Some(&main_fn) = self.functions.get("main") {
                            if let Some(entry) = main_fn.get_first_basic_block() {
                                self.builder.position_at_end(entry);
                            }
                        }
                    } else {
                        let val = self.compile_expression(&expr_stmt.expression)?;
                        last_value = Some(val);
                    }
                    continue;
                }
            }
            self.compile_statement(statement)?;
        }
        if has_user_main {
            // User's fn main() already has a return. The build_return below
            // would be unreachable (second terminator). Skip it.
        } else {
            let return_val = match last_value {
                Some(tv) => match tv.aha_type {
                    AhaType::String | AhaType::Struct(_) | AhaType::Enum(_) => self.i64_type.const_int(0, false).into(),
                    _ => tv.value,
                },
                None => self.i64_type.const_int(0, false).into(),
            };
            let _ = self.builder.build_return(Some(&return_val));
        }

        Self::diag_mark("5: main compiled");

        // DIAGNOSTIC: dump ALL functions and their blocks
        {
            let mut func_count = 0;
            for f in self.module.get_functions() {
                func_count += 1;
                let name = f.get_name().to_str().unwrap_or("?");
                let block_count = f.get_basic_blocks().len();
                let first_term = f.get_first_basic_block()
                    .and_then(|bb| bb.get_terminator())
                    .is_some();
                Self::diag_mark(&format!("5a: @{} #{} has {} blocks, entry_has_term={}", name, func_count, block_count, first_term));
                // Dump block names
                for bb in f.get_basic_blocks() {
                    let bname = bb.get_name().to_str().unwrap_or("?");
                    let has_term = bb.get_terminator().is_some();
                    Self::diag_mark(&format!("5c: @{} block '{}' has_terminator={}", name, bname, has_term));
                }
            }
            Self::diag_mark(&format!("5a2: total functions in module: {}, self.functions keys: {:?}", func_count,
                self.functions.keys().collect::<Vec<_>>()));
        }

        // DIAGNOSTIC: second verify after main compilation, before
        // returning to the caller (print_to_string / JIT).
        match self.module.verify() {
            Ok(()) => Self::diag_mark("6: verify after main ok"),
            Err(e) => {
                Self::diag_mark(&format!("6: VERIFY AFTER MAIN FAILED: {}", e));
                std::process::abort();
            }
        }

        Ok(())
    }

    fn declare_printf(&mut self) {
        let i8_ptr_type = self.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let printf_type = self.i64_type.fn_type(&[i8_ptr_type.into()], true);
        let printf_fn = self.module.add_function("printf", printf_type, None);
        self.functions.insert("printf".to_string(), printf_fn);
        
        self.create_print_int_builtin();
        self.create_print_str_builtin();
        self.create_abs_builtin();
        self.create_min_builtin();
        self.create_max_builtin();
        self.create_len_builtin();

        // Register return types for builtins
        self.fn_types.insert("print".to_string(), AhaType::Int);
        self.fn_types.insert("abs".to_string(), AhaType::Int);
        self.fn_types.insert("min".to_string(), AhaType::Int);
        self.fn_types.insert("max".to_string(), AhaType::Int);
        self.fn_types.insert("len".to_string(), AhaType::Int);
    }

    /// Declare string + file I/O builtins (depend on C runtime: malloc, snprintf, fopen, etc.)
    fn declare_string_and_file_builtins(&mut self) {
        self.create_int_to_string_builtin();
        self.create_string_to_int_builtin();
        self.create_string_sub_builtin();
        self.create_char_at_builtin();
        self.create_file_read_builtin();
        self.create_file_write_builtin();

        self.fn_types.insert("int_to_string".to_string(), AhaType::String);
        self.fn_types.insert("string_to_int".to_string(), AhaType::Int);
        self.fn_types.insert("string_sub".to_string(), AhaType::String);
        self.fn_types.insert("char_at".to_string(), AhaType::Int);
        self.fn_types.insert("file_read".to_string(), AhaType::String);
        self.fn_types.insert("file_write".to_string(), AhaType::Int);
    }

    // Builtin: print(int) -> prints integer with newline
    fn create_print_int_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = i64_type.fn_type(&[i64_type.into()], false);
        let function = self.module.add_function("print", fn_type, None);
        
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        
        let value = function.get_nth_param(0).expect("print: missing param 0").into_int_value();
        let format_str = self.builder.build_global_string_ptr("%lld\n", "fmt")
            .expect("print: failed to build format string");
        
        let printf_fn = self.functions.get("printf").expect("printf not declared");
        let _ = self.builder.build_call(
            *printf_fn,
            &[format_str.as_pointer_value().into(), value.into()],
            "printf_call"
        );
        
        let _ = self.builder.build_return(Some(&value));
        self.functions.insert("print".to_string(), function);
    }

    // Builtin: print_str(string_struct) -> prints string content
    fn create_print_str_builtin(&mut self) {
        let i64_type = self.i64_type;
        let string_type = self.string_type;
        let fn_type = i64_type.fn_type(&[string_type.into()], false);
        let function = self.module.add_function("print_str", fn_type, None);
        
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        
        // Extract i8* pointer from string struct (field 0)
        let str_struct = function.get_nth_param(0).expect("print_str: missing param 0").into_struct_value();
        let str_ptr = self.builder.build_extract_value(str_struct, 0, "str_ptr")
            .expect("print_str: failed to extract pointer")
            .into_pointer_value();
        
        let format_str = self.builder.build_global_string_ptr("%s\n", "str_fmt")
            .expect("print_str: failed to build format string");
        
        let printf_fn = self.functions.get("printf").expect("printf not declared");
        let _ = self.builder.build_call(
            *printf_fn,
            &[format_str.as_pointer_value().into(), str_ptr.into()],
            "printf_str_call"
        );
        
        let zero = i64_type.const_int(0, false);
        let _ = self.builder.build_return(Some(&zero));
        self.functions.insert("print_str".to_string(), function);
        self.fn_types.insert("print_str".to_string(), AhaType::Int);
    }

    // Builtin: abs(x) -> absolute value
    fn create_abs_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = i64_type.fn_type(&[i64_type.into()], false);
        let function = self.module.add_function("abs", fn_type, None);
        
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        
        let x = function.get_nth_param(0).expect("abs: missing param 0").into_int_value();
        let zero = i64_type.const_int(0, false);
        let neg_x = self.builder.build_int_neg(x, "neg_x")
            .expect("abs: failed to build neg");
        let is_neg = self.builder.build_int_compare(inkwell::IntPredicate::SLT, x, zero, "is_neg")
            .expect("abs: failed to build compare");
        let result = self.builder.build_select(is_neg, neg_x, x, "abs_result")
            .expect("abs: failed to build select");
        
        let _ = self.builder.build_return(Some(&result));
        self.functions.insert("abs".to_string(), function);
    }

    // Builtin: min(a, b) -> minimum value
    fn create_min_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = i64_type.fn_type(&[i64_type.into(), i64_type.into()], false);
        let function = self.module.add_function("min", fn_type, None);
        
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        
        let a = function.get_nth_param(0).expect("min: missing param 0").into_int_value();
        let b = function.get_nth_param(1).expect("min: missing param 1").into_int_value();
        let is_less = self.builder.build_int_compare(inkwell::IntPredicate::SLT, a, b, "is_less")
            .expect("min: failed to build compare");
        let result = self.builder.build_select(is_less, a, b, "min_result")
            .expect("min: failed to build select");
        
        let _ = self.builder.build_return(Some(&result));
        self.functions.insert("min".to_string(), function);
    }

    // Builtin: max(a, b) -> maximum value
    fn create_max_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = i64_type.fn_type(&[i64_type.into(), i64_type.into()], false);
        let function = self.module.add_function("max", fn_type, None);
        
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        
        let a = function.get_nth_param(0).expect("max: missing param 0").into_int_value();
        let b = function.get_nth_param(1).expect("max: missing param 1").into_int_value();
        let is_greater = self.builder.build_int_compare(inkwell::IntPredicate::SGT, a, b, "is_greater")
            .expect("max: failed to build compare");
        let result = self.builder.build_select(is_greater, a, b, "max_result")
            .expect("max: failed to build select");
        
        let _ = self.builder.build_return(Some(&result));
        self.functions.insert("max".to_string(), function);
    }

    // Builtin: len(string_struct) -> returns string length
    fn create_len_builtin(&mut self) {
        let i64_type = self.i64_type;
        let string_type = self.string_type;
        let fn_type = i64_type.fn_type(&[string_type.into()], false);
        let function = self.module.add_function("len", fn_type, None);
        
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        
        let str_struct = function.get_nth_param(0).expect("len: missing param 0").into_struct_value();
        let str_len = self.builder.build_extract_value(str_struct, 1, "str_len")
            .expect("len: failed to extract length");
        
        let _ = self.builder.build_return(Some(&str_len));
        self.functions.insert("len".to_string(), function);
    }

    // =====================================================================
    // String builtins — conversion, substring, character access.
    // =====================================================================

    // Builtin: int_to_string(value: int) -> string
    // Uses snprintf to format i64 into a heap-allocated buffer.
    fn create_int_to_string_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = self.string_type.fn_type(&[i64_type.into()], false);
        let function = self.module.add_function("int_to_string", fn_type, None);

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let value = function.get_nth_param(0).expect("int_to_string: missing param").into_int_value();

        // Alloc 32-byte buffer
        let buf_size = i64_type.const_int(32, false);
        let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
        let buf = self.builder.build_call(malloc_fn, &[buf_size.into()], "buf")
            .expect("malloc failed").try_as_basic_value().left().unwrap().into_pointer_value();

        // snprintf(buf, 32, "%lld", value)
        let fmt = self.builder.build_global_string_ptr("%lld", "fmt").expect("fmt failed");
        let snprintf_fn = *self.functions.get("snprintf").expect("snprintf not declared");
        let _ = self.builder.build_call(snprintf_fn, &[buf.into(), buf_size.into(), fmt.as_pointer_value().into(), value.into()], "snprintf_call");

        // len = strlen(buf)
        let strlen_fn = *self.functions.get("strlen").expect("strlen not declared");
        let len = self.builder.build_call(strlen_fn, &[buf.into()], "str_len")
            .expect("strlen failed").try_as_basic_value().left().unwrap().into_int_value();

        // Build {i8*, i64} string struct
        let s = self.string_type.const_zero();
        let s = self.builder.build_insert_value(s, buf, 0, "sptr").expect("insert ptr").into_struct_value();
        let s = self.builder.build_insert_value(s, len, 1, "slen").expect("insert len").into_struct_value();

        let _ = self.builder.build_return(Some(&s));
        self.functions.insert("int_to_string".to_string(), function);
    }

    // Builtin: string_to_int(str: string) -> int
    // Uses strtol to parse a string to i64.
    fn create_string_to_int_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = i64_type.fn_type(&[self.string_type.into()], false);
        let function = self.module.add_function("string_to_int", fn_type, None);

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let str_struct = function.get_nth_param(0).expect("string_to_int: missing param").into_struct_value();
        let str_ptr = self.builder.build_extract_value(str_struct, 0, "sptr")
            .expect("extract ptr").into_pointer_value();

        // strtol(str_ptr, NULL, 10)
        let strtol_fn = *self.functions.get("strtol").expect("strtol not declared");
        let i8_type = self.context.i8_type();
        let i8_ptr_ptr_type = i8_type.ptr_type(inkwell::AddressSpace::default()).ptr_type(inkwell::AddressSpace::default());
        let null_ptr = i8_ptr_ptr_type.const_null();
        let base_10 = i64_type.const_int(10, false);
        let result = self.builder.build_call(strtol_fn, &[str_ptr.into(), null_ptr.into(), base_10.into()], "strtol_result")
            .expect("strtol failed").try_as_basic_value().left().unwrap().into_int_value();

        let _ = self.builder.build_return(Some(&result));
        self.functions.insert("string_to_int".to_string(), function);
    }

    // Builtin: string_sub(str: string, start: int, len: int) -> string
    // Extracts a substring via malloc + memcpy.
    fn create_string_sub_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = self.string_type.fn_type(&[self.string_type.into(), i64_type.into(), i64_type.into()], false);
        let function = self.module.add_function("string_sub", fn_type, None);

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let str_struct = function.get_nth_param(0).expect("string_sub: missing str").into_struct_value();
        let str_ptr = self.builder.build_extract_value(str_struct, 0, "sptr")
            .expect("extract ptr").into_pointer_value();
        let str_len = self.builder.build_extract_value(str_struct, 1, "slen")
            .expect("extract len").into_int_value();
        let start = function.get_nth_param(1).expect("string_sub: missing start").into_int_value();
        let req_len = function.get_nth_param(2).expect("string_sub: missing len").into_int_value();

        // Clamp: actual_len = min(req_len, str_len - start)
        let i64_zero = i64_type.const_int(0, false);
        let remaining = self.builder.build_int_sub(str_len, start, "remaining").expect("sub failed");
        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::SGT, req_len, remaining, "cmp_len").expect("cmp failed");
        let actual_len = self.builder.build_select(cmp, remaining, req_len, "actual_len").expect("select failed").into_int_value();

        // src = str_ptr + start
        let src = unsafe { self.builder.build_gep(str_ptr, &[start], "src").expect("gep failed") };

        // alloc actual_len + 1
        let one = i64_type.const_int(1, false);
        let alloc_size = self.builder.build_int_add(actual_len, one, "alloc_sz").expect("add failed");
        let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
        let new_buf = self.builder.build_call(malloc_fn, &[alloc_size.into()], "newbuf")
            .expect("malloc failed").try_as_basic_value().left().unwrap().into_pointer_value();

        // memcpy(new_buf, src, actual_len)
        let memcpy_fn = *self.functions.get("memcpy").expect("memcpy not declared");
        let _ = self.builder.build_call(memcpy_fn, &[new_buf.into(), src.into(), actual_len.into()], "cp");

        // null terminate
        let null_pos = unsafe { self.builder.build_gep(new_buf, &[actual_len], "nullpos").expect("gep null") };
        let i8_type = self.context.i8_type();
        let _ = self.builder.build_store(null_pos, i8_type.const_int(0, false));

        // Build {i8*, i64} string struct
        let s = self.string_type.const_zero();
        let s = self.builder.build_insert_value(s, new_buf, 0, "sptr").expect("insert ptr").into_struct_value();
        let s = self.builder.build_insert_value(s, actual_len, 1, "slen").expect("insert len").into_struct_value();

        let _ = self.builder.build_return(Some(&s));
        self.functions.insert("string_sub".to_string(), function);
    }

    // Builtin: char_at(str: string, index: int) -> int
    // Returns the character at the given index as an integer (ASCII value).
    fn create_char_at_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = i64_type.fn_type(&[self.string_type.into(), i64_type.into()], false);
        let function = self.module.add_function("char_at", fn_type, None);

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        let str_struct = function.get_nth_param(0).expect("char_at: missing str").into_struct_value();
        let str_ptr = self.builder.build_extract_value(str_struct, 0, "sptr")
            .expect("extract ptr").into_pointer_value();
        let index = function.get_nth_param(1).expect("char_at: missing index").into_int_value();

        // char_ptr = str_ptr + index
        let char_ptr = unsafe { self.builder.build_gep(str_ptr, &[index], "char_ptr").expect("gep failed") };

        // Load i8 value and extend to i64
        let i8_type = self.context.i8_type();
        let char_val = self.builder.build_load(char_ptr, "char_val")
            .expect("load failed").into_int_value();
        let result = self.builder.build_int_z_extend(char_val, i64_type, "char_ext").expect("zext failed");

        let _ = self.builder.build_return(Some(&result));
        self.functions.insert("char_at".to_string(), function);
    }

    // =====================================================================
    // File I/O builtins — fopen/fread/fwrite/fclose wrappers.
    // =====================================================================

    // Builtin: file_read(path: string) -> string
    // Reads entire file into a heap-allocated string.
    fn create_file_read_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = self.string_type.fn_type(&[self.string_type.into()], false);
        let function = self.module.add_function("file_read", fn_type, None);

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        // Extract path string
        let path_struct = function.get_nth_param(0).expect("file_read: missing path").into_struct_value();
        let path_ptr = self.builder.build_extract_value(path_struct, 0, "path_ptr")
            .expect("extract ptr").into_pointer_value();

        // fopen(path, "rb")
        let fopen_fn = *self.functions.get("fopen").expect("fopen not declared");
        let mode = self.builder.build_global_string_ptr("rb", "mode").expect("mode failed");
        let fp = self.builder.build_call(fopen_fn, &[path_ptr.into(), mode.as_pointer_value().into()], "fp")
            .expect("fopen failed").try_as_basic_value().left().unwrap().into_pointer_value();

        // fseek(fp, 0, SEEK_END) — SEEK_END = 2
        let fseek_fn = *self.functions.get("fseek").expect("fseek not declared");
        let zero = i64_type.const_int(0, false);
        let seek_end = self.context.i32_type().const_int(2, false);
        let _ = self.builder.build_call(fseek_fn, &[fp.into(), zero.into(), seek_end.into()], "fseek_end");

        // size = ftell(fp)
        let ftell_fn = *self.functions.get("ftell").expect("ftell not declared");
        let size = self.builder.build_call(ftell_fn, &[fp.into()], "size")
            .expect("ftell failed").try_as_basic_value().left().unwrap().into_int_value();

        // fseek(fp, 0, SEEK_SET) — SEEK_SET = 0
        let seek_set = self.context.i32_type().const_int(0, false);
        let _ = self.builder.build_call(fseek_fn, &[fp.into(), zero.into(), seek_set.into()], "fseek_set");

        // buf = malloc(size + 1)
        let one = i64_type.const_int(1, false);
        let alloc_size = self.builder.build_int_add(size, one, "alloc_sz").expect("add failed");
        let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
        let buf = self.builder.build_call(malloc_fn, &[alloc_size.into()], "buf")
            .expect("malloc failed").try_as_basic_value().left().unwrap().into_pointer_value();

        // fread(buf, 1, size, fp)
        let fread_fn = *self.functions.get("fread").expect("fread not declared");
        let one_64 = i64_type.const_int(1, false);
        let _ = self.builder.build_call(fread_fn, &[buf.into(), one_64.into(), size.into(), fp.into()], "fread_call");

        // fclose(fp)
        let fclose_fn = *self.functions.get("fclose").expect("fclose not declared");
        let _ = self.builder.build_call(fclose_fn, &[fp.into()], "fclose_call");

        // null terminate
        let null_pos = unsafe { self.builder.build_gep(buf, &[size], "nullpos").expect("gep null") };
        let i8_type = self.context.i8_type();
        let _ = self.builder.build_store(null_pos, i8_type.const_int(0, false));

        // Build {i8*, i64} string struct
        let s = self.string_type.const_zero();
        let s = self.builder.build_insert_value(s, buf, 0, "sptr").expect("insert ptr").into_struct_value();
        let s = self.builder.build_insert_value(s, size, 1, "slen").expect("insert len").into_struct_value();

        let _ = self.builder.build_return(Some(&s));
        self.functions.insert("file_read".to_string(), function);
    }

    // Builtin: file_write(path: string, content: string) -> int
    // Writes string content to file. Returns bytes written.
    fn create_file_write_builtin(&mut self) {
        let i64_type = self.i64_type;
        let fn_type = i64_type.fn_type(&[self.string_type.into(), self.string_type.into()], false);
        let function = self.module.add_function("file_write", fn_type, None);

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        // Extract path
        let path_struct = function.get_nth_param(0).expect("file_write: missing path").into_struct_value();
        let path_ptr = self.builder.build_extract_value(path_struct, 0, "path_ptr")
            .expect("extract path ptr").into_pointer_value();

        // Extract content
        let content_struct = function.get_nth_param(1).expect("file_write: missing content").into_struct_value();
        let content_ptr = self.builder.build_extract_value(content_struct, 0, "content_ptr")
            .expect("extract content ptr").into_pointer_value();
        let content_len = self.builder.build_extract_value(content_struct, 1, "content_len")
            .expect("extract content len").into_int_value();

        // fopen(path, "wb")
        let fopen_fn = *self.functions.get("fopen").expect("fopen not declared");
        let mode = self.builder.build_global_string_ptr("wb", "mode").expect("mode failed");
        let fp = self.builder.build_call(fopen_fn, &[path_ptr.into(), mode.as_pointer_value().into()], "fp")
            .expect("fopen failed").try_as_basic_value().left().unwrap().into_pointer_value();

        // bytes_written = fwrite(content_ptr, 1, content_len, fp)
        let fwrite_fn = *self.functions.get("fwrite").expect("fwrite not declared");
        let one = i64_type.const_int(1, false);
        let written = self.builder.build_call(fwrite_fn, &[content_ptr.into(), one.into(), content_len.into(), fp.into()], "written")
            .expect("fwrite failed").try_as_basic_value().left().unwrap().into_int_value();

        // fclose(fp)
        let fclose_fn = *self.functions.get("fclose").expect("fclose not declared");
        let _ = self.builder.build_call(fclose_fn, &[fp.into()], "fclose_call");

        let _ = self.builder.build_return(Some(&written));
        self.functions.insert("file_write".to_string(), function);
    }

    // =====================================================================
    // List<T> builtins — dynamic array with heap allocation.
    //
    // A list is an opaque i64 handle = pointer to a heap header struct:
    //   struct ListHeader { data: i8*, len: i64, cap: i64, elem_size: i64 }
    // The handle is the header address itself (an i64), so it fits the
    // existing i64 variable model. Element storage is a raw malloc'd
    // buffer of `elem_size`-byte records. list_* builtins take/return
    // the handle as i64, so no AhaType::List plumbing is needed at the
    // LLVM level — types are tracked purely in the type system.
    //
    // Builtin list (always declared, like print/len):
    //   list_new()                    -> List<Int>  (elem_size 8)
    //   list_new_string()             -> List<String> (elem_size 16)
    //   list_push(list, value)        -> list (realloc if needed)
    //   list_get(list, index)         -> element (Int or String)
    //   list_len(list)                -> Int
    //   list_free(list)               -> Int (0) — frees data + header
    // =====================================================================

    fn create_list_builtins(&mut self) {
        Self::diag_mark("3a: create_list_builtins start");
        let i64_type = self.i64_type;
        let i8_ptr = self.i8_ptr_type();
        let header = self.list_header_type;
        let header_ptr = header.ptr_type(inkwell::AddressSpace::default());

        // Helper: header pointer from a list handle (i64).
        // Used by every list_* builtin after the first.
        let header_from_handle = |builder: &Builder<'ctx>, handle: inkwell::values::IntValue<'ctx>| {
            builder.build_int_to_ptr(handle, header_ptr, "list_hdr").expect("int_to_ptr failed")
        };
        Self::diag_mark("3b: header_from_handle closure created");

        // --- list_new() -> List<Int> ---
        {
            Self::diag_mark("3c: list_new start");
            let fn_type = i64_type.fn_type(&[], false);
            Self::diag_mark("3c1: fn_type ok");
            let function = self.module.add_function("list_new", fn_type, None);
            Self::diag_mark("3c2: add_function ok");
            let entry = self.context.append_basic_block(function, "entry");
            Self::diag_mark("3c3: append_basic_block ok");
            self.builder.position_at_end(entry);
            Self::diag_mark("3c4: position ok");

            let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
            let hdr_size = i64_type.const_int(32, false); // 4 x i64 header
            let hdr = self.builder.build_call(malloc_fn, &[hdr_size.into()], "list_hdr")
                .expect("malloc failed")
                .try_as_basic_value().left().expect("malloc void")
                .into_pointer_value();
            Self::diag_mark("3c5: malloc call ok");

            // Zero the whole header explicitly — malloc memory is garbage.
            let zero = i64_type.const_int(0, false);
            let hdr_ptr = self.builder.build_bitcast(hdr, header_ptr, "hdr_typed")
                .expect("bitcast failed").into_pointer_value();
            Self::diag_mark("3c6: bitcast ok");
            Self::diag_mark("3c6a: before gep");
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr")
                .expect("gep failed");
            Self::diag_mark("3c6b: gep ok");
            self.builder.build_store(data_ptr, self.i8_ptr_type().const_null()).expect("store failed");
            Self::diag_mark("3c7: data store ok");
            let len_ptr = self.builder.build_struct_gep(hdr_ptr, 1, "len_ptr")
                .expect("gep failed");
            self.builder.build_store(len_ptr, zero).expect("store failed");
            Self::diag_mark("3c8: len store ok");
            let cap_ptr = self.builder.build_struct_gep(hdr_ptr, 2, "cap_ptr")
                .expect("gep failed");
            self.builder.build_store(cap_ptr, zero).expect("store failed");
            Self::diag_mark("3c9: cap store ok");

            // elem_size = 8 (Int)
            let es_ptr = self.builder.build_struct_gep(hdr_ptr, 3, "es_ptr")
                .expect("gep failed");
            self.builder.build_store(es_ptr, i64_type.const_int(8, false)).expect("store failed");
            Self::diag_mark("3c10: es store ok");

            // Return handle as i64 (header address).
            let handle = self.builder.build_ptr_to_int(hdr, i64_type, "list_handle")
                .expect("ptr_to_int failed");
            Self::diag_mark("3c11: ptr_to_int ok");
            let _ = self.builder.build_return(Some(&handle));
            Self::diag_mark("3c12: return ok");
            self.functions.insert("list_new".to_string(), function);
            Self::diag_mark("3c13: list_new done");
        }

        // --- list_new_string() -> List<String> (elem_size 16) ---
        {
            Self::diag_mark("3d: list_new_string start");
            let fn_type = i64_type.fn_type(&[], false);
            let function = self.module.add_function("list_new_string", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
            let hdr_size = i64_type.const_int(32, false);
            let hdr = self.builder.build_call(malloc_fn, &[hdr_size.into()], "list_hdr")
                .expect("malloc failed")
                .try_as_basic_value().left().expect("malloc void")
                .into_pointer_value();
            // Zero the whole header explicitly.
            let zero = i64_type.const_int(0, false);
            let hdr_ptr = self.builder.build_bitcast(hdr, header_ptr, "hdr_typed").expect("bitcast failed").into_pointer_value();
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr")
                .expect("gep failed");
            self.builder.build_store(data_ptr, self.i8_ptr_type().const_null()).expect("store failed");
            let len_ptr = self.builder.build_struct_gep(hdr_ptr, 1, "len_ptr")
                .expect("gep failed");
            self.builder.build_store(len_ptr, zero).expect("store failed");
            let cap_ptr = self.builder.build_struct_gep(hdr_ptr, 2, "cap_ptr")
                .expect("gep failed");
            self.builder.build_store(cap_ptr, zero).expect("store failed");
            let es_ptr = self.builder.build_struct_gep(hdr_ptr, 3, "es_ptr")
                .expect("gep failed");
            self.builder.build_store(es_ptr, i64_type.const_int(16, false)).expect("store failed");
            let handle = self.builder.build_ptr_to_int(hdr, i64_type, "list_handle").expect("ptr_to_int failed");
            let _ = self.builder.build_return(Some(&handle));
            self.functions.insert("list_new_string".to_string(), function);
        }

        // --- list_push(list, value) -> list ---
        {
            Self::diag_mark("3e: list_push start");
            let fn_type = i64_type.fn_type(&[i64_type.into(), i64_type.into()], false);
            let function = self.module.add_function("list_push", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("push: param 0").into_int_value();
            let value = function.get_nth_param(1).expect("push: param 1").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);

            // Load len and cap.
            let len_ptr = self.builder.build_struct_gep(hdr_ptr, 1, "len_ptr")
                .expect("gep failed");
            let cap_ptr = self.builder.build_struct_gep(hdr_ptr, 2, "cap_ptr")
                .expect("gep failed");
            let es_ptr = self.builder.build_struct_gep(hdr_ptr, 3, "es_ptr")
                .expect("gep failed");
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr")
                .expect("gep failed");

            let len = self.builder.build_load(len_ptr, "len").expect("load failed").into_int_value();
            let cap = self.builder.build_load(cap_ptr, "cap").expect("load failed").into_int_value();
            let elem_size = self.builder.build_load(es_ptr, "elem_size").expect("load failed").into_int_value();
            let data = self.builder.build_load(data_ptr, "data").expect("load failed").into_pointer_value();

            // If len == cap, grow: new_cap = cap == 0 ? 4 : cap * 2.
            let zero = i64_type.const_int(0, false);
            let four = i64_type.const_int(4, false);
            let two = i64_type.const_int(2, false);
            let needs_grow = self.builder.build_int_compare(inkwell::IntPredicate::EQ, len, cap, "needs_grow")
                .expect("cmp failed");

            let grow_block = self.context.append_basic_block(function, "grow");
            let no_grow_block = self.context.append_basic_block(function, "no_grow");
            let merge_block = self.context.append_basic_block(function, "grow_merge");
            self.builder.build_conditional_branch(needs_grow, grow_block, no_grow_block)
                .expect("branch failed");

            // no_grow: just branch to merge
            self.builder.position_at_end(no_grow_block);
            self.builder.build_unconditional_branch(merge_block).expect("branch failed");

            // Grow: realloc(data, new_cap * elem_size)
            self.builder.position_at_end(grow_block);
            let new_cap = self.builder.build_select(
                self.builder.build_int_compare(inkwell::IntPredicate::EQ, cap, zero, "cap_is_zero").expect("cmp failed"),
                four,
                self.builder.build_int_mul(cap, two, "cap_x2").expect("mul failed"),
                "new_cap"
            ).expect("select failed");
            let realloc_fn = *self.functions.get("realloc").expect("realloc not declared");
            let new_data_size = self.builder.build_int_mul(new_cap.into_int_value(), elem_size, "new_size").expect("mul failed");
            let new_data = self.builder.build_call(realloc_fn, &[data.into(), new_data_size.into()], "realloc_data")
                .expect("realloc failed")
                .try_as_basic_value().left().expect("realloc void")
                .into_pointer_value();
            // store new data + new cap back into header
            self.builder.build_store(data_ptr, new_data).expect("store failed");
            self.builder.build_store(cap_ptr, new_cap).expect("store failed");
            self.builder.build_unconditional_branch(merge_block).expect("branch failed");

            // Merge: reload data (may have changed) and store value at data[len*elem_size]
            self.builder.position_at_end(merge_block);
            let merged_data = self.builder.build_load(data_ptr, "data2").expect("load failed").into_pointer_value();
            let byte_off = self.builder.build_int_mul(len, elem_size, "byte_off").expect("mul failed");
            let elem_ptr = unsafe { self.builder.build_gep(merged_data, &[byte_off], "elem_ptr") }
                .expect("gep failed");
            // Bitcast i8* to i64* before storing — LLVM requires typed pointers.
            let elem_i64_ptr = self.builder.build_bitcast(elem_ptr, i64_type.ptr_type(inkwell::AddressSpace::default()), "elem_i64_ptr")
                .expect("bitcast failed").into_pointer_value();
            self.builder.build_store(elem_i64_ptr, value).expect("store failed");

            // len += 1
            let new_len = self.builder.build_int_add(len, i64_type.const_int(1, false), "new_len").expect("add failed");
            self.builder.build_store(len_ptr, new_len).expect("store failed");

            let _ = self.builder.build_return(Some(&handle));
            self.functions.insert("list_push".to_string(), function);
        }

        // --- list_push_string(list, ptr, len) -> list ---
        // For List<String>, the caller splits the string struct into
        // (i8* pointer, i64 length) and passes both; this builtin stores
        // the full 16-byte element {i8*, i64} at data[len].
        {
            Self::diag_mark("3f: list_push_string start");
            let fn_type = i64_type.fn_type(&[i64_type.into(), i8_ptr.into(), i64_type.into()], false);
            let function = self.module.add_function("list_push_string", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("push_s: param 0").into_int_value();
            let str_ptr = function.get_nth_param(1).expect("push_s: param 1").into_pointer_value();
            let str_len = function.get_nth_param(2).expect("push_s: param 2").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);

            let len_ptr = self.builder.build_struct_gep(hdr_ptr, 1, "len_ptr")
                .expect("gep failed");
            let cap_ptr = self.builder.build_struct_gep(hdr_ptr, 2, "cap_ptr")
                .expect("gep failed");
            let es_ptr = self.builder.build_struct_gep(hdr_ptr, 3, "es_ptr")
                .expect("gep failed");
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr")
                .expect("gep failed");

            let len = self.builder.build_load(len_ptr, "len").expect("load failed").into_int_value();
            let cap = self.builder.build_load(cap_ptr, "cap").expect("load failed").into_int_value();
            let elem_size = self.builder.build_load(es_ptr, "elem_size").expect("load failed").into_int_value();
            let data = self.builder.build_load(data_ptr, "data").expect("load failed").into_pointer_value();

            let zero = i64_type.const_int(0, false);
            let four = i64_type.const_int(4, false);
            let two = i64_type.const_int(2, false);
            let needs_grow = self.builder.build_int_compare(inkwell::IntPredicate::EQ, len, cap, "needs_grow")
                .expect("cmp failed");
            let grow_block = self.context.append_basic_block(function, "grow");
            let no_grow_block = self.context.append_basic_block(function, "no_grow");
            let merge_block = self.context.append_basic_block(function, "grow_merge");
            self.builder.build_conditional_branch(needs_grow, grow_block, no_grow_block)
                .expect("branch failed");

            // no_grow: just branch to merge
            self.builder.position_at_end(no_grow_block);
            self.builder.build_unconditional_branch(merge_block).expect("branch failed");

            self.builder.position_at_end(grow_block);
            let new_cap = self.builder.build_select(
                self.builder.build_int_compare(inkwell::IntPredicate::EQ, cap, zero, "cap_is_zero").expect("cmp failed"),
                four,
                self.builder.build_int_mul(cap, two, "cap_x2").expect("mul failed"),
                "new_cap"
            ).expect("select failed");
            let realloc_fn = *self.functions.get("realloc").expect("realloc not declared");
            let new_data_size = self.builder.build_int_mul(new_cap.into_int_value(), elem_size, "new_size").expect("mul failed");
            let new_data = self.builder.build_call(realloc_fn, &[data.into(), new_data_size.into()], "realloc_data")
                .expect("realloc failed")
                .try_as_basic_value().left().expect("realloc void")
                .into_pointer_value();
            self.builder.build_store(data_ptr, new_data).expect("store failed");
            self.builder.build_store(cap_ptr, new_cap).expect("store failed");
            self.builder.build_unconditional_branch(merge_block).expect("branch failed");

            self.builder.position_at_end(merge_block);
            let merged_data = self.builder.build_load(data_ptr, "data2").expect("load failed").into_pointer_value();
            let byte_off = self.builder.build_int_mul(len, elem_size, "byte_off").expect("mul failed");
            let elem_ptr = unsafe { self.builder.build_gep(merged_data, &[byte_off], "elem_ptr") }
                .expect("gep failed");
            // Bitcast i8* element pointer to i64* for typed store.
            let elem_i64_ptr = self.builder.build_bitcast(elem_ptr, i64_type.ptr_type(inkwell::AddressSpace::default()), "elem_i64_ptr")
                .expect("bitcast failed").into_pointer_value();
            // Store the i8* pointer as the first 8 bytes (i64).
            let ptr_as_i64 = self.builder.build_ptr_to_int(str_ptr, i64_type, "ptr_as_i64")
                .expect("ptr_to_int failed");
            self.builder.build_store(elem_i64_ptr, ptr_as_i64).expect("store failed");
            // Store the i64 length at offset 8 (GEP index 1 on i64*).
            let str_len_ptr = unsafe { self.builder.build_gep(elem_i64_ptr, &[i64_type.const_int(1, false)], "str_len_ptr") }
                .expect("gep failed");
            self.builder.build_store(str_len_ptr, str_len).expect("store failed");

            let new_len = self.builder.build_int_add(len, i64_type.const_int(1, false), "new_len").expect("add failed");
            self.builder.build_store(len_ptr, new_len).expect("store failed");

            let _ = self.builder.build_return(Some(&handle));
            self.functions.insert("list_push_string".to_string(), function);
        }

        // --- list_get(list, index) -> i64 (Int element or string ptr) ---
        {
            Self::diag_mark("3g: list_get start");
            let fn_type = i64_type.fn_type(&[i64_type.into(), i64_type.into()], false);
            let function = self.module.add_function("list_get", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("get: param 0").into_int_value();
            let index = function.get_nth_param(1).expect("get: param 1").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);

            let len_ptr = self.builder.build_struct_gep(hdr_ptr, 1, "len_ptr")
                .expect("gep failed");
            let es_ptr = self.builder.build_struct_gep(hdr_ptr, 3, "es_ptr")
                .expect("gep failed");
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr")
                .expect("gep failed");

            let len = self.builder.build_load(len_ptr, "len").expect("load failed").into_int_value();
            let elem_size = self.builder.build_load(es_ptr, "elem_size").expect("load failed").into_int_value();
            let data = self.builder.build_load(data_ptr, "data").expect("load failed").into_pointer_value();

            // Bounds check: index < len ? data[index] : 0
            let in_bounds = self.builder.build_int_compare(inkwell::IntPredicate::SLT, index, len, "in_bounds")
                .expect("cmp failed");
            let ok_block = self.context.append_basic_block(function, "get_ok");
            let oob_block = self.context.append_basic_block(function, "get_oob");
            let merge_block = self.context.append_basic_block(function, "get_merge");
            self.builder.build_conditional_branch(in_bounds, ok_block, oob_block)
                .expect("branch failed");

            self.builder.position_at_end(oob_block);
            let oob_val = i64_type.const_int(0, false);
            self.builder.build_unconditional_branch(merge_block).expect("branch failed");

            self.builder.position_at_end(ok_block);
            // element offset = index * elem_size (byte offset into i8* data)
            let byte_off = self.builder.build_int_mul(index, elem_size, "byte_off").expect("mul failed");
            let elem_ptr = unsafe { self.builder.build_gep(data, &[byte_off], "elem_ptr") }
                .expect("gep failed");
            // Bitcast i8* to i64* before loading — LLVM requires typed pointers.
            let elem_i64_ptr = self.builder.build_bitcast(elem_ptr, i64_type.ptr_type(inkwell::AddressSpace::default()), "elem_i64_ptr")
                .expect("bitcast failed").into_pointer_value();
            let elem_val = self.builder.build_load(elem_i64_ptr, "elem_val").expect("load failed").into_int_value();
            self.builder.build_unconditional_branch(merge_block).expect("branch failed");

            self.builder.position_at_end(merge_block);
            let merged = self.builder.build_phi(i64_type, "get_result").expect("phi failed");
            merged.add_incoming(&[(&oob_val as &dyn inkwell::values::BasicValue, oob_block)]);
            merged.add_incoming(&[(&elem_val as &dyn inkwell::values::BasicValue, ok_block)]);
            let merged_val = merged.as_basic_value();
            let _ = self.builder.build_return(Some(&merged_val));
            self.functions.insert("list_get".to_string(), function);
        }

        // --- list_get_string(list, index) -> {i8*, i64} string element ---
        {
            Self::diag_mark("3h: list_get_string start");
            let fn_type = self.string_type.fn_type(&[i64_type.into(), i64_type.into()], false);
            let function = self.module.add_function("list_get_string", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("get_s: param 0").into_int_value();
            let index = function.get_nth_param(1).expect("get_s: param 1").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);

            let len_ptr = self.builder.build_struct_gep(hdr_ptr, 1, "len_ptr")
                .expect("gep failed");
            let es_ptr = self.builder.build_struct_gep(hdr_ptr, 3, "es_ptr")
                .expect("gep failed");
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr")
                .expect("gep failed");

            let len = self.builder.build_load(len_ptr, "len").expect("load failed").into_int_value();
            let elem_size = self.builder.build_load(es_ptr, "elem_size").expect("load failed").into_int_value();
            let data = self.builder.build_load(data_ptr, "data").expect("load failed").into_pointer_value();

            let in_bounds = self.builder.build_int_compare(inkwell::IntPredicate::SLT, index, len, "in_bounds")
                .expect("cmp failed");
            let ok_block = self.context.append_basic_block(function, "get_ok");
            let oob_block = self.context.append_basic_block(function, "get_oob");
            let merge_block = self.context.append_basic_block(function, "get_merge");
            self.builder.build_conditional_branch(in_bounds, ok_block, oob_block)
                .expect("branch failed");

            self.builder.position_at_end(oob_block);
            let empty_str = self.string_type.const_zero();
            self.builder.build_unconditional_branch(merge_block).expect("branch failed");

            self.builder.position_at_end(ok_block);
            let byte_off = self.builder.build_int_mul(index, elem_size, "byte_off").expect("mul failed");
            let elem_ptr = unsafe { self.builder.build_gep(data, &[byte_off], "elem_ptr") }
                .expect("gep failed");
            // Load the full {i8*, i64} string struct from the element slot.
            let elem_struct_ptr = self.builder.build_bitcast(elem_ptr, self.string_type.ptr_type(inkwell::AddressSpace::default()), "elem_str_ptr")
                .expect("bitcast failed").into_pointer_value();
            let elem_str = self.builder.build_load(elem_struct_ptr, "elem_str").expect("load failed");
            self.builder.build_unconditional_branch(merge_block).expect("branch failed");

            self.builder.position_at_end(merge_block);
            let merged = self.builder.build_phi(self.string_type, "get_s_result").expect("phi failed");
            merged.add_incoming(&[(&empty_str as &dyn inkwell::values::BasicValue, oob_block)]);
            merged.add_incoming(&[(&elem_str as &dyn inkwell::values::BasicValue, ok_block)]);
            let merged_val = merged.as_basic_value();
            let _ = self.builder.build_return(Some(&merged_val));
            self.functions.insert("list_get_string".to_string(), function);
        }

        // --- list_len(list) -> i64 ---
        {
            Self::diag_mark("3i: list_len start");
            let fn_type = i64_type.fn_type(&[i64_type.into()], false);
            let function = self.module.add_function("list_len", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("list_len: param 0").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);
            let len_ptr = self.builder.build_struct_gep(hdr_ptr, 1, "len_ptr")
                .expect("gep failed");
            let len = self.builder.build_load(len_ptr, "len").expect("load failed");
            let _ = self.builder.build_return(Some(&len));
            self.functions.insert("list_len".to_string(), function);
        }

        // --- list_free(list) -> i64 (0) ---
        {
            Self::diag_mark("3j: list_free start");
            let fn_type = i64_type.fn_type(&[i64_type.into()], false);
            let function = self.module.add_function("list_free", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("list_free: param 0").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr")
                .expect("gep failed");
            let data = self.builder.build_load(data_ptr, "data").expect("load failed").into_pointer_value();
            let free_fn = *self.functions.get("free").expect("free not declared");
            // free data buffer, then free header
            self.builder.build_call(free_fn, &[data.into()], "free_data").expect("free call failed");
            let hdr_i8 = self.builder.build_bitcast(hdr_ptr, i8_ptr, "hdr_i8").expect("bitcast failed").into_pointer_value();
            self.builder.build_call(free_fn, &[hdr_i8.into()], "free_hdr").expect("free call failed");
            let zero = i64_type.const_int(0, false);
            let _ = self.builder.build_return(Some(&zero));
            self.functions.insert("list_free".to_string(), function);
        }

        // Register return types for list builtins
        self.fn_types.insert("list_new".to_string(), AhaType::List(Box::new(AhaType::Int)));
        self.fn_types.insert("list_new_string".to_string(), AhaType::List(Box::new(AhaType::String)));
        self.fn_types.insert("list_push".to_string(), AhaType::List(Box::new(AhaType::Int)));
        self.fn_types.insert("list_push_string".to_string(), AhaType::List(Box::new(AhaType::String)));
        self.fn_types.insert("list_get".to_string(), AhaType::Int);
        self.fn_types.insert("list_get_string".to_string(), AhaType::String);
        self.fn_types.insert("list_len".to_string(), AhaType::Int);
        self.fn_types.insert("list_free".to_string(), AhaType::Int);
        Self::diag_mark("3k: create_list_builtins done");
    }

    /// Generate LLVM IR for a deterministic hash table (open addressing,
    /// linear probing).  Called once per K×V combo from
    /// `create_map_builtins`.  `key_sz` / `val_sz` are the slot-field
    /// sizes in bytes (8 for Int, 16 for String).  `key_is_str` controls
    /// how the key is hashed (splitmix64 for Int, FNV-1a over bytes for
    /// String) and compared (i64 eq vs memcmp).  `val_is_str` controls
    /// the return type of the get variant (i64 vs {i8*,i64}).
    fn emit_map_combo(
        &mut self,
        prefix: &str,
        key_sz: u64,
        val_sz: u64,
        key_is_str: bool,
        val_is_str: bool,
    ) {
        let i64_type = self.i64_type;
        let i8_ptr = self.i8_ptr_type();
        let header = self.map_header_type;
        let header_ptr = header.ptr_type(inkwell::AddressSpace::default());
        let slot_sz = key_sz + val_sz + 8; // +8 for occupied flag

        // Helper: header pointer from map handle (i64).
        let hdr_from = |b: &Builder<'ctx>, h: inkwell::values::IntValue<'ctx>| {
            b.build_int_to_ptr(h, header_ptr, "map_hdr").expect("int_to_ptr failed")
        };

        // Helper: hash an i64 key via splitmix64.
        let splitmix64 = |b: &Builder<'ctx>, x: inkwell::values::IntValue<'ctx>| {
            let x = b.build_xor(x, i64_type.const_int(0x9e3779b97f4a7c15, false), "sm64_a").unwrap();
            let x = b.build_xor(x, b.build_right_shift(x, i64_type.const_int(30, false), false, "sm64_r1").unwrap(), "sm64_b").unwrap();
            let x = b.build_int_mul(x, i64_type.const_int(0xbf58476d1ce4e5b9, false), "sm64_c").unwrap();
            let x = b.build_xor(x, b.build_right_shift(x, i64_type.const_int(27, false), false, "sm64_r2").unwrap(), "sm64_d").unwrap();
            let x = b.build_int_mul(x, i64_type.const_int(0x94d049bb133111eb, false), "sm64_e").unwrap();
            b.build_xor(x, b.build_right_shift(x, i64_type.const_int(31, false), false, "sm64_r3").unwrap(), "sm64_f").unwrap()
        };

        let memcmp_fn = *self.functions.get("memcmp").expect("memcmp not declared");

        // Real LLVM function for FNV-1a string hash — allocas in its own entry block.
        // Only create once (emit_map_combo called 4 times for 4 combos).
        let fnv_func = if let Some(f) = self.module.get_function("__fnv1a_hash") {
            f
        } else {
            let fnv_hash_type = i64_type.fn_type(&[i8_ptr.into(), i64_type.into()], false);
            let f = self.module.add_function("__fnv1a_hash", fnv_hash_type, None);
            let fnv_entry = self.context.append_basic_block(f, "entry");
            let fnv_check = self.context.append_basic_block(f, "check");
            let fnv_loop = self.context.append_basic_block(f, "loop");
            let fnv_done = self.context.append_basic_block(f, "done");

            self.builder.position_at_end(fnv_entry);
            let fnv_offset = i64_type.const_int(0xcbf29ce484222325, false);
            let fnv_prime = i64_type.const_int(0x100000001b3, false);
            let fnv_zero = i64_type.const_int(0, false);
            let fnv_one = i64_type.const_int(1, false);
            let fnv_key_ptr = f.get_nth_param(0).unwrap().into_pointer_value();
            let fnv_key_len = f.get_nth_param(1).unwrap().into_int_value();
            let h_alloca = self.builder.build_alloca(i64_type, "h").unwrap();
            self.builder.build_store(h_alloca, fnv_offset).unwrap();
            let i_alloca = self.builder.build_alloca(i64_type, "i").unwrap();
            self.builder.build_store(i_alloca, fnv_zero).unwrap();
            self.builder.build_unconditional_branch(fnv_check).unwrap();

            self.builder.position_at_end(fnv_check);
            let i_val = self.builder.build_load(i_alloca, "i").unwrap().into_int_value();
            let cmp = self.builder.build_int_compare(inkwell::IntPredicate::SLT, i_val, fnv_key_len, "cmp").unwrap();
            self.builder.build_conditional_branch(cmp, fnv_loop, fnv_done).unwrap();

            self.builder.position_at_end(fnv_loop);
            let i2 = self.builder.build_load(i_alloca, "i2").unwrap().into_int_value();
            let byte_ptr = unsafe { self.builder.build_gep(fnv_key_ptr, &[i2], "byte_ptr").unwrap() };
            let byte = self.builder.build_load(byte_ptr, "byte").unwrap();
            let byte_i64 = self.builder.build_int_z_extend(byte.into_int_value(), i64_type, "byte_ext").unwrap();
            let cur = self.builder.build_load(h_alloca, "cur").unwrap().into_int_value();
            let xored = self.builder.build_xor(cur, byte_i64, "xored").unwrap();
            let mul = self.builder.build_int_mul(xored, fnv_prime, "mul").unwrap();
            self.builder.build_store(h_alloca, mul).unwrap();
            let i_next = self.builder.build_int_add(i2, fnv_one, "i_next").unwrap();
            self.builder.build_store(i_alloca, i_next).unwrap();
            self.builder.build_unconditional_branch(fnv_check).unwrap();

            self.builder.position_at_end(fnv_done);
            let result = self.builder.build_load(h_alloca, "result").unwrap().into_int_value();
            self.builder.build_return(Some(&result)).unwrap();
            f
        };

        // Helper: store key bytes into a slot.  For Int keys, store i64;
        // for String keys, store {i8*, i64} as two i64s.
        let store_key = |b: &Builder<'ctx>,
                         slot_base: inkwell::values::PointerValue<'ctx>,
                         key_param: &[inkwell::values::BasicValueEnum<'ctx>]| {
            if key_is_str {
                // key is (ptr, len) — store as two i64s
                let ptr_i64 = b.build_ptr_to_int(key_param[0].into_pointer_value(), i64_type, "kp").unwrap();
                let slot0 = b.build_bitcast(slot_base, i64_type.ptr_type(inkwell::AddressSpace::default()), "sk0").unwrap().into_pointer_value();
                b.build_store(slot0, ptr_i64).unwrap();
                let slot1 = unsafe { b.build_gep(slot0, &[i64_type.const_int(1, false)], "sk1").unwrap() };
                b.build_store(slot1, key_param[1].into_int_value()).unwrap();
            } else {
                let slot0 = b.build_bitcast(slot_base, i64_type.ptr_type(inkwell::AddressSpace::default()), "sk").unwrap().into_pointer_value();
                b.build_store(slot0, key_param[0].into_int_value()).unwrap();
            }
        };

        // Helper: store val bytes into a slot.
        let store_val = |b: &Builder<'ctx>,
                         slot_base: inkwell::values::PointerValue<'ctx>,
                         val_param: &[inkwell::values::BasicValueEnum<'ctx>]| {
            let val_off = b.build_int_add(
                b.build_ptr_to_int(slot_base, i64_type, "vo_base").unwrap(),
                i64_type.const_int(key_sz, false),
                "vo_off"
            ).unwrap();
            let val_ptr_raw = b.build_int_to_ptr(val_off, i64_type.ptr_type(inkwell::AddressSpace::default()), "vp").unwrap();
            if val_is_str {
                let ptr_i64 = b.build_ptr_to_int(val_param[0].into_pointer_value(), i64_type, "vp2").unwrap();
                b.build_store(val_ptr_raw, ptr_i64).unwrap();
                let val2 = unsafe { b.build_gep(val_ptr_raw, &[i64_type.const_int(1, false)], "vp3").unwrap() };
                b.build_store(val2, val_param[1].into_int_value()).unwrap();
            } else {
                b.build_store(val_ptr_raw, val_param[0].into_int_value()).unwrap();
            }
        };

        // Helper: load val from a slot, return as BasicValueEnum.
        let load_val = |b: &Builder<'ctx>,
                        slot_base: inkwell::values::PointerValue<'ctx>|
         -> inkwell::values::BasicValueEnum<'ctx> {
            let val_off = b.build_int_add(
                b.build_ptr_to_int(slot_base, i64_type, "lvo_base").unwrap(),
                i64_type.const_int(key_sz, false),
                "lvo_off"
            ).unwrap();
            let val_ptr = b.build_int_to_ptr(val_off, i64_type.ptr_type(inkwell::AddressSpace::default()), "lvp").unwrap();
            if val_is_str {
                let val_i64 = b.build_load(val_ptr, "lv_val").unwrap().into_int_value();
                let val2_ptr = unsafe { b.build_gep(val_ptr, &[i64_type.const_int(1, false)], "lvp2").unwrap() };
                let val2 = b.build_load(val2_ptr, "lv_val2").unwrap().into_int_value();
                let str_ptr = b.build_int_to_ptr(val_i64, i8_ptr, "lv_str_ptr").unwrap();
                let str_struct = self.string_type.const_zero();
                let str_struct = b.build_insert_value(str_struct, str_ptr, 0, "lv_ins1").unwrap().into_struct_value();
                let str_struct = b.build_insert_value(str_struct, val2, 1, "lv_ins2").unwrap().into_struct_value();
                str_struct.into()
            } else {
                b.build_load(val_ptr, "lv_val").unwrap()
            }
        };

        // Helper: compare key at a slot with the given key params.
        // Returns i64 0 (equal) or nonzero (not equal).
        let key_cmp = |b: &Builder<'ctx>,
                       f: inkwell::values::FunctionValue<'ctx>,
                       slot_base: inkwell::values::PointerValue<'ctx>,
                       key_param: &[inkwell::values::BasicValueEnum<'ctx>]|
         -> inkwell::values::IntValue<'ctx> {
            if key_is_str {
                // String keys stored as {i8*, i64}: compare len then content.
                let slot_i64 = b.build_bitcast(slot_base, i64_type.ptr_type(inkwell::AddressSpace::default()), "kc_slot_i64").unwrap().into_pointer_value();
                let slot_ptr_i64 = b.build_load(slot_i64, "kc_slot_ptr").unwrap().into_int_value();
                let slot_len_ptr = unsafe { b.build_gep(slot_i64, &[i64_type.const_int(1, false)], "kc_slot_len_p").unwrap() };
                let slot_len = b.build_load(slot_len_ptr, "kc_slot_len").unwrap().into_int_value();
                let slot_ptr = b.build_int_to_ptr(slot_ptr_i64, i8_ptr, "kc_slot_p").unwrap();
                let key_ptr = key_param[0].into_pointer_value();
                let key_len = key_param[1].into_int_value();
                // Lengths differ → not equal
                let len_eq = b.build_int_compare(inkwell::IntPredicate::EQ, slot_len, key_len, "kc_len_eq").unwrap();
                let i32_zero = self.context.i32_type().const_int(0, false);
                // Content comparison via memcmp(ptr1, ptr2, len) — memcmp takes i64 len
                let memcmp_call = b.build_call(memcmp_fn, &[slot_ptr.into(), key_ptr.into(), slot_len.into()], "kc_memcmp")
                    .unwrap().try_as_basic_value().left().unwrap().into_int_value();
                let content_eq = b.build_int_compare(inkwell::IntPredicate::EQ, memcmp_call, i32_zero, "kc_content_eq").unwrap();
                // Equal iff lengths match AND content matches → return NOT(both_eq) as i64
                let both_eq = b.build_and(len_eq, content_eq, "kc_both_eq").unwrap();
                let not_eq = b.build_not(both_eq, "kc_not_eq").unwrap();
                b.build_int_z_extend(not_eq, i64_type, "kc_result_i64").unwrap()
            } else {
                let slot_i64_ptr = b.build_bitcast(slot_base, i64_type.ptr_type(inkwell::AddressSpace::default()), "kc_i64").unwrap().into_pointer_value();
                let slot_key = b.build_load(slot_i64_ptr, "kc_key").unwrap().into_int_value();
                let cmp_val = b.build_int_compare(inkwell::IntPredicate::NE, slot_key, key_param[0].into_int_value(), "kc_cmp").unwrap();
                b.build_int_z_extend(cmp_val, i64_type, "kc_cmp_i64").unwrap()
            }
        };

        // Helper: probe for a slot.  Returns (slot_index, found_bool_as_i64).
        // If not found and for_set is true, returns the first empty slot index.
        // If cap == 0, returns (-1, 0).
        let probe = |b: &Builder<'ctx>,
                     f: inkwell::values::FunctionValue<'ctx>,
                     hdr: inkwell::values::PointerValue<'ctx>,
                     hash: inkwell::values::IntValue<'ctx>,
                     key_param: &[inkwell::values::BasicValueEnum<'ctx>],
                     for_set: bool|
         -> (inkwell::values::IntValue<'ctx>, inkwell::values::IntValue<'ctx>) {
            let zero = i64_type.const_int(0, false);
            let one = i64_type.const_int(1, false);

            let cap_ptr = b.build_struct_gep(hdr, 2, "cap_ptr").unwrap();
            let cap = b.build_load(cap_ptr, "cap").unwrap().into_int_value();
            let data_ptr = b.build_struct_gep(hdr, 0, "data_ptr").unwrap();
            let data = b.build_load(data_ptr, "data").unwrap().into_pointer_value();

            let no_cap = b.build_int_compare(inkwell::IntPredicate::EQ, cap, zero, "no_cap").unwrap();
            let probe_start = self.context.append_basic_block(f, "probe_start");
            let probe_done = self.context.append_basic_block(f, "probe_done");
            let probe_body = self.context.append_basic_block(f, "probe_body");
            let probe_found = self.context.append_basic_block(f, "probe_found");
            let probe_next = self.context.append_basic_block(f, "probe_next");
            let probe_empty = self.context.append_basic_block(f, "probe_empty");
            let probe_wrap = self.context.append_basic_block(f, "probe_wrap");
            let probe_unwrap = self.context.append_basic_block(f, "probe_unwrap");
            b.build_conditional_branch(no_cap, probe_done, probe_start).unwrap();

            // start: idx = hash % cap
            b.position_at_end(probe_start);
            let idx = b.build_int_unsigned_rem(hash, cap, "probe_idx").unwrap();
            let start_idx = b.build_alloca(i64_type, "start_idx").unwrap();
            b.build_store(start_idx, idx).unwrap();
            let first_empty = b.build_alloca(i64_type, "first_empty").unwrap();
            b.build_store(first_empty, i64_type.const_int(u64::MAX, false)).unwrap();
            b.build_unconditional_branch(probe_body).unwrap();

            // body: check slot
            b.position_at_end(probe_body);
            let cur_idx = b.build_load(start_idx, "cur_idx").unwrap().into_int_value();
            // data + cur_idx * slot_sz
            let byte_off = b.build_int_mul(cur_idx, i64_type.const_int(slot_sz, false), "byte_off").unwrap();
            let slot_ptr = unsafe { b.build_gep(data, &[byte_off], "slot_ptr").unwrap() };
            // occupied flag at offset key_sz + val_sz
            let occ_off = b.build_int_add(byte_off, i64_type.const_int(key_sz + val_sz, false), "occ_off").unwrap();
            let occ_ptr_raw = unsafe { b.build_gep(data, &[occ_off], "occ_ptr").unwrap() };
            let occ_ptr = b.build_pointer_cast(occ_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "occ_ptr_typed").unwrap();
            let occupied = b.build_load(occ_ptr, "occupied").unwrap().into_int_value();
            let is_occ = b.build_int_compare(inkwell::IntPredicate::NE, occupied, zero, "is_occ").unwrap();

            // Found occupied slot — check if key matches
            b.build_conditional_branch(is_occ, probe_found, probe_empty).unwrap();

            // found: compare keys
            b.position_at_end(probe_found);
            let cmp = key_cmp(b, f, slot_ptr, key_param);
            let keys_eq = b.build_int_compare(inkwell::IntPredicate::EQ, cmp, zero, "keys_eq").unwrap();
            b.build_conditional_branch(keys_eq, probe_done, probe_next).unwrap();

            // next: idx = (idx + 1) % cap
            b.position_at_end(probe_next);
            let next_idx = b.build_int_unsigned_rem(
                b.build_int_add(cur_idx, one, "next_idx").unwrap(),
                cap,
                "next_idx_mod"
            ).unwrap();
            b.build_store(start_idx, next_idx).unwrap();
            // Check if we've wrapped around
            let wrapped = b.build_int_compare(inkwell::IntPredicate::EQ, next_idx, cur_idx, "wrapped").unwrap();
            b.build_conditional_branch(wrapped, probe_wrap, probe_body).unwrap();

            // wrap: check if we've returned to start
            b.position_at_end(probe_wrap);
            let start = b.build_load(start_idx, "start").unwrap().into_int_value();
            let full_wrap = b.build_int_compare(inkwell::IntPredicate::EQ, start, idx, "full_wrap").unwrap();
            b.build_conditional_branch(full_wrap, probe_done, probe_unwrap).unwrap();
            b.position_at_end(probe_unwrap);
            b.build_unconditional_branch(probe_body).unwrap();

            // empty: record first empty slot and continue
            b.position_at_end(probe_empty);
            let first_val = b.build_load(first_empty, "fe").unwrap().into_int_value();
            let is_max = b.build_int_compare(inkwell::IntPredicate::EQ, first_val, i64_type.const_int(u64::MAX, false), "is_max").unwrap();
            let fe_block = self.context.append_basic_block(f, "fe_store");
            let fe_skip = self.context.append_basic_block(f, "fe_skip");
            b.build_conditional_branch(is_max, fe_block, fe_skip).unwrap();
            b.position_at_end(fe_block);
            b.build_store(first_empty, cur_idx).unwrap();
            b.build_unconditional_branch(fe_skip).unwrap();
            b.position_at_end(fe_skip);
            b.build_unconditional_branch(probe_next).unwrap();

            // done: return (idx, found_flag)
            b.position_at_end(probe_done);
            // ponytail: removed malformed phi nodes. This closure is never invoked
            // (let _ = probe), so the blocks exist only for IR well-formedness.
            // Use allocas + stores in predecessor blocks; load here.
            let idx_alloca = b.build_alloca(i64_type, "ret_idx").unwrap();
            let found_alloca = b.build_alloca(i64_type, "ret_found").unwrap();
            b.build_store(idx_alloca, cur_idx).unwrap();
            b.build_store(found_alloca, b.build_load(occ_ptr, "occ_load").unwrap().into_int_value()).unwrap();
            b.build_unreachable().unwrap();
            (b.build_load(idx_alloca, "ret_idx_v").unwrap().into_int_value(),
             b.build_load(found_alloca, "ret_found_v").unwrap().into_int_value())
        };

        // Avoid unused variable warnings — probe is called inside each builtin.
        let _ = probe;

        // ==================================================================
        // {prefix}_new — allocate header, zero fields, return handle
        // ==================================================================
        {
            let fn_type = i64_type.fn_type(&[], false);
            let function = self.module.add_function(&format!("{}_new", prefix), fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
            let hdr_sz = i64_type.const_int(40, false); // 5 × i64
            let hdr = self.builder.build_call(malloc_fn, &[hdr_sz.into()], "map_hdr")
                .expect("malloc failed")
                .try_as_basic_value().left().expect("malloc void")
                .into_pointer_value();
            let hdr_ptr = self.builder.build_bitcast(hdr, header_ptr, "hdr_typed").unwrap().into_pointer_value();
            let zero = i64_type.const_int(0, false);
            // data = null
            let dptr = self.builder.build_struct_gep(hdr_ptr, 0, "dptr").unwrap();
            self.builder.build_store(dptr, self.i8_ptr_type().const_null()).unwrap();
            // len = 0
            let lptr = self.builder.build_struct_gep(hdr_ptr, 1, "lptr").unwrap();
            self.builder.build_store(lptr, zero).unwrap();
            // cap = 0
            let cptr = self.builder.build_struct_gep(hdr_ptr, 2, "cptr").unwrap();
            self.builder.build_store(cptr, zero).unwrap();
            // key_size
            let kptr = self.builder.build_struct_gep(hdr_ptr, 3, "kptr").unwrap();
            self.builder.build_store(kptr, i64_type.const_int(key_sz, false)).unwrap();
            // val_size
            let vptr = self.builder.build_struct_gep(hdr_ptr, 4, "vptr").unwrap();
            self.builder.build_store(vptr, i64_type.const_int(val_sz, false)).unwrap();

            let handle = self.builder.build_ptr_to_int(hdr, i64_type, "map_handle").unwrap();
            let _ = self.builder.build_return(Some(&handle));
            self.functions.insert(format!("{}_new", prefix), function);
        }

        // ==================================================================
        // {prefix}_set(handle, key..., val...) -> handle
        // ==================================================================
        {
            let key_params: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = if key_is_str {
                vec![i8_ptr.into(), i64_type.into()]
            } else {
                vec![i64_type.into()]
            };
            let val_params: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = if val_is_str {
                vec![i8_ptr.into(), i64_type.into()]
            } else {
                vec![i64_type.into()]
            };
            let mut all_params: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = vec![i64_type.into()];
            all_params.extend(key_params.iter().cloned());
            all_params.extend(val_params.iter().cloned());
            let fn_type = i64_type.fn_type(&all_params, false);
            let func_name = format!("{}_set", prefix);
            let function = self.module.add_function(&func_name, fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).unwrap().into_int_value();
            let mut key_args: Vec<inkwell::values::BasicValueEnum> = Vec::new();
            let mut val_args: Vec<inkwell::values::BasicValueEnum> = Vec::new();
            let mut idx = 1u32;
            let n_key = if key_is_str { 2 } else { 1 };
            for _ in 0..n_key {
                key_args.push(function.get_nth_param(idx).unwrap());
                idx += 1;
            }
            for _ in 0..(if val_is_str { 2 } else { 1 }) {
                val_args.push(function.get_nth_param(idx).unwrap());
                idx += 1;
            }

            let hdr_ptr = hdr_from(&self.builder, handle);
            let cap_ptr = self.builder.build_struct_gep(hdr_ptr, 2, "cap_ptr").unwrap();
            let cap = self.builder.build_load(cap_ptr, "cap").unwrap().into_int_value();
            let len_ptr = self.builder.build_struct_gep(hdr_ptr, 1, "len_ptr").unwrap();
            let len = self.builder.build_load(len_ptr, "len").unwrap().into_int_value();
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr").unwrap();
            let data = self.builder.build_load(data_ptr, "data").unwrap().into_pointer_value();

            let zero = i64_type.const_int(0, false);
            let one = i64_type.const_int(1, false);

            // Compute hash via real LLVM function (allocas in its own entry block).
            let hash = if key_is_str {
                self.builder.build_call(fnv_func, &[
                    key_args[0].into_pointer_value().into(),
                    key_args[1].into_int_value().into(),
                ], "hash").unwrap().try_as_basic_value().left().unwrap().into_int_value()
            } else {
                splitmix64(&self.builder, key_args[0].into_int_value())
            };

            // Loop counters — alloca in entry block so mem2reg promotes to SSA.
            self.builder.position_at_end(entry);
            let z_counter = self.builder.build_alloca(i64_type, "z_counter").unwrap();
            self.builder.build_store(z_counter, zero).unwrap();
            let p_counter = self.builder.build_alloca(i64_type, "p_counter").unwrap();
            self.builder.build_store(p_counter, one).unwrap();

            // Branch from entry to continuation — terminates entry block.
            let cont = self.context.append_basic_block(function, "cont");
            self.builder.build_unconditional_branch(cont).unwrap();
            self.builder.position_at_end(cont);

            let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
            let free_fn = *self.functions.get("free").expect("free not declared");
            let slot_size = i64_type.const_int(slot_sz, false);

            // Route: cap==0 → init_alloc, len>=cap → grow_rehash, else → probe
            let no_cap = self.builder.build_int_compare(inkwell::IntPredicate::EQ, cap, zero, "no_cap").unwrap();
            let needs_grow = self.builder.build_int_compare(inkwell::IntPredicate::SGE, len, cap, "needs_grow").unwrap();
            let should_grow = self.builder.build_or(no_cap, needs_grow, "should_grow").unwrap();
            let init_alloc = self.context.append_basic_block(function, "init_alloc");
            let grow_rehash = self.context.append_basic_block(function, "grow_rehash");
            let probe_block = self.context.append_basic_block(function, "probe");
            self.builder.build_conditional_branch(should_grow, init_alloc, probe_block).unwrap();

            // === init_alloc: cap==0, allocate initial4 slots, zero occupied ===
            self.builder.position_at_end(init_alloc);
            let new_cap_init = i64_type.const_int(4, false);
            let alloc_size_init = self.builder.build_int_mul(new_cap_init, slot_size, "alloc_sz").unwrap();
            let new_data_init = self.builder.build_call(malloc_fn, &[alloc_size_init.into()], "new_data_init")
                .unwrap().try_as_basic_value().left().unwrap().into_pointer_value();
            let z_cond = self.context.append_basic_block(function, "z_cond");
            let z_loop = self.context.append_basic_block(function, "z_loop");
            let z_done = self.context.append_basic_block(function, "z_done");
            self.builder.build_unconditional_branch(z_cond).unwrap();
            self.builder.position_at_end(z_cond);
            let z_c = self.builder.build_load(z_counter, "z_c").unwrap().into_int_value();
            let z_cmp = self.builder.build_int_compare(inkwell::IntPredicate::SLT, z_c, new_cap_init, "z_cmp").unwrap();
            self.builder.build_conditional_branch(z_cmp, z_loop, z_done).unwrap();
            self.builder.position_at_end(z_loop);
            let z_c2 = self.builder.build_load(z_counter, "z_c2").unwrap().into_int_value();
            let z_byte = self.builder.build_int_mul(z_c2, slot_size, "z_byte").unwrap();
            let z_occ = self.builder.build_int_add(z_byte, i64_type.const_int(key_sz + val_sz, false), "z_occ").unwrap();
            let z_ptr_raw = unsafe { self.builder.build_gep(new_data_init, &[z_occ], "z_ptr").unwrap() };
            let z_ptr = self.builder.build_pointer_cast(z_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "z_ptr_t").unwrap();
            self.builder.build_store(z_ptr, zero).unwrap();
            let z_next = self.builder.build_int_add(z_c2, one, "z_next").unwrap();
            self.builder.build_store(z_counter, z_next).unwrap();
            self.builder.build_unconditional_branch(z_cond).unwrap();
            self.builder.position_at_end(z_done);
            self.builder.build_store(data_ptr, new_data_init).unwrap();
            self.builder.build_store(cap_ptr, new_cap_init).unwrap();
            self.builder.build_unconditional_branch(probe_block).unwrap();

            // === grow_rehash: len>=cap, alloc 2*cap, rehash, free old ===
            self.builder.position_at_end(grow_rehash);
            let new_cap_grow = self.builder.build_int_mul(cap, i64_type.const_int(2, false), "new_cap_grow").unwrap();
            let alloc_sz_grow = self.builder.build_int_mul(new_cap_grow, slot_size, "alloc_sz_grow").unwrap();
            let new_data_grow = self.builder.build_call(malloc_fn, &[alloc_sz_grow.into()], "new_data_grow")
                .unwrap().try_as_basic_value().left().unwrap().into_pointer_value();
            // Zero new buffer occupied flags.
            let gz_cond = self.context.append_basic_block(function, "gz_cond");
            let gz_loop = self.context.append_basic_block(function, "gz_loop");
            let gz_done = self.context.append_basic_block(function, "gz_done");
            self.builder.build_unconditional_branch(gz_cond).unwrap();
            self.builder.position_at_end(gz_cond);
            let gz_c = self.builder.build_load(z_counter, "gz_c").unwrap().into_int_value();
            let gz_cmp = self.builder.build_int_compare(inkwell::IntPredicate::SLT, gz_c, new_cap_grow, "gz_cmp").unwrap();
            self.builder.build_conditional_branch(gz_cmp, gz_loop, gz_done).unwrap();
            self.builder.position_at_end(gz_loop);
            let gz_c2 = self.builder.build_load(z_counter, "gz_c2").unwrap().into_int_value();
            let gz_byte = self.builder.build_int_mul(gz_c2, slot_size, "gz_byte").unwrap();
            let gz_occ = self.builder.build_int_add(gz_byte, i64_type.const_int(key_sz + val_sz, false), "gz_occ").unwrap();
            let gz_ptr_raw = unsafe { self.builder.build_gep(new_data_grow, &[gz_occ], "gz_ptr").unwrap() };
            let gz_ptr = self.builder.build_pointer_cast(gz_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "gz_ptr_t").unwrap();
            self.builder.build_store(gz_ptr, zero).unwrap();
            let gz_next = self.builder.build_int_add(gz_c2, one, "gz_next").unwrap();
            self.builder.build_store(z_counter, gz_next).unwrap();
            self.builder.build_unconditional_branch(gz_cond).unwrap();
            self.builder.position_at_end(gz_done);

            // Rehash loop: for each occupied slot in old buffer, read key, hash, insert into new.
            let rh_i = self.builder.build_alloca(i64_type, "rh_i").unwrap();
            self.builder.build_store(rh_i, zero).unwrap();
            let rh_cond = self.context.append_basic_block(function, "rh_cond");
            let rh_body = self.context.append_basic_block(function, "rh_body");
            let rh_done = self.context.append_basic_block(function, "rh_done");
            self.builder.build_unconditional_branch(rh_cond).unwrap();

            self.builder.position_at_end(rh_cond);
            let rh_c = self.builder.build_load(rh_i, "rh_c").unwrap().into_int_value();
            let rh_cmp = self.builder.build_int_compare(inkwell::IntPredicate::SLT, rh_c, cap, "rh_cmp").unwrap();
            self.builder.build_conditional_branch(rh_cmp, rh_body, rh_done).unwrap();

            self.builder.position_at_end(rh_body);
            let rh_c2 = self.builder.build_load(rh_i, "rh_c2").unwrap().into_int_value();
            let rh_byte = self.builder.build_int_mul(rh_c2, slot_size, "rh_byte").unwrap();
            // Check if slot is occupied.
            let rh_occ_off = self.builder.build_int_add(rh_byte, i64_type.const_int(key_sz + val_sz, false), "rh_occ_off").unwrap();
            let rh_occ_ptr_raw = unsafe { self.builder.build_gep(data, &[rh_occ_off], "rh_occ_ptr").unwrap() };
            let rh_occ_ptr = self.builder.build_pointer_cast(rh_occ_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rh_occ_ptr_t").unwrap();
            let rh_occ = self.builder.build_load(rh_occ_ptr, "rh_occ").unwrap().into_int_value();
            let rh_is_occ = self.builder.build_int_compare(inkwell::IntPredicate::EQ, rh_occ, one, "rh_is_occ").unwrap();
            let rh_skip = self.context.append_basic_block(function, "rh_skip");
            let rh_reinsert = self.context.append_basic_block(function, "rh_reinsert");
            self.builder.build_conditional_branch(rh_is_occ, rh_reinsert, rh_skip).unwrap();

            self.builder.position_at_end(rh_skip);
            let rh_next = self.builder.build_int_add(rh_c2, one, "rh_next").unwrap();
            self.builder.build_store(rh_i, rh_next).unwrap();
            self.builder.build_unconditional_branch(rh_cond).unwrap();

            // Reinsert occupied entry into new buffer.
            self.builder.position_at_end(rh_reinsert);
            let rh_slot = unsafe { self.builder.build_gep(data, &[rh_byte], "rh_slot").unwrap() };
            // Read key from old slot, hash it, find empty in new buffer, copy key+val.
            if key_is_str {
                // String key: load {i8*, i64} from old slot.
                let rk_ptr_raw = unsafe { self.builder.build_gep(rh_slot, &[i64_type.const_int(0, false)], "rk_ptr_raw").unwrap() };
                let rk_ptr = self.builder.build_pointer_cast(rk_ptr_raw, i8_ptr.ptr_type(inkwell::AddressSpace::default()), "rk_ptr").unwrap();
                let rk_ptr_l = self.builder.build_load(rk_ptr, "rk_ptr_l").unwrap().into_pointer_value();
                let rk_len_off = i64_type.const_int(8, false);
                let rk_len_ptr_raw = unsafe { self.builder.build_gep(rh_slot, &[rk_len_off], "rk_len_ptr_raw").unwrap() };
                let rk_len_ptr = self.builder.build_pointer_cast(rk_len_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rk_len_ptr").unwrap();
                let rk_len = self.builder.build_load(rk_len_ptr, "rk_len").unwrap().into_int_value();
                let rh_hash_val = self.builder.build_call(fnv_func, &[rk_ptr_l.into(), rk_len.into()], "rh_hash")
                    .unwrap().try_as_basic_value().left().unwrap().into_int_value();
                // Find empty slot in new buffer via linear probe.
                let rh_idx = self.builder.build_int_unsigned_rem(rh_hash_val, new_cap_grow, "rh_idx").unwrap();
                let rh_p = self.builder.build_alloca(i64_type, "rh_p").unwrap();
                self.builder.build_store(rh_p, one).unwrap();
                let rp_cond = self.context.append_basic_block(function, "rp_cond");
                let rp_body = self.context.append_basic_block(function, "rp_body");
                let rp_found = self.context.append_basic_block(function, "rp_found");
                // Check initial slot.
                let rh_boff0 = self.builder.build_int_mul(rh_idx, slot_size, "rh_boff0").unwrap();
                let rh_occ0_off = self.builder.build_int_add(rh_boff0, i64_type.const_int(key_sz + val_sz, false), "rh_occ0_off").unwrap();
                let rh_occ0_raw = unsafe { self.builder.build_gep(new_data_grow, &[rh_occ0_off], "rh_occ0").unwrap() };
                let rh_occ0_ptr = self.builder.build_pointer_cast(rh_occ0_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rh_occ0_t").unwrap();
                let rh_occ0 = self.builder.build_load(rh_occ0_ptr, "rh_occ0_v").unwrap().into_int_value();
                let rh_empty0 = self.builder.build_int_compare(inkwell::IntPredicate::EQ, rh_occ0, zero, "rh_empty0").unwrap();
                self.builder.build_conditional_branch(rh_empty0, rp_found, rp_cond).unwrap();
                self.builder.position_at_end(rp_cond);
                let rh_pc = self.builder.build_load(rh_p, "rh_pc").unwrap().into_int_value();
                let rh_pd = self.builder.build_int_compare(inkwell::IntPredicate::SLT, rh_pc, new_cap_grow, "rh_pd").unwrap();
                self.builder.build_conditional_branch(rh_pd, rp_body, rp_found).unwrap();
                self.builder.position_at_end(rp_body);
                let rh_pc2 = self.builder.build_load(rh_p, "rh_pc2").unwrap().into_int_value();
                let rh_psum = self.builder.build_int_add(rh_idx, rh_pc2, "rh_psum").unwrap();
                let rh_pidx = self.builder.build_int_unsigned_rem(rh_psum, new_cap_grow, "rh_pidx").unwrap();
                let rh_pboff = self.builder.build_int_mul(rh_pidx, slot_size, "rh_pboff").unwrap();
                let rh_pocc_off = self.builder.build_int_add(rh_pboff, i64_type.const_int(key_sz + val_sz, false), "rh_pocc_off").unwrap();
                let rh_pocc_raw = unsafe { self.builder.build_gep(new_data_grow, &[rh_pocc_off], "rh_pocc").unwrap() };
                let rh_pocc_ptr = self.builder.build_pointer_cast(rh_pocc_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rh_pocc_t").unwrap();
                let rh_pocc = self.builder.build_load(rh_pocc_ptr, "rh_pocc_v").unwrap().into_int_value();
                let rh_pempty = self.builder.build_int_compare(inkwell::IntPredicate::EQ, rh_pocc, zero, "rh_pempty").unwrap();
                let rp_inc = self.context.append_basic_block(function, "rp_inc");
                self.builder.build_conditional_branch(rh_pempty, rp_found, rp_inc).unwrap();
                self.builder.position_at_end(rp_inc);
                let rh_pn = self.builder.build_int_add(rh_pc2, one, "rh_pn").unwrap();
                self.builder.build_store(rh_p, rh_pn).unwrap();
                self.builder.build_unconditional_branch(rp_cond).unwrap();
                self.builder.position_at_end(rp_found);
                let rp_sum = self.builder.build_load(rh_p, "rp_sum").unwrap().into_int_value();
                // Probe counter minus1 gives the offset from initial index.
                let rp_idx = self.builder.build_int_sub(rp_sum, one, "rp_idx").unwrap();
                let rp_final = self.builder.build_int_unsigned_rem(
                    self.builder.build_int_add(rh_idx, rp_idx, "rp_final_sum").unwrap(),
                    new_cap_grow, "rp_final"
                ).unwrap();
                let rp_boff = self.builder.build_int_mul(rp_final, slot_size, "rp_boff").unwrap();
                let rp_slot = unsafe { self.builder.build_gep(new_data_grow, &[rp_boff], "rp_slot").unwrap() };
                // Copy string key: store pointer and length.
                let rk_dst_ptr_raw = unsafe { self.builder.build_gep(rp_slot, &[i64_type.const_int(0, false)], "rk_dst_raw").unwrap() };
                let rk_dst_ptr = self.builder.build_pointer_cast(rk_dst_ptr_raw, i8_ptr.ptr_type(inkwell::AddressSpace::default()), "rk_dst").unwrap();
                self.builder.build_store(rk_dst_ptr, rk_ptr_l).unwrap();
                let rk_dst_len_off = i64_type.const_int(8, false);
                let rk_dst_len_raw = unsafe { self.builder.build_gep(rp_slot, &[rk_dst_len_off], "rk_dst_len_raw").unwrap() };
                let rk_dst_len = self.builder.build_pointer_cast(rk_dst_len_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rk_dst_len").unwrap();
                self.builder.build_store(rk_dst_len, rk_len).unwrap();
                // Copy value (same approach for string or int val).
                if val_is_str {
                    let rv_off = i64_type.const_int(key_sz, false);
                    let rv_slot = unsafe { self.builder.build_gep(rp_slot, &[rv_off], "rv_slot").unwrap() };
                    let rv_dst_raw = unsafe { self.builder.build_gep(rv_slot, &[i64_type.const_int(0, false)], "rv_dst_raw").unwrap() };
                    let rv_dst_ptr = self.builder.build_pointer_cast(rv_dst_raw, i8_ptr.ptr_type(inkwell::AddressSpace::default()), "rv_dst").unwrap();
                    let rv_old_off = i64_type.const_int(key_sz, false);
                    let rv_old_slot = unsafe { self.builder.build_gep(rh_slot, &[rv_old_off], "rv_old_slot").unwrap() };
                    let rv_old_ptr_raw = unsafe { self.builder.build_gep(rv_old_slot, &[i64_type.const_int(0, false)], "rv_old_raw").unwrap() };
                    let rv_old_ptr = self.builder.build_pointer_cast(rv_old_ptr_raw, i8_ptr.ptr_type(inkwell::AddressSpace::default()), "rv_old_ptr").unwrap();
                    let rv_old_ptr_l = self.builder.build_load(rv_old_ptr, "rv_old_ptr_l").unwrap().into_pointer_value();
                    self.builder.build_store(rv_dst_ptr, rv_old_ptr_l).unwrap();
                    let rv_old_len_off = i64_type.const_int(8, false);
                    let rv_old_len_raw = unsafe { self.builder.build_gep(rv_old_slot, &[rv_old_len_off], "rv_old_len_raw").unwrap() };
                    let rv_old_len_ptr = self.builder.build_pointer_cast(rv_old_len_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rv_old_len_ptr").unwrap();
                    let rv_old_len = self.builder.build_load(rv_old_len_ptr, "rv_old_len").unwrap().into_int_value();
                    let rv_dst_len_off2 = i64_type.const_int(8, false);
                    let rv_dst_len_raw2 = unsafe { self.builder.build_gep(rv_slot, &[rv_dst_len_off2], "rv_dst_len_raw2").unwrap() };
                    let rv_dst_len_ptr2 = self.builder.build_pointer_cast(rv_dst_len_raw2, i64_type.ptr_type(inkwell::AddressSpace::default()), "rv_dst_len_ptr2").unwrap();
                    self.builder.build_store(rv_dst_len_ptr2, rv_old_len).unwrap();
                } else {
                    // Int val: copy i64.
                    let rv_off = i64_type.const_int(key_sz, false);
                    let rv_dst_raw = unsafe { self.builder.build_gep(rp_slot, &[rv_off], "rv_dst").unwrap() };
                    let rv_dst_ptr = self.builder.build_pointer_cast(rv_dst_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rv_dst_t").unwrap();
                    let rv_old_raw = unsafe { self.builder.build_gep(rh_slot, &[rv_off], "rv_old").unwrap() };
                    let rv_old_ptr = self.builder.build_pointer_cast(rv_old_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rv_old_t").unwrap();
                    let rv_old_val = self.builder.build_load(rv_old_ptr, "rv_old_val").unwrap().into_int_value();
                    self.builder.build_store(rv_dst_ptr, rv_old_val).unwrap();
                }
                // Mark new slot occupied.
                let rp_occ_off = self.builder.build_int_add(rp_boff, i64_type.const_int(key_sz + val_sz, false), "rp_occ_off").unwrap();
                let rp_occ_raw = unsafe { self.builder.build_gep(new_data_grow, &[rp_occ_off], "rp_occ").unwrap() };
                let rp_occ_ptr = self.builder.build_pointer_cast(rp_occ_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rp_occ_t").unwrap();
                self.builder.build_store(rp_occ_ptr, one).unwrap();
            } else {
                // Int key: read i64, splitmix64 hash, find empty slot, copy key+val.
                let rk_ptr_raw = unsafe { self.builder.build_gep(rh_slot, &[i64_type.const_int(0, false)], "rk_ptr_raw").unwrap() };
                let rk_ptr = self.builder.build_pointer_cast(rk_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rk_ptr").unwrap();
                let rk_val = self.builder.build_load(rk_ptr, "rk_val").unwrap().into_int_value();
                let rh_hash_val = splitmix64(&self.builder, rk_val);
                let rh_idx = self.builder.build_int_unsigned_rem(rh_hash_val, new_cap_grow, "rh_idx").unwrap();
                let rh_p = self.builder.build_alloca(i64_type, "rh_p").unwrap();
                self.builder.build_store(rh_p, one).unwrap();
                let rp_cond = self.context.append_basic_block(function, "rp_cond");
                let rp_body = self.context.append_basic_block(function, "rp_body");
                let rp_found = self.context.append_basic_block(function, "rp_found");
                let rh_boff0 = self.builder.build_int_mul(rh_idx, slot_size, "rh_boff0").unwrap();
                let rh_occ0_off = self.builder.build_int_add(rh_boff0, i64_type.const_int(key_sz + val_sz, false), "rh_occ0_off").unwrap();
                let rh_occ0_raw = unsafe { self.builder.build_gep(new_data_grow, &[rh_occ0_off], "rh_occ0").unwrap() };
                let rh_occ0_ptr = self.builder.build_pointer_cast(rh_occ0_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rh_occ0_t").unwrap();
                let rh_occ0 = self.builder.build_load(rh_occ0_ptr, "rh_occ0_v").unwrap().into_int_value();
                let rh_empty0 = self.builder.build_int_compare(inkwell::IntPredicate::EQ, rh_occ0, zero, "rh_empty0").unwrap();
                self.builder.build_conditional_branch(rh_empty0, rp_found, rp_cond).unwrap();
                self.builder.position_at_end(rp_cond);
                let rh_pc = self.builder.build_load(rh_p, "rh_pc").unwrap().into_int_value();
                let rh_pd = self.builder.build_int_compare(inkwell::IntPredicate::SLT, rh_pc, new_cap_grow, "rh_pd").unwrap();
                self.builder.build_conditional_branch(rh_pd, rp_body, rp_found).unwrap();
                self.builder.position_at_end(rp_body);
                let rh_pc2 = self.builder.build_load(rh_p, "rh_pc2").unwrap().into_int_value();
                let rh_psum = self.builder.build_int_add(rh_idx, rh_pc2, "rh_psum").unwrap();
                let rh_pidx = self.builder.build_int_unsigned_rem(rh_psum, new_cap_grow, "rh_pidx").unwrap();
                let rh_pboff = self.builder.build_int_mul(rh_pidx, slot_size, "rh_pboff").unwrap();
                let rh_pocc_off = self.builder.build_int_add(rh_pboff, i64_type.const_int(key_sz + val_sz, false), "rh_pocc_off").unwrap();
                let rh_pocc_raw = unsafe { self.builder.build_gep(new_data_grow, &[rh_pocc_off], "rh_pocc").unwrap() };
                let rh_pocc_ptr = self.builder.build_pointer_cast(rh_pocc_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rh_pocc_t").unwrap();
                let rh_pocc = self.builder.build_load(rh_pocc_ptr, "rh_pocc_v").unwrap().into_int_value();
                let rh_pempty = self.builder.build_int_compare(inkwell::IntPredicate::EQ, rh_pocc, zero, "rh_pempty").unwrap();
                let rp_inc = self.context.append_basic_block(function, "rp_inc");
                self.builder.build_conditional_branch(rh_pempty, rp_found, rp_inc).unwrap();
                self.builder.position_at_end(rp_inc);
                let rh_pn = self.builder.build_int_add(rh_pc2, one, "rh_pn").unwrap();
                self.builder.build_store(rh_p, rh_pn).unwrap();
                self.builder.build_unconditional_branch(rp_cond).unwrap();
                self.builder.position_at_end(rp_found);
                let rp_sum = self.builder.build_load(rh_p, "rp_sum").unwrap().into_int_value();
                let rp_idx = self.builder.build_int_sub(rp_sum, one, "rp_idx").unwrap();
                let rp_final = self.builder.build_int_unsigned_rem(
                    self.builder.build_int_add(rh_idx, rp_idx, "rp_final_sum").unwrap(),
                    new_cap_grow, "rp_final"
                ).unwrap();
                let rp_boff = self.builder.build_int_mul(rp_final, slot_size, "rp_boff").unwrap();
                let rp_slot = unsafe { self.builder.build_gep(new_data_grow, &[rp_boff], "rp_slot").unwrap() };
                // Store key (i64).
                let rk_dst_raw = unsafe { self.builder.build_gep(rp_slot, &[i64_type.const_int(0, false)], "rk_dst_raw").unwrap() };
                let rk_dst_ptr = self.builder.build_pointer_cast(rk_dst_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rk_dst").unwrap();
                self.builder.build_store(rk_dst_ptr, rk_val).unwrap();
                // Copy value.
                if val_is_str {
                    let rv_off = i64_type.const_int(key_sz, false);
                    let rv_slot = unsafe { self.builder.build_gep(rp_slot, &[rv_off], "rv_slot").unwrap() };
                    let rv_dst_raw = unsafe { self.builder.build_gep(rv_slot, &[i64_type.const_int(0, false)], "rv_dst_raw").unwrap() };
                    let rv_dst_ptr = self.builder.build_pointer_cast(rv_dst_raw, i8_ptr.ptr_type(inkwell::AddressSpace::default()), "rv_dst").unwrap();
                    let rv_old_slot = unsafe { self.builder.build_gep(rh_slot, &[rv_off], "rv_old_slot").unwrap() };
                    let rv_old_raw = unsafe { self.builder.build_gep(rv_old_slot, &[i64_type.const_int(0, false)], "rv_old_raw").unwrap() };
                    let rv_old_ptr = self.builder.build_pointer_cast(rv_old_raw, i8_ptr.ptr_type(inkwell::AddressSpace::default()), "rv_old_ptr").unwrap();
                    let rv_old_ptr_l = self.builder.build_load(rv_old_ptr, "rv_old_ptr_l").unwrap().into_pointer_value();
                    self.builder.build_store(rv_dst_ptr, rv_old_ptr_l).unwrap();
                    let rv_old_len_off = i64_type.const_int(8, false);
                    let rv_old_len_raw = unsafe { self.builder.build_gep(rv_old_slot, &[rv_old_len_off], "rv_old_len_raw").unwrap() };
                    let rv_old_len_ptr = self.builder.build_pointer_cast(rv_old_len_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rv_old_len_ptr").unwrap();
                    let rv_old_len = self.builder.build_load(rv_old_len_ptr, "rv_old_len").unwrap().into_int_value();
                    let rv_dst_len_off = i64_type.const_int(8, false);
                    let rv_dst_len_raw = unsafe { self.builder.build_gep(rv_slot, &[rv_dst_len_off], "rv_dst_len_raw").unwrap() };
                    let rv_dst_len_ptr = self.builder.build_pointer_cast(rv_dst_len_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rv_dst_len_ptr").unwrap();
                    self.builder.build_store(rv_dst_len_ptr, rv_old_len).unwrap();
                } else {
                    let rv_off = i64_type.const_int(key_sz, false);
                    let rv_dst_raw = unsafe { self.builder.build_gep(rp_slot, &[rv_off], "rv_dst").unwrap() };
                    let rv_dst_ptr = self.builder.build_pointer_cast(rv_dst_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rv_dst_t").unwrap();
                    let rv_old_raw = unsafe { self.builder.build_gep(rh_slot, &[rv_off], "rv_old").unwrap() };
                    let rv_old_ptr = self.builder.build_pointer_cast(rv_old_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rv_old_t").unwrap();
                    let rv_old_val = self.builder.build_load(rv_old_ptr, "rv_old_val").unwrap().into_int_value();
                    self.builder.build_store(rv_dst_ptr, rv_old_val).unwrap();
                }
                let rp_occ_off = self.builder.build_int_add(rp_boff, i64_type.const_int(key_sz + val_sz, false), "rp_occ_off").unwrap();
                let rp_occ_raw = unsafe { self.builder.build_gep(new_data_grow, &[rp_occ_off], "rp_occ").unwrap() };
                let rp_occ_ptr = self.builder.build_pointer_cast(rp_occ_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "rp_occ_t").unwrap();
                self.builder.build_store(rp_occ_ptr, one).unwrap();
            }
            // Advance rehash counter and continue.
            let rh_next2 = self.builder.build_int_add(rh_c2, one, "rh_next2").unwrap();
            self.builder.build_store(rh_i, rh_next2).unwrap();
            self.builder.build_unconditional_branch(rh_cond).unwrap();

            self.builder.position_at_end(rh_done);
            // Free old data buffer and update header.
            self.builder.build_call(free_fn, &[data.into()], "free_old").unwrap();
            self.builder.build_store(data_ptr, new_data_grow).unwrap();
            self.builder.build_store(cap_ptr, new_cap_grow).unwrap();
            self.builder.build_unconditional_branch(probe_block).unwrap();

            // ==================================================================
            // probe_block — shared by grow and non-grow paths.
            // Non-grow: cap/data from header (unchanged).
            // Grow: header already updated by grow_needed_block above.
            // ==================================================================
            self.builder.position_at_end(probe_block);
            // Re-read cap/data from header (works for both paths).
            let cap_probe = self.builder.build_load(cap_ptr, "cap_p").unwrap().into_int_value();
            let data_probe = self.builder.build_load(data_ptr, "data_p").unwrap().into_pointer_value();
            let idx_probe = self.builder.build_int_unsigned_rem(hash, cap_probe, "idx_p").unwrap();
            // Check if first slot is occupied
            let boff_p = self.builder.build_int_mul(idx_probe, slot_size, "boff_p").unwrap();
            let slot_p = unsafe { self.builder.build_gep(data_probe, &[boff_p], "slot_p").unwrap() };
            let occ_off_p = self.builder.build_int_add(boff_p, i64_type.const_int(key_sz + val_sz, false), "occ_off_p").unwrap();
            let occ_ptr_p_raw = unsafe { self.builder.build_gep(data_probe, &[occ_off_p], "occ_ptr_p").unwrap() };
            let occ_ptr_p = self.builder.build_pointer_cast(occ_ptr_p_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "occ_ptr_p_typed").unwrap();
            let occ_p = self.builder.build_load(occ_ptr_p, "occ_p").unwrap().into_int_value();
            let is_occ_p = self.builder.build_int_compare(inkwell::IntPredicate::EQ, occ_p, zero, "is_occ_p").unwrap();
            let store_empty = self.context.append_basic_block(function, "store_empty");
            let init_check = self.context.append_basic_block(function, "init_check");
            let probe_loop = self.context.append_basic_block(function, "probe_loop");
            let store_done = self.context.append_basic_block(function, "store_done");
            self.builder.build_conditional_branch(is_occ_p, store_empty, init_check).unwrap();

            self.builder.position_at_end(store_empty);
            store_key(&self.builder, slot_p, &key_args);
            store_val(&self.builder, slot_p, &val_args);
            self.builder.build_store(occ_ptr_p, one).unwrap();
            // len++
            let len_cur = self.builder.build_load(len_ptr, "len_cur").unwrap().into_int_value();
            self.builder.build_store(len_ptr, self.builder.build_int_add(len_cur, one, "len_inc").unwrap()).unwrap();
            self.builder.build_unconditional_branch(store_done).unwrap();

            // Initial slot occupied — check if key matches for overwrite
            self.builder.position_at_end(init_check);
            let init_cmp = key_cmp(&self.builder, function, slot_p, &key_args);
            let init_eq = self.builder.build_int_compare(inkwell::IntPredicate::EQ, init_cmp, zero, "init_eq").unwrap();
            let init_overwrite = self.context.append_basic_block(function, "init_overwrite");
            self.builder.build_conditional_branch(init_eq, init_overwrite, probe_loop).unwrap();

            self.builder.position_at_end(init_overwrite);
            store_val(&self.builder, slot_p, &val_args);
            self.builder.build_unconditional_branch(store_done).unwrap();

            // Probe loop: linear scan for existing key or empty slot
            self.builder.position_at_end(probe_loop);
            let p_loop_check = self.context.append_basic_block(function, "p_loop_check");
            let p_loop_body = self.context.append_basic_block(function, "p_loop_body");
            self.builder.build_unconditional_branch(p_loop_check).unwrap();

            self.builder.position_at_end(p_loop_check);
            let p_c = self.builder.build_load(p_counter, "p_c").unwrap().into_int_value();
            let p_done = self.builder.build_int_compare(inkwell::IntPredicate::SLT, p_c, cap_probe, "p_done").unwrap();
            self.builder.build_conditional_branch(p_done, p_loop_body, store_done).unwrap();

            self.builder.position_at_end(p_loop_body);
            let p_c2 = self.builder.build_load(p_counter, "p_c2").unwrap().into_int_value();
            let p_idx = self.builder.build_int_unsigned_rem(
                self.builder.build_int_add(idx_probe, p_c2, "p_idx_sum").unwrap(),
                cap_probe,
                "p_idx_mod"
            ).unwrap();
            let p_boff = self.builder.build_int_mul(p_idx, slot_size, "p_boff").unwrap();
            let p_slot = unsafe { self.builder.build_gep(data_probe, &[p_boff], "p_slot").unwrap() };
            let p_occ_off = self.builder.build_int_add(p_boff, i64_type.const_int(key_sz + val_sz, false), "p_occ_off").unwrap();
            let p_occ_ptr_raw = unsafe { self.builder.build_gep(data_probe, &[p_occ_off], "p_occ_ptr").unwrap() };
            let p_occ_ptr = self.builder.build_pointer_cast(p_occ_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "p_occ_ptr_typed").unwrap();
            let p_occ = self.builder.build_load(p_occ_ptr, "p_occ").unwrap().into_int_value();
            let p_is_occ = self.builder.build_int_compare(inkwell::IntPredicate::EQ, p_occ, zero, "p_is_occ").unwrap();

            // If empty, store here
            let p_empty = self.context.append_basic_block(function, "p_empty");
            let p_occ_check = self.context.append_basic_block(function, "p_occ_check");
            self.builder.build_conditional_branch(p_is_occ, p_empty, p_occ_check).unwrap();

            self.builder.position_at_end(p_empty);
            store_key(&self.builder, p_slot, &key_args);
            store_val(&self.builder, p_slot, &val_args);
            self.builder.build_store(p_occ_ptr, one).unwrap();
            let len_cur2 = self.builder.build_load(len_ptr, "len_cur2").unwrap().into_int_value();
            self.builder.build_store(len_ptr, self.builder.build_int_add(len_cur2, one, "len_inc2").unwrap()).unwrap();
            self.builder.build_unconditional_branch(store_done).unwrap();

            // If occupied, check if key matches
            self.builder.position_at_end(p_occ_check);
            let p_cmp = key_cmp(&self.builder, function, p_slot, &key_args);
            let p_eq = self.builder.build_int_compare(inkwell::IntPredicate::EQ, p_cmp, zero, "p_eq").unwrap();
            let p_match = self.context.append_basic_block(function, "p_match");
            let p_cont = self.context.append_basic_block(function, "p_cont");
            self.builder.build_conditional_branch(p_eq, p_match, p_cont).unwrap();

            self.builder.position_at_end(p_match);
            store_val(&self.builder, p_slot, &val_args);
            self.builder.build_unconditional_branch(store_done).unwrap();

            self.builder.position_at_end(p_cont);
            let p_c_next = self.builder.build_int_add(p_c2, one, "p_c_next").unwrap();
            self.builder.build_store(p_counter, p_c_next).unwrap();
            self.builder.build_unconditional_branch(p_loop_check).unwrap();

            self.builder.position_at_end(store_done);
            let _ = self.builder.build_return(Some(&handle));
            self.functions.insert(func_name, function);
        }

        // ==================================================================
        // {prefix}_get(handle, key...) -> value (i64 or {i8*,i64})
        // ==================================================================
        {
            let key_params: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = if key_is_str {
                vec![i8_ptr.into(), i64_type.into()]
            } else {
                vec![i64_type.into()]
            };
            let mut all_params: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = vec![i64_type.into()];
            all_params.extend(key_params.iter().cloned());
            let fn_type = if val_is_str {
                self.string_type.fn_type(&all_params, false)
            } else {
                i64_type.fn_type(&all_params, false)
            };
            let func_name = format!("{}_get", prefix);
            let function = self.module.add_function(&func_name, fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).unwrap().into_int_value();
            let mut key_args: Vec<inkwell::values::BasicValueEnum> = Vec::new();
            let mut idx = 1u32;
            let n_key = if key_is_str { 2 } else { 1 };
            for _ in 0..n_key {
                key_args.push(function.get_nth_param(idx).unwrap());
                idx += 1;
            }

            let hdr_ptr = hdr_from(&self.builder, handle);
            let cap_ptr = self.builder.build_struct_gep(hdr_ptr, 2, "cap_ptr").unwrap();
            let cap = self.builder.build_load(cap_ptr, "cap").unwrap().into_int_value();
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr").unwrap();
            let data = self.builder.build_load(data_ptr, "data").unwrap().into_pointer_value();
            let zero = i64_type.const_int(0, false);
            let one = i64_type.const_int(1, false);

            // Compute hash via real LLVM function (allocas in its own entry block).
            let hash = if key_is_str {
                self.builder.build_call(fnv_func, &[
                    key_args[0].into_pointer_value().into(),
                    key_args[1].into_int_value().into(),
                ], "hash").unwrap().try_as_basic_value().left().unwrap().into_int_value()
            } else {
                splitmix64(&self.builder, key_args[0].into_int_value())
            };

            // Counter — alloca in entry block so mem2reg promotes to SSA.
            self.builder.position_at_end(entry);
            let g_counter = self.builder.build_alloca(i64_type, "g_counter").unwrap();
            self.builder.build_store(g_counter, zero).unwrap();

            // Branch from entry to continuation — terminates entry block.
            let g_cont = self.context.append_basic_block(function, "g_cont");
            self.builder.build_unconditional_branch(g_cont).unwrap();
            self.builder.position_at_end(g_cont);

            let no_cap = self.builder.build_int_compare(inkwell::IntPredicate::EQ, cap, zero, "no_cap").unwrap();
            let get_miss = self.context.append_basic_block(function, "get_miss");
            let get_probe = self.context.append_basic_block(function, "get_probe");
            self.builder.build_conditional_branch(no_cap, get_miss, get_probe).unwrap();

            self.builder.position_at_end(get_probe);
            let idx = self.builder.build_int_unsigned_rem(hash, cap, "g_idx").unwrap();
            let g_loop_check = self.context.append_basic_block(function, "g_loop_check");
            let g_loop_body = self.context.append_basic_block(function, "g_loop_body");
            let g_found = self.context.append_basic_block(function, "g_found");
            self.builder.build_unconditional_branch(g_loop_check).unwrap();

            self.builder.position_at_end(g_loop_check);
            let g_c = self.builder.build_load(g_counter, "g_c").unwrap().into_int_value();
            let g_done = self.builder.build_int_compare(inkwell::IntPredicate::SLT, g_c, cap, "g_done").unwrap();
            self.builder.build_conditional_branch(g_done, g_loop_body, get_miss).unwrap();

            self.builder.position_at_end(g_loop_body);
            let g_c2 = self.builder.build_load(g_counter, "g_c2").unwrap().into_int_value();
            let g_slot_idx = self.builder.build_int_unsigned_rem(
                self.builder.build_int_add(idx, g_c2, "g_sum").unwrap(), cap, "g_mod"
            ).unwrap();
            let g_boff = self.builder.build_int_mul(g_slot_idx, i64_type.const_int(slot_sz, false), "g_boff").unwrap();
            let g_slot = unsafe { self.builder.build_gep(data, &[g_boff], "g_slot").unwrap() };
            let g_occ_off = self.builder.build_int_add(g_boff, i64_type.const_int(key_sz + val_sz, false), "g_occ_off").unwrap();
            let g_occ_ptr_raw = unsafe { self.builder.build_gep(data, &[g_occ_off], "g_occ_ptr").unwrap() };
            let g_occ_ptr = self.builder.build_pointer_cast(g_occ_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "g_occ_ptr_typed").unwrap();
            let g_occ = self.builder.build_load(g_occ_ptr, "g_occ").unwrap().into_int_value();
            let g_is_occ = self.builder.build_int_compare(inkwell::IntPredicate::NE, g_occ, zero, "g_is_occ").unwrap();

            let g_check_key = self.context.append_basic_block(function, "g_check_key");
            let g_cont = self.context.append_basic_block(function, "g_cont");
            self.builder.build_conditional_branch(g_is_occ, g_check_key, g_cont).unwrap();

            self.builder.position_at_end(g_check_key);
            let g_cmp = key_cmp(&self.builder, function, g_slot, &key_args);
            let g_eq = self.builder.build_int_compare(inkwell::IntPredicate::EQ, g_cmp, zero, "g_eq").unwrap();
            self.builder.build_conditional_branch(g_eq, g_found, g_cont).unwrap();

            self.builder.position_at_end(g_cont);
            let g_c_next = self.builder.build_int_add(g_c2, one, "g_c_next").unwrap();
            self.builder.build_store(g_counter, g_c_next).unwrap();
            self.builder.build_unconditional_branch(g_loop_check).unwrap();

            self.builder.position_at_end(g_found);
            let g_val = load_val(&self.builder, g_slot);
            let _ = self.builder.build_return(Some(&g_val));

            self.builder.position_at_end(get_miss);
            let default_val: inkwell::values::BasicValueEnum<'ctx> = if val_is_str {
                self.string_type.const_zero().into()
            } else {
                zero.into()
            };
            let _ = self.builder.build_return(Some(&default_val));
            self.functions.insert(func_name, function);
        }

        // ==================================================================
        // {prefix}_contains(handle, key...) -> i64 (0 or 1)
        // ==================================================================
        {
            let key_params: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = if key_is_str {
                vec![i8_ptr.into(), i64_type.into()]
            } else {
                vec![i64_type.into()]
            };
            let mut all_params: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = vec![i64_type.into()];
            all_params.extend(key_params.iter().cloned());
            let fn_type = i64_type.fn_type(&all_params, false);
            let func_name = format!("{}_contains", prefix);
            let function = self.module.add_function(&func_name, fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).unwrap().into_int_value();
            let mut key_args: Vec<inkwell::values::BasicValueEnum> = Vec::new();
            let mut idx = 1u32;
            for _ in 0..(if key_is_str { 2 } else { 1 }) {
                key_args.push(function.get_nth_param(idx).unwrap());
                idx += 1;
            }

            let hdr_ptr = hdr_from(&self.builder, handle);
            let cap_ptr = self.builder.build_struct_gep(hdr_ptr, 2, "cap_ptr").unwrap();
            let cap = self.builder.build_load(cap_ptr, "cap").unwrap().into_int_value();
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr").unwrap();
            let data = self.builder.build_load(data_ptr, "data").unwrap().into_pointer_value();
            let zero = i64_type.const_int(0, false);
            let one = i64_type.const_int(1, false);

            // Compute hash via real LLVM function (allocas in its own entry block).
            let hash = if key_is_str {
                self.builder.build_call(fnv_func, &[
                    key_args[0].into_pointer_value().into(),
                    key_args[1].into_int_value().into(),
                ], "hash").unwrap().try_as_basic_value().left().unwrap().into_int_value()
            } else {
                splitmix64(&self.builder, key_args[0].into_int_value())
            };

            // Counter — alloca in entry block so mem2reg promotes to SSA.
            self.builder.position_at_end(entry);
            let c_counter = self.builder.build_alloca(i64_type, "c_counter").unwrap();
            self.builder.build_store(c_counter, zero).unwrap();

            // Branch from entry to continuation — terminates entry block.
            let c_cont = self.context.append_basic_block(function, "c_cont");
            self.builder.build_unconditional_branch(c_cont).unwrap();
            self.builder.position_at_end(c_cont);

            let no_cap = self.builder.build_int_compare(inkwell::IntPredicate::EQ, cap, zero, "no_cap").unwrap();
            let c_miss = self.context.append_basic_block(function, "c_miss");
            let c_probe = self.context.append_basic_block(function, "c_probe");
            self.builder.build_conditional_branch(no_cap, c_miss, c_probe).unwrap();

            self.builder.position_at_end(c_probe);
            let c_idx = self.builder.build_int_unsigned_rem(hash, cap, "c_idx").unwrap();
            let c_loop_check = self.context.append_basic_block(function, "c_loop_check");
            let c_loop_body = self.context.append_basic_block(function, "c_loop_body");
            let c_found = self.context.append_basic_block(function, "c_found");
            self.builder.build_unconditional_branch(c_loop_check).unwrap();

            self.builder.position_at_end(c_loop_check);
            let c_c = self.builder.build_load(c_counter, "c_c").unwrap().into_int_value();
            let c_done = self.builder.build_int_compare(inkwell::IntPredicate::SLT, c_c, cap, "c_done").unwrap();
            self.builder.build_conditional_branch(c_done, c_loop_body, c_miss).unwrap();

            self.builder.position_at_end(c_loop_body);
            let c_c2 = self.builder.build_load(c_counter, "c_c2").unwrap().into_int_value();
            let c_slot_idx = self.builder.build_int_unsigned_rem(
                self.builder.build_int_add(c_idx, c_c2, "c_sum").unwrap(), cap, "c_mod"
            ).unwrap();
            let c_boff = self.builder.build_int_mul(c_slot_idx, i64_type.const_int(slot_sz, false), "c_boff").unwrap();
            let c_slot = unsafe { self.builder.build_gep(data, &[c_boff], "c_slot").unwrap() };
            let c_occ_off = self.builder.build_int_add(c_boff, i64_type.const_int(key_sz + val_sz, false), "c_occ_off").unwrap();
            let c_occ_ptr_raw = unsafe { self.builder.build_gep(data, &[c_occ_off], "c_occ_ptr").unwrap() };
            let c_occ_ptr = self.builder.build_pointer_cast(c_occ_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "c_occ_ptr_typed").unwrap();
            let c_occ = self.builder.build_load(c_occ_ptr, "c_occ").unwrap().into_int_value();
            let c_is_occ = self.builder.build_int_compare(inkwell::IntPredicate::NE, c_occ, zero, "c_is_occ").unwrap();

            let c_check_key = self.context.append_basic_block(function, "c_check_key");
            let c_cont = self.context.append_basic_block(function, "c_cont");
            self.builder.build_conditional_branch(c_is_occ, c_check_key, c_cont).unwrap();

            self.builder.position_at_end(c_check_key);
            let c_cmp = key_cmp(&self.builder, function, c_slot, &key_args);
            let c_eq = self.builder.build_int_compare(inkwell::IntPredicate::EQ, c_cmp, zero, "c_eq").unwrap();
            self.builder.build_conditional_branch(c_eq, c_found, c_cont).unwrap();

            self.builder.position_at_end(c_cont);
            let c_c_next = self.builder.build_int_add(c_c2, one, "c_c_next").unwrap();
            self.builder.build_store(c_counter, c_c_next).unwrap();
            self.builder.build_unconditional_branch(c_loop_check).unwrap();

            self.builder.position_at_end(c_found);
            let _ = self.builder.build_return(Some(&one));
            self.builder.position_at_end(c_miss);
            let _ = self.builder.build_return(Some(&zero));
            self.functions.insert(func_name, function);
        }

        // ==================================================================
        // {prefix}_remove(handle, key...) -> handle
        // ==================================================================
        {
            let key_params: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = if key_is_str {
                vec![i8_ptr.into(), i64_type.into()]
            } else {
                vec![i64_type.into()]
            };
            let mut all_params: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> = vec![i64_type.into()];
            all_params.extend(key_params.iter().cloned());
            let fn_type = i64_type.fn_type(&all_params, false);
            let func_name = format!("{}_remove", prefix);
            let function = self.module.add_function(&func_name, fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).unwrap().into_int_value();
            let mut key_args: Vec<inkwell::values::BasicValueEnum> = Vec::new();
            let mut idx = 1u32;
            for _ in 0..(if key_is_str { 2 } else { 1 }) {
                key_args.push(function.get_nth_param(idx).unwrap());
                idx += 1;
            }

            let hdr_ptr = hdr_from(&self.builder, handle);
            let cap_ptr = self.builder.build_struct_gep(hdr_ptr, 2, "cap_ptr").unwrap();
            let cap = self.builder.build_load(cap_ptr, "cap").unwrap().into_int_value();
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr").unwrap();
            let data = self.builder.build_load(data_ptr, "data").unwrap().into_pointer_value();
            let len_ptr = self.builder.build_struct_gep(hdr_ptr, 1, "len_ptr").unwrap();
            let zero = i64_type.const_int(0, false);
            let one = i64_type.const_int(1, false);

            // Compute hash via real LLVM function (allocas in its own entry block).
            let hash = if key_is_str {
                self.builder.build_call(fnv_func, &[
                    key_args[0].into_pointer_value().into(),
                    key_args[1].into_int_value().into(),
                ], "hash").unwrap().try_as_basic_value().left().unwrap().into_int_value()
            } else {
                splitmix64(&self.builder, key_args[0].into_int_value())
            };

            // Counter — alloca in entry block so mem2reg promotes to SSA.
            self.builder.position_at_end(entry);
            let r_counter = self.builder.build_alloca(i64_type, "r_counter").unwrap();
            self.builder.build_store(r_counter, zero).unwrap();

            // Branch from entry to continuation — terminates entry block.
            let r_cont = self.context.append_basic_block(function, "r_cont");
            self.builder.build_unconditional_branch(r_cont).unwrap();
            self.builder.position_at_end(r_cont);

            let no_cap = self.builder.build_int_compare(inkwell::IntPredicate::EQ, cap, zero, "no_cap").unwrap();
            let r_done = self.context.append_basic_block(function, "r_done");
            let r_probe = self.context.append_basic_block(function, "r_probe");
            self.builder.build_conditional_branch(no_cap, r_done, r_probe).unwrap();

            self.builder.position_at_end(r_probe);
            let r_idx = self.builder.build_int_unsigned_rem(hash, cap, "r_idx").unwrap();
            let r_loop_check = self.context.append_basic_block(function, "r_loop_check");
            let r_loop_body = self.context.append_basic_block(function, "r_loop_body");
            let r_found = self.context.append_basic_block(function, "r_found");
            self.builder.build_unconditional_branch(r_loop_check).unwrap();

            self.builder.position_at_end(r_loop_check);
            let r_c = self.builder.build_load(r_counter, "r_c").unwrap().into_int_value();
            let r_done2 = self.builder.build_int_compare(inkwell::IntPredicate::SLT, r_c, cap, "r_done2").unwrap();
            self.builder.build_conditional_branch(r_done2, r_loop_body, r_done).unwrap();

            self.builder.position_at_end(r_loop_body);
            let r_c2 = self.builder.build_load(r_counter, "r_c2").unwrap().into_int_value();
            let r_slot_idx = self.builder.build_int_unsigned_rem(
                self.builder.build_int_add(r_idx, r_c2, "r_sum").unwrap(), cap, "r_mod"
            ).unwrap();
            let r_boff = self.builder.build_int_mul(r_slot_idx, i64_type.const_int(slot_sz, false), "r_boff").unwrap();
            let r_slot = unsafe { self.builder.build_gep(data, &[r_boff], "r_slot").unwrap() };
            let r_occ_off = self.builder.build_int_add(r_boff, i64_type.const_int(key_sz + val_sz, false), "r_occ_off").unwrap();
            let r_occ_ptr_raw = unsafe { self.builder.build_gep(data, &[r_occ_off], "r_occ_ptr").unwrap() };
            let r_occ_ptr = self.builder.build_pointer_cast(r_occ_ptr_raw, i64_type.ptr_type(inkwell::AddressSpace::default()), "r_occ_ptr_typed").unwrap();
            let r_occ = self.builder.build_load(r_occ_ptr, "r_occ").unwrap().into_int_value();
            let r_is_occ = self.builder.build_int_compare(inkwell::IntPredicate::NE, r_occ, zero, "r_is_occ").unwrap();

            let r_check_key = self.context.append_basic_block(function, "r_check_key");
            let r_cont = self.context.append_basic_block(function, "r_cont");
            self.builder.build_conditional_branch(r_is_occ, r_check_key, r_cont).unwrap();

            self.builder.position_at_end(r_check_key);
            let r_cmp = key_cmp(&self.builder, function, r_slot, &key_args);
            let r_eq = self.builder.build_int_compare(inkwell::IntPredicate::EQ, r_cmp, zero, "r_eq").unwrap();
            self.builder.build_conditional_branch(r_eq, r_found, r_cont).unwrap();

            self.builder.position_at_end(r_cont);
            let r_c_next = self.builder.build_int_add(r_c2, one, "r_c_next").unwrap();
            self.builder.build_store(r_counter, r_c_next).unwrap();
            self.builder.build_unconditional_branch(r_loop_check).unwrap();

            self.builder.position_at_end(r_found);
            // Set occupied to 0 (tombstone-free: just mark empty)
            self.builder.build_store(r_occ_ptr, zero).unwrap();
            // len--
            let r_len = self.builder.build_load(len_ptr, "r_len").unwrap().into_int_value();
            self.builder.build_store(len_ptr, self.builder.build_int_sub(r_len, one, "r_dec").unwrap()).unwrap();
            self.builder.build_unconditional_branch(r_done).unwrap();

            self.builder.position_at_end(r_done);
            let _ = self.builder.build_return(Some(&handle));
            self.functions.insert(func_name, function);
        }

        // ==================================================================
        // {prefix}_len(handle) -> i64
        // ==================================================================
        {
            let fn_type = i64_type.fn_type(&[i64_type.into()], false);
            let func_name = format!("{}_len", prefix);
            let function = self.module.add_function(&func_name, fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).unwrap().into_int_value();
            let hdr = hdr_from(&self.builder, handle);
            let lptr = self.builder.build_struct_gep(hdr, 1, "lptr").unwrap();
            let len = self.builder.build_load(lptr, "len").unwrap();
            let _ = self.builder.build_return(Some(&len));
            self.functions.insert(func_name, function);
        }

        // ==================================================================
        // {prefix}_free(handle) -> i64 (0)
        // ==================================================================
        {
            let fn_type = i64_type.fn_type(&[i64_type.into()], false);
            let func_name = format!("{}_free", prefix);
            let function = self.module.add_function(&func_name, fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).unwrap().into_int_value();
            let hdr = hdr_from(&self.builder, handle);
            let dptr = self.builder.build_struct_gep(hdr, 0, "dptr").unwrap();
            let data = self.builder.build_load(dptr, "data").unwrap().into_pointer_value();
            let free_fn = *self.functions.get("free").expect("free not declared");
            self.builder.build_call(free_fn, &[data.into()], "free_data").unwrap();
            let hdr_i8 = self.builder.build_bitcast(hdr, i8_ptr, "hdr_i8").unwrap().into_pointer_value();
            self.builder.build_call(free_fn, &[hdr_i8.into()], "free_hdr").unwrap();
            let zero = i64_type.const_int(0, false);
            let _ = self.builder.build_return(Some(&zero));
            self.functions.insert(func_name, function);
        }

        // Register fn_types
        let map_type = |kt: AhaType, vt: AhaType| AhaType::Map(Box::new(kt), Box::new(vt));
        let int_t = AhaType::Int;
        let str_t = AhaType::String;
        let kt = if key_is_str { str_t.clone() } else { int_t.clone() };
        let vt = if val_is_str { str_t.clone() } else { int_t.clone() };
        self.fn_types.insert(format!("{}_new", prefix), map_type(kt.clone(), vt.clone()));
        self.fn_types.insert(format!("{}_set", prefix), map_type(kt.clone(), vt.clone()));
        self.fn_types.insert(format!("{}_get", prefix), vt.clone());
        self.fn_types.insert(format!("{}_contains", prefix), int_t.clone());
        self.fn_types.insert(format!("{}_remove", prefix), map_type(kt.clone(), vt.clone()));
        self.fn_types.insert(format!("{}_len", prefix), int_t.clone());
        self.fn_types.insert(format!("{}_free", prefix), int_t.clone());
    }

    fn create_map_builtins(&mut self) {
        Self::diag_mark("3m: create_map_builtins start");

        // Map<Int, Int> — prefix "map_"
        self.emit_map_combo("map", 8, 8, false, false);

        // Map<String, Int> — prefix "map_string_key_"
        self.emit_map_combo("map_string_key", 16, 8, true, false);

        // Map<Int, String> — prefix "map_string_val_"
        self.emit_map_combo("map_string_val", 8, 16, false, true);

        // Map<String, String> — prefix "map_strings_"
        self.emit_map_combo("map_strings", 16, 16, true, true);

        Self::diag_mark("3n: create_map_builtins done");
    }

    fn compile_statement(&mut self, statement: &ast::Statement) -> Result<(), String> {
        match statement {
            ast::Statement::Let(let_stmt) => {
                let typed_val = self.compile_expression(&let_stmt.value)?;
                // Determine allocation type: prefer explicit annotation,
                // then fall back to inferred type from the expression.
                let alloc_type = if let Some(ref hint) = let_stmt.type_annotation {
                    let hint_type = AhaType::from_hint(hint)
                        .unwrap_or(AhaType::Int);
                    // If the struct_defs or enum_defs registry has a matching name.
                    let hint_type = if self.struct_defs.contains_key(hint) {
                        AhaType::Struct(hint.clone())
                    } else if self.enum_defs.contains_key(hint) {
                        AhaType::Enum(hint.clone())
                    } else {
                        hint_type
                    };
                    // Type-check: annotation must match the inferred type.
                    // Struct("Point") vs Struct("Point") or Enum("Color") vs Enum("Color") is compatible.
                    let compatible = match (&hint_type, &typed_val.aha_type) {
                        (AhaType::Struct(a), AhaType::Struct(b)) => a == b,
                        (AhaType::Enum(a), AhaType::Enum(b)) => a == b,
                        _ => hint_type == typed_val.aha_type,
                    };
                    if !compatible {
                        return Err(format!(
                            "Type mismatch: variable '{}' annotated as '{}' but value has type '{}'",
                            let_stmt.name.value, hint, typed_val.aha_type
                        ));
                    }
                    self.aha_type_to_llvm_type(&hint_type)?
                } else {
                    self.aha_type_to_llvm_type(&typed_val.aha_type)?
                };
                let pointer = self.builder.build_alloca(alloc_type, &let_stmt.name.value)
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(pointer, typed_val.value)
                    .map_err(|e| e.to_string())?;
                self.insert_variable(let_stmt.name.value.clone(), pointer, typed_val.aha_type);
            },
            ast::Statement::Expression(expr_stmt) => {
                self.compile_expression(&expr_stmt.expression)?;
            },
            ast::Statement::Return(ret_stmt) => {
                let typed_val = self.compile_expression(&ret_stmt.return_value)?;
                if self.has_heap_locals() {
                    let escaped = Self::find_heap_vars_in_expr(&ret_stmt.return_value);
                    self.insert_cleanup_inline(&escaped);
                }
                self.builder.build_return(Some(&typed_val.value))
                    .map_err(|e| e.to_string())?;
            },
            ast::Statement::Struct(_struct_def) => {
                // Struct definitions are compile-time metadata
            }
            ast::Statement::Import(_) => {
                // Import statements are handled by the compiler orchestrator
            }
            ast::Statement::Actor(_) => {
                // Actor definitions are registered as structs (actor = struct + thread)
            }
            ast::Statement::Enum(_) => {
                // Enum definitions are compile-time metadata
            }
        }
        Ok(())
    }

    fn compile_expression(&mut self, expression: &ast::Expression) -> Result<TypedValue<'ctx>, String> {
        match expression {
            ast::Expression::Integer(int_lit) => {
                let val = self.i64_type.const_int(int_lit.value as u64, false);
                Ok(TypedValue::int(val.into()))
            },
            ast::Expression::Identifier(ident) => {
                if let Some(info) = self.lookup_variable(&ident.value) {
                    let var_type = info.var_type.clone();
                    let ptr = info.ptr;
                    let loaded = self.builder.build_load(ptr, &ident.value)
                        .map_err(|e| e.to_string())?;
                    Ok(TypedValue::new(loaded, var_type))
                } else {
                    Err(format!("Variable '{}' not found", ident.value))
                }
            },
            ast::Expression::Infix(infix) => self.compile_infix(infix),
            ast::Expression::If(if_expr) => self.compile_if_expression(if_expr),
            ast::Expression::While(while_expr) => self.compile_while_expression(while_expr),
            ast::Expression::For(for_expr) => self.compile_for_expression(for_expr),
            ast::Expression::Boolean(bool_lit) => {
                let val = if bool_lit.value { 1 } else { 0 };
                Ok(TypedValue::bool_val(self.i64_type.const_int(val, false).into()))
            },
            ast::Expression::String(str_lit) => self.compile_string_literal(&str_lit.value),
            ast::Expression::Prefix(prefix) => self.compile_prefix_expression(prefix),
            ast::Expression::Function(func_lit) => self.compile_function(func_lit),
            ast::Expression::Call(call_expr) => self.compile_call(call_expr),
            ast::Expression::Array(arr_lit) => self.compile_array_literal(arr_lit),
            ast::Expression::Index(idx_expr) => self.compile_index_expression(idx_expr),
            ast::Expression::Range(range_expr) => self.compile_range_expression(range_expr),
            ast::Expression::Assignment(assign) => self.compile_assignment(assign),
            ast::Expression::StructLiteral(struct_lit) => self.compile_struct_literal(struct_lit),
            ast::Expression::FieldAccess(field_access) => self.compile_field_access(field_access),
            ast::Expression::ModuleAccess(ma) => {
                // module::name — resolve to the flat function/variable name
                // (compiler already merged pub items into global scope)
                self.compile_expression(&ast::Expression::Identifier(
                    ast::Identifier { value: ma.name.clone() }
                ))
            },
            ast::Expression::Break => {
                if let Some(&(_, break_block)) = self.loop_stack.last() {
                    self.builder.build_unconditional_branch(break_block)
                        .map_err(|e| e.to_string())?;
                }
                Ok(TypedValue::void(self.i64_type.const_int(0, false).into()))
            },
            ast::Expression::Continue => {
                if let Some(&(continue_block, _)) = self.loop_stack.last() {
                    self.builder.build_unconditional_branch(continue_block)
                        .map_err(|e| e.to_string())?;
                }
                Ok(TypedValue::void(self.i64_type.const_int(0, false).into()))
            },
            ast::Expression::Spawn(spawn_expr) => {
                // Phase 1: spawn ActorName { field: value, ... }
                // Compiles to: actor_spawn(handler_fn_ptr, state_ptr)
                // The handler function is a JIT function: fn(state: i64, msg: i64) -> i64
                // Convention: handler is the function named "handle" in the module.
                let struct_name = &spawn_expr.actor_name.value;
                let field_names: Vec<String> = match self.struct_defs.get(struct_name) {
                    Some(fields) => fields.iter().map(|(n, _)| n.clone()).collect(),
                    None => return Err(format!("Unknown actor type: {}", struct_name)),
                };

                let mut field_values = Vec::new();
                for field_name in &field_names {
                    let val = spawn_expr.fields.iter()
                        .find(|(k, _)| &k.value == field_name)
                        .map(|(_, v)| self.compile_expression(v))
                        .transpose()?
                        .unwrap_or_else(|| TypedValue::int(self.i64_type.const_int(0, false).into()));
                    field_values.push(val);
                }

                // Allocate the struct on the heap.
                let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
                let struct_size = self.i64_type.const_int((field_names.len() * 8) as u64, false);
                let raw_ptr = self.builder.build_call(malloc_fn, &[struct_size.into()], "actor_alloc")
                    .expect("malloc failed")
                    .try_as_basic_value().left().expect("malloc void")
                    .into_pointer_value();

                // Cast i8* from malloc to i64* for field access.
                let i64_ptr_type = self.i64_type.ptr_type(inkwell::AddressSpace::default());
                let ptr = self.builder.build_pointer_cast(raw_ptr, i64_ptr_type, "actor_struct_ptr")
                    .expect("bitcast failed");

                for (i, val) in field_values.iter().enumerate() {
                    let field_ptr = unsafe {
                        self.builder.build_gep(
                            ptr,
                            &[self.i64_type.const_int(i as u64, false)],
                            &format!("actor_field_{}", i),
                        ).expect("gep failed")
                    };
                    self.builder.build_store(field_ptr, val.value).expect("store failed");
                }

                // state_ptr = i64 handle to the heap-allocated struct.
                let state_ptr = self.builder.build_ptr_to_int(raw_ptr, self.i64_type, "actor_state_ptr")
                    .expect("ptr_to_int failed");

                // Get handler function pointer (convention: fn handle(state, msg) -> int).
                let handler_fn = self.module.get_function("handle")
                    .ok_or("Actor requires a 'handle' function: fn handle(state, msg) -> int")?;
                let handler_ptr = self.builder.build_ptr_to_int(
                    handler_fn.as_global_value().as_pointer_value(),
                    self.i64_type,
                    "handler_ptr",
                ).expect("ptr_to_int failed");

                // Call actor_spawn(handler_ptr, state_ptr) -> handle.
                let actor_spawn_fn = *self.functions.get("actor_spawn").expect("actor_spawn not declared");
                let args_meta: Vec<BasicMetadataValueEnum> = vec![handler_ptr.into(), state_ptr.into()];
                let call_result = self.builder.build_call(actor_spawn_fn, &args_meta, "actor_handle")
                    .map_err(|e| e.to_string())?;
                let handle = call_result.try_as_basic_value()
                    .left()
                    .ok_or("actor_spawn did not return a handle")?;

                Ok(TypedValue::new(handle.into(), AhaType::Int))
            },
            ast::Expression::Match(m) => self.compile_match_expression(m),
            _ => Err(format!("Expression type not yet implemented: {:?}", expression)),
        }
    }

    /// Compile a string literal into an LLVM struct {i8*, i64}
    fn compile_string_literal(&mut self, value: &str) -> Result<TypedValue<'ctx>, String> {
        let str_ptr = self.builder.build_global_string_ptr(value, "str")
            .map_err(|e| e.to_string())?;
        let str_len = self.i64_type.const_int(value.len() as u64, false);
        
        // Build struct {i8*, i64}
        let str_struct = self.string_type.const_zero();
        let str_struct = self.builder.build_insert_value(str_struct, str_ptr.as_pointer_value(), 0, "str_ptr")
            .map_err(|e| e.to_string())?
            .into_struct_value();
        let str_struct = self.builder.build_insert_value(str_struct, str_len, 1, "str_len")
            .map_err(|e| e.to_string())?
            .into_struct_value();
        
        Ok(TypedValue::string(str_struct.into()))
    }

    fn compile_array_literal(&mut self, arr: &ast::ArrayLiteral) -> Result<TypedValue<'ctx>, String> {
        let array_size = arr.elements.len() as u32;
        let array_type = self.i64_type.array_type(array_size);
        let array_ptr = self.builder.build_alloca(array_type, "arr")
            .map_err(|e| e.to_string())?;
        for (i, elem) in arr.elements.iter().enumerate() {
            let value = self.compile_expression(elem)?;
            let idx = self.i64_type.const_int(i as u64, false);
            let zero = self.i64_type.const_int(0, false);
            let elem_ptr = unsafe {
                self.builder.build_gep(array_ptr, &[zero, idx], "elem_ptr")
                    .map_err(|e| e.to_string())?
            };
            self.builder.build_store(elem_ptr, value.value)
                .map_err(|e| e.to_string())?;
        }
        let ptr_as_int = self.builder.build_ptr_to_int(array_ptr, self.i64_type, "arr_ptr")
            .map_err(|e| e.to_string())?;
        Ok(TypedValue::new(ptr_as_int.into(), AhaType::Array(Box::new(AhaType::Int))))
    }

    fn compile_index_expression(&mut self, idx: &ast::IndexExpression) -> Result<TypedValue<'ctx>, String> {
        let array_val = self.compile_expression(&idx.left)?;
        let index_val = self.compile_expression(&idx.index)?;

        // List<T> indexing: delegate to list_get/list_get_string builtin.
        if let AhaType::List(inner) = &array_val.aha_type {
            let list_handle = array_val.value.into_int_value();
            let args_meta: Vec<_> = [
                list_handle.into(),
                index_val.value.into(),
            ].iter().map(|a: &inkwell::values::BasicValueEnum| (*a).into()).collect();
            let builtin = if inner.is_string() { "list_get_string" } else { "list_get" };
            let function = *self.functions.get(builtin).expect("list builtin not declared");
            let call_result = self.builder.build_call(function, &args_meta, "listidx")
                .map_err(|e| e.to_string())?;
            let val = call_result.try_as_basic_value()
                .left()
                .ok_or("list_get returned void")?;
            let tv = if inner.is_string() {
                TypedValue::string(val)
            } else {
                TypedValue::int(val)
            };
            return Ok(tv);
        }

        let array_ptr = self.builder.build_int_to_ptr(
            array_val.value.into_int_value(),
            self.i64_type.ptr_type(inkwell::AddressSpace::default()),
            "arr_ptr_cast"
        ).map_err(|e| e.to_string())?;
        let elem_ptr = unsafe {
            self.builder.build_gep(array_ptr, &[index_val.value.into_int_value()], "elem")
                .map_err(|e| e.to_string())?
        };
        let elem_val = self.builder.build_load(elem_ptr, "elem_val")
            .map_err(|e| e.to_string())?;
        Ok(TypedValue::int(elem_val))
    }

    /// Declare C runtime functions (malloc, strlen, memcpy, strcmp, sprintf, realloc, free)
    fn declare_c_runtime(&mut self) {
        let i8_ptr = self.i8_ptr_type();
        let i64_t = self.i64_type;
        // malloc
        let malloc_ty = i8_ptr.fn_type(&[i64_t.into()], false);
        let malloc_fn = self.module.add_function("malloc", malloc_ty, None);
        self.functions.insert("malloc".to_string(), malloc_fn);
        // realloc
        let realloc_ty = i8_ptr.fn_type(&[i8_ptr.into(), i64_t.into()], false);
        let realloc_fn = self.module.add_function("realloc", realloc_ty, None);
        self.functions.insert("realloc".to_string(), realloc_fn);
        // free — returns void
        let free_ty = self.context.void_type().fn_type(&[i8_ptr.into()], false);
        let free_fn = self.module.add_function("free", free_ty, None);
        self.functions.insert("free".to_string(), free_fn);
        // strlen
        let strlen_ty = i64_t.fn_type(&[i8_ptr.into()], false);
        let strlen_fn = self.module.add_function("strlen", strlen_ty, None);
        self.functions.insert("strlen".to_string(), strlen_fn);
        // memcpy
        let memcpy_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_t.into()], false);
        let memcpy_fn = self.module.add_function("memcpy", memcpy_ty, None);
        self.functions.insert("memcpy".to_string(), memcpy_fn);
        // strcmp
        let strcmp_ty = self.context.i32_type().fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
        let strcmp_fn = self.module.add_function("strcmp", strcmp_ty, None);
        self.functions.insert("strcmp".to_string(), strcmp_fn);
        // memcmp — length-bounded string comparison for Map<String,...> keys.
        // strcmp is unsafe here: concatenated strings aren't NUL-terminated.
        let memcmp_ty = self.context.i32_type().fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_t.into()], false);
        let memcmp_fn = self.module.add_function("memcmp", memcmp_ty, None);
        self.functions.insert("memcmp".to_string(), memcmp_fn);
        // snprintf(buf, size, fmt, ...) — for int_to_string
        let snprintf_ty = self.context.i32_type().fn_type(&[i8_ptr.into(), i64_t.into(), i8_ptr.into()], true);
        let snprintf_fn = self.module.add_function("snprintf", snprintf_ty, None);
        self.functions.insert("snprintf".to_string(), snprintf_fn);
        // strtol(str, NULL, base) — for string_to_int
        let strtol_ty = i64_t.fn_type(&[i8_ptr.into(), i8_ptr.ptr_type(inkwell::AddressSpace::default()).into(), i64_t.into()], false);
        let strtol_fn = self.module.add_function("strtol", strtol_ty, None);
        self.functions.insert("strtol".to_string(), strtol_fn);
        // FILE* fopen(path, mode)
        let i8_ptr_2 = self.i8_ptr_type();
        let fopen_ty = i8_ptr_2.fn_type(&[i8_ptr_2.into(), i8_ptr_2.into()], false);
        let fopen_fn = self.module.add_function("fopen", fopen_ty, None);
        self.functions.insert("fopen".to_string(), fopen_fn);
        // int fclose(FILE*)
        let fclose_ty = self.context.i32_type().fn_type(&[i8_ptr_2.into()], false);
        let fclose_fn = self.module.add_function("fclose", fclose_ty, None);
        self.functions.insert("fclose".to_string(), fclose_fn);
        // size_t fread(buf, size, count, FILE*)
        let fread_ty = i64_t.fn_type(&[i8_ptr_2.into(), i64_t.into(), i64_t.into(), i8_ptr_2.into()], false);
        let fread_fn = self.module.add_function("fread", fread_ty, None);
        self.functions.insert("fread".to_string(), fread_fn);
        // size_t fwrite(buf, size, count, FILE*)
        let fwrite_ty = i64_t.fn_type(&[i8_ptr_2.into(), i64_t.into(), i64_t.into(), i8_ptr_2.into()], false);
        let fwrite_fn = self.module.add_function("fwrite", fwrite_ty, None);
        self.functions.insert("fwrite".to_string(), fwrite_fn);
        // int fseek(FILE*, offset, whence)
        let fseek_ty = self.context.i32_type().fn_type(&[i8_ptr_2.into(), i64_t.into(), self.context.i32_type().into()], false);
        let fseek_fn = self.module.add_function("fseek", fseek_ty, None);
        self.functions.insert("fseek".to_string(), fseek_fn);
        // long ftell(FILE*)
        let ftell_ty = i64_t.fn_type(&[i8_ptr_2.into()], false);
        let ftell_fn = self.module.add_function("ftell", ftell_ty, None);
        self.functions.insert("ftell".to_string(), ftell_fn);
    }

    /// Declare actor runtime functions (actor_spawn, actor_send, actor_call)
    /// for Phase 2 threading. Mapped to actual Rust functions via add_global_mapping in run_jit.
    fn declare_actor_runtime(&mut self) {
        let i64_t = self.i64_type;
        // actor_spawn(fn_ptr: i64, init_state: i64) -> i64
        let spawn_ty = i64_t.fn_type(&[i64_t.into(), i64_t.into()], false);
        let spawn_fn = self.module.add_function("actor_spawn", spawn_ty, None);
        self.functions.insert("actor_spawn".to_string(), spawn_fn);
        // actor_send(handle: i64, msg: i64) -> void
        let send_ty = self.context.void_type().fn_type(&[i64_t.into(), i64_t.into()], false);
        let send_fn = self.module.add_function("actor_send", send_ty, None);
        self.functions.insert("actor_send".to_string(), send_fn);
        // actor_call(handle: i64, msg: i64) -> i64
        let call_ty = i64_t.fn_type(&[i64_t.into(), i64_t.into()], false);
        let call_fn = self.module.add_function("actor_call", call_ty, None);
        self.functions.insert("actor_call".to_string(), call_fn);
    }

    /// Type-checked infix operator compilation
    fn compile_infix(&mut self, infix: &ast::InfixExpression) -> Result<TypedValue<'ctx>, String> {
        let left = self.compile_expression(&infix.left)?;
        let right = self.compile_expression(&infix.right)?;
        let op = infix.operator.as_str();

        // Type check
        let result_type = left.aha_type.check_binary_op(op, &right.aha_type)?;

        match (&left.aha_type, op, &right.aha_type) {
            // Int arithmetic
            (AhaType::Int, "+", AhaType::Int) => {
                let r = self.builder.build_int_add(left.value.into_int_value(), right.value.into_int_value(), "addtmp")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue::int(r.into()))
            },
            (AhaType::Int, "-", AhaType::Int) => {
                let r = self.builder.build_int_sub(left.value.into_int_value(), right.value.into_int_value(), "subtmp")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue::int(r.into()))
            },
            (AhaType::Int, "*", AhaType::Int) => {
                let r = self.builder.build_int_mul(left.value.into_int_value(), right.value.into_int_value(), "multmp")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue::int(r.into()))
            },
            (AhaType::Int, "/", AhaType::Int) => {
                let r = self.builder.build_int_signed_div(left.value.into_int_value(), right.value.into_int_value(), "divtmp")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue::int(r.into()))
            },
            (AhaType::Int, "%", AhaType::Int) => {
                let r = self.builder.build_int_signed_rem(left.value.into_int_value(), right.value.into_int_value(), "modtmp")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue::int(r.into()))
            },
            // Int comparison
            (AhaType::Int, "==" | "!=" | "<" | ">" | "<=" | ">=", AhaType::Int) => {
                let pred = match op {
                    "==" => inkwell::IntPredicate::EQ,
                    "!=" => inkwell::IntPredicate::NE,
                    "<"  => inkwell::IntPredicate::SLT,
                    ">"  => inkwell::IntPredicate::SGT,
                    "<=" => inkwell::IntPredicate::SLE,
                    ">=" => inkwell::IntPredicate::SGE,
                    _ => unreachable!(),
                };
                let cmp = self.builder.build_int_compare(pred, left.value.into_int_value(), right.value.into_int_value(), "cmptmp")
                    .map_err(|e| e.to_string())?;
                let ext = self.builder.build_int_z_extend(cmp, self.i64_type, "cmpext")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue::new(ext.into(), result_type))
            },
            // Bool comparison
            (AhaType::Bool, "==" | "!=", AhaType::Bool) => {
                let pred = if op == "==" { inkwell::IntPredicate::EQ } else { inkwell::IntPredicate::NE };
                let cmp = self.builder.build_int_compare(pred, left.value.into_int_value(), right.value.into_int_value(), "boolcmp")
                    .map_err(|e| e.to_string())?;
                let ext = self.builder.build_int_z_extend(cmp, self.i64_type, "boolext")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue::new(ext.into(), result_type))
            },
            // String concatenation
            (AhaType::String, "+", AhaType::String) => {
                self.compile_string_concat(&left, &right)
            },
            // String comparison
            (AhaType::String, "==" | "!=", AhaType::String) => {
                self.compile_string_compare(&left, &right, op)
            },
            // Logical AND (short-circuit): if left is false, skip right
            (AhaType::Int, "&&", AhaType::Int) | (AhaType::Bool, "&&", AhaType::Bool) => {
                self.compile_logical_and(&left, &right)
            },
            // Logical OR (short-circuit): if left is true, skip right
            (AhaType::Int, "||", AhaType::Int) | (AhaType::Bool, "||", AhaType::Bool) => {
                self.compile_logical_or(&left, &right)
            },
            _ => Err(format!("Cannot apply '{}' to {} and {}", op, left.aha_type, right.aha_type)),
        }
    }

    /// Extract i8* pointer from a string struct value
    fn extract_str_ptr(&mut self, str_val: &TypedValue<'ctx>) -> Result<inkwell::values::PointerValue<'ctx>, String> {
        self.builder.build_extract_value(str_val.value.into_struct_value(), 0, "sptr")
            .map_err(|e| e.to_string())
            .map(|v| v.into_pointer_value())
    }

    /// Extract i64 length from a string struct value
    fn extract_str_len(&mut self, str_val: &TypedValue<'ctx>) -> Result<inkwell::values::IntValue<'ctx>, String> {
        self.builder.build_extract_value(str_val.value.into_struct_value(), 1, "slen")
            .map_err(|e| e.to_string())
            .map(|v| v.into_int_value())
    }

    /// Compile string concatenation: allocate new buffer, memcpy both, build struct
    fn compile_string_concat(&mut self, left: &TypedValue<'ctx>, right: &TypedValue<'ctx>) -> Result<TypedValue<'ctx>, String> {
        let l_ptr = self.extract_str_ptr(left)?;
        let l_len = self.extract_str_len(left)?;
        let r_ptr = self.extract_str_ptr(right)?;
        let r_len = self.extract_str_len(right)?;

        // total_len = l_len + r_len
        let total_len = self.builder.build_int_add(l_len, r_len, "total_len").map_err(|e| e.to_string())?;
        // alloc_size = total_len + 1 (null terminator)
        let one = self.i64_type.const_int(1, false);
        let alloc_size = self.builder.build_int_add(total_len, one, "alloc_sz").map_err(|e| e.to_string())?;

        let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
        let new_buf = self.builder.build_call(malloc_fn, &[alloc_size.into()], "newbuf")
            .map_err(|e| e.to_string())?
            .try_as_basic_value().left().ok_or("malloc returned void")?
            .into_pointer_value();

        let memcpy_fn = *self.functions.get("memcpy").expect("memcpy not declared");
        // memcpy(new_buf, l_ptr, l_len)
        self.builder.build_call(memcpy_fn, &[new_buf.into(), l_ptr.into(), l_len.into()], "cp1").map_err(|e| e.to_string())?;
        // dest2 = new_buf + l_len
        let dest2 = unsafe { self.builder.build_gep(new_buf, &[l_len], "dest2").map_err(|e| e.to_string())? };
        // memcpy(dest2, r_ptr, r_len)
        self.builder.build_call(memcpy_fn, &[dest2.into(), r_ptr.into(), r_len.into()], "cp2").map_err(|e| e.to_string())?;
        // null terminate
        let null_pos = unsafe { self.builder.build_gep(new_buf, &[total_len], "nullpos").map_err(|e| e.to_string())? };
        self.builder.build_store(null_pos, self.context.i8_type().const_int(0, false)).map_err(|e| e.to_string())?;

        // Build result struct {i8*, i64}
        let s = self.string_type.const_zero();
        let s = self.builder.build_insert_value(s, new_buf, 0, "rptr").map_err(|e| e.to_string())?.into_struct_value();
        let s = self.builder.build_insert_value(s, total_len, 1, "rlen").map_err(|e| e.to_string())?.into_struct_value();
        Ok(TypedValue::string(s.into()))
    }

    /// Compile string comparison using strcmp
    fn compile_string_compare(&mut self, left: &TypedValue<'ctx>, right: &TypedValue<'ctx>, op: &str) -> Result<TypedValue<'ctx>, String> {
        let l_ptr = self.extract_str_ptr(left)?;
        let r_ptr = self.extract_str_ptr(right)?;
        let strcmp_fn = *self.functions.get("strcmp").expect("strcmp not declared");
        let cmp_result = self.builder.build_call(strcmp_fn, &[l_ptr.into(), r_ptr.into()], "strcmp_r")
            .map_err(|e| e.to_string())?
            .try_as_basic_value().left().ok_or("strcmp returned void")?
            .into_int_value();
        let zero_i32 = self.context.i32_type().const_int(0, false);
        let pred = if op == "==" { inkwell::IntPredicate::EQ } else { inkwell::IntPredicate::NE };
        let cmp = self.builder.build_int_compare(pred, cmp_result, zero_i32, "streq").map_err(|e| e.to_string())?;
        let ext = self.builder.build_int_z_extend(cmp, self.i64_type, "streqext").map_err(|e| e.to_string())?;
        Ok(TypedValue::new(ext.into(), AhaType::Bool))
    }

    /// Logical AND: both operands are already evaluated by compile_infix.
    /// Result = (left != 0) && (right != 0)
    fn compile_logical_and(&mut self, left: &TypedValue<'ctx>, right: &TypedValue<'ctx>) -> Result<TypedValue<'ctx>, String> {
        let zero = self.i64_type.const_int(0, false);
        let left_bool = self.builder.build_int_compare(
            inkwell::IntPredicate::NE, left.value.into_int_value(), zero, "and_left_bool"
        ).map_err(|e| e.to_string())?;
        let right_bool = self.builder.build_int_compare(
            inkwell::IntPredicate::NE, right.value.into_int_value(), zero, "and_right_bool"
        ).map_err(|e| e.to_string())?;
        let result = self.builder.build_and(left_bool, right_bool, "and_tmp")
            .map_err(|e| e.to_string())?;
        let ext = self.builder.build_int_z_extend(result, self.i64_type, "and_ext")
            .map_err(|e| e.to_string())?;
        Ok(TypedValue::int(ext.into()))
    }

    /// Logical OR: both operands are already evaluated by compile_infix.
    /// Result = (left != 0) || (right != 0)
    fn compile_logical_or(&mut self, left: &TypedValue<'ctx>, right: &TypedValue<'ctx>) -> Result<TypedValue<'ctx>, String> {
        let zero = self.i64_type.const_int(0, false);
        let left_bool = self.builder.build_int_compare(
            inkwell::IntPredicate::NE, left.value.into_int_value(), zero, "or_left_bool"
        ).map_err(|e| e.to_string())?;
        let right_bool = self.builder.build_int_compare(
            inkwell::IntPredicate::NE, right.value.into_int_value(), zero, "or_right_bool"
        ).map_err(|e| e.to_string())?;
        let result = self.builder.build_or(left_bool, right_bool, "or_tmp")
            .map_err(|e| e.to_string())?;
        let ext = self.builder.build_int_z_extend(result, self.i64_type, "or_ext")
            .map_err(|e| e.to_string())?;
        Ok(TypedValue::int(ext.into()))
    }

    /// Infer parameter types by scanning call expressions in the program.
    /// If a call passes a string arg, that param is String. Otherwise Int.
    fn infer_param_types(&mut self, func_name: &str, params: &[ast::Identifier], hints: &[Option<String>]) -> Vec<AhaType> {
        let mut types = vec![AhaType::Int; params.len()];
        // Use type hints first (e.g. `d: Day` → Enum("Day"))
        for (i, hint) in hints.iter().enumerate() {
            if i < types.len() {
                if let Some(h) = hint {
                    types[i] = if self.enum_defs.contains_key(h.as_str()) {
                        AhaType::Enum(h.clone())
                    } else if self.struct_defs.contains_key(h.as_str()) {
                        AhaType::Struct(h.clone())
                    } else {
                        AhaType::from_hint(h).unwrap_or(AhaType::Int)
                    };
                }
            }
        }
        // Override with call-site inference when available
        if let Some(inferred) = self.param_type_map.get(func_name) {
            for (i, t) in inferred.iter().enumerate() {
                if i < types.len() {
                    types[i] = t.clone();
                }
            }
        }
        types
    }

    // Compile function definition — FIX C-05 (double return) and C-06 (variable restore safety)
    // Generic functions are compiled lazily via monomorphization; skip body compilation here.
    fn compile_function(&mut self, func: &ast::FunctionLiteral) -> Result<TypedValue<'ctx>, String> {
        let func_name = func.name.as_ref()
            .map(|id| id.value.clone())
            .unwrap_or_else(|| format!("anonymous_{}", self.functions.len()));
        Self::diag_mark(&format!("COMPILE_FN: func_name='{}', current_function={:?}, builder_block={:?}",
            func_name,
            self.current_function.map(|f| f.get_name().to_str().unwrap_or("?").to_string()),
            self.builder.get_insert_block().map(|b| b.get_name().to_str().unwrap_or("?").to_string())));
        if !func.type_params.is_empty() {
            return Ok(TypedValue::void(self.i64_type.const_int(0, false).into()));
        }

        // Infer param types from call sites: scan all call expressions
        // in already-compiled code for this function name
        let param_aha_types = self.infer_param_types(&func_name, &func.parameters, &func.param_type_hints);

        // Determine return type — reuse the pre-declared type if present
        // (set by predeclare_functions), otherwise infer it now.
        let return_type = self.fn_types.get(&func_name)
            .cloned()
            .unwrap_or_else(|| self.infer_function_return_type(func, &func_name));

        // If a return type annotation is present, validate it against the
        // body's inferred return type to catch mismatches early.
        if let Some(ref hint) = func.return_type_hint {
            let param_aha_types_imm = self.infer_param_types_immutable(&func_name, &func.parameters, &func.param_type_hints);
            let scope: HashMap<String, AhaType> = func.parameters.iter().enumerate()
                .map(|(i, p)| (p.value.clone(), param_aha_types_imm.get(i).cloned().unwrap_or(AhaType::Int)))
                .collect();
            let mut body_type = AhaType::Int;
            for stmt in &func.body.statements {
                if let ast::Statement::Return(ret) = stmt {
                    body_type = self.infer_expr_type_with_scope(&ret.return_value, &scope);
                    break;
                }
            }
            if body_type == AhaType::Int {
                for stmt in func.body.statements.iter().rev() {
                    if let ast::Statement::Expression(expr_stmt) = stmt {
                        body_type = self.infer_expr_type_with_scope(&expr_stmt.expression, &scope);
                        break;
                    }
                }
            }
            let hint_type = if self.struct_defs.contains_key(hint.as_str()) {
                AhaType::Struct(hint.clone())
            } else if self.enum_defs.contains_key(hint.as_str()) {
                AhaType::Enum(hint.clone())
            } else {
                AhaType::from_hint(hint).unwrap_or(AhaType::Int)
            };
            let compatible = match (&hint_type, &body_type) {
                (AhaType::Struct(a), AhaType::Struct(b)) => a == b,
                (AhaType::Enum(a), AhaType::Enum(b)) => a == b,
                (AhaType::Int, t) if t.is_bool() => true, // Int and Bool are both i64
                (t, AhaType::Int) if t.is_bool() => true,
                // Body inferred as Int but hint is complex — trust the hint
                // (infer_expr_type_with_scope can't track local variable types
                // for Map/List; String literals ARE inferred correctly).
                (AhaType::Map(_, _), AhaType::Int) => true,
                (AhaType::List(_), AhaType::Int) => true,
                _ => hint_type == body_type,
            };
            if !compatible {
                return Err(format!(
                    "Return type annotation '{}' does not match actual return type '{}' in function '{}'",
                    hint, body_type, func_name
                ));
            }
        }

        // Reuse pre-declared function if it exists (for forward references)
        let function = if let Some(f) = self.functions.get(&func_name) {
            *f
        } else {
            let param_types: Result<Vec<_>, _> = param_aha_types.iter()
                .map(|t| self.aha_type_to_llvm_type(t))
                .collect();
            let param_types = param_types?;
            let ret_llvm = self.build_fn_type(&return_type, &param_types)?;
            let function = self.module.add_function(&func_name, ret_llvm, None);
            self.functions.insert(func_name.clone(), function);
            function
        };
        self.fn_types.insert(func_name.clone(), return_type.clone());

        let saved_block = self.builder.get_insert_block();
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
        let saved_function = self.current_function;

        let entry_block = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry_block);
        self.current_function = Some(function);
        Self::diag_mark(&format!("COMPILE_FN_ENTRY: func='{}', entry='{}', fn_blocks={}",
            func_name,
            entry_block.get_name().to_str().unwrap_or("?"),
            function.get_basic_blocks().len()));

        let result = (|| -> Result<(), String> {
            for (i, param) in func.parameters.iter().enumerate() {
                let param_value = function.get_nth_param(i as u32)
                    .ok_or("Failed to get parameter")?;
                let aha_type = &param_aha_types[i];
                let alloc_type = self.aha_type_to_llvm_type(aha_type)?;
                let alloca = self.builder.build_alloca(alloc_type, &param.value)
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(alloca, param_value)
                    .map_err(|e| e.to_string())?;
                self.insert_variable(param.value.clone(), alloca, aha_type.clone());
                self.mark_param(&param.value);
            }

            // Escape analysis: find variables returned from this function.
            // Escaped variables must NEVER be freed by last-use or cleanup.
            let mut escaped = std::collections::HashSet::new();
            for stmt in func.body.statements.iter() {
                if let ast::Statement::Return(ret) = stmt {
                    escaped = Self::find_heap_vars_in_expr(&ret.return_value);
                    break;
                }
            }
            // Also check implicit return (last expression = return value).
            if escaped.is_empty() {
                if let Some(ast::Statement::Expression(es)) = func.body.statements.last() {
                    escaped = Self::find_heap_vars_in_expr(&es.expression);
                }
            }

            // Pre-scan: find last-use points for each heap variable.
            // Remove escaped variables — they must not be freed by last-use.
            let mut last_uses = Self::find_last_uses(&func.body.statements);
            for var in &escaped {
                last_uses.remove(var);
            }

            let mut has_return = false;
            let mut last_value: BasicValueEnum<'ctx> = match &return_type {
                AhaType::String => self.string_type.const_zero().into(),
                AhaType::Struct(name) => {
                    self.struct_llvm_type(name)?.const_zero().into()
                }
                AhaType::Enum(name) => {
                    self.enum_llvm_type(name)?.const_zero().into()
                }
                _ => self.i64_type.const_int(0, false).into(),
            };

            for (stmt_idx, stmt) in func.body.statements.iter().enumerate() {
                if let ast::Statement::Return(_) = stmt {
                    self.compile_statement(stmt)?;
                    has_return = true;
                    break;
                } else if let ast::Statement::Expression(expr_stmt) = stmt {
                    let tv = self.compile_expression(&expr_stmt.expression)?;
                    last_value = tv.value;
                } else {
                    self.compile_statement(stmt)?;
                }
                // Phase 2: insert free calls at last-use points
                for (var_name, &last_idx) in &last_uses {
                    if last_idx == stmt_idx {
                        self.insert_free_for_var(var_name);
                    }
                }
            }

            if !has_return && self.has_heap_locals() {
                self.insert_cleanup_inline(&escaped);
                self.builder.build_return(Some(&last_value))
                    .map_err(|e| e.to_string())?;
            } else if !has_return {
                self.builder.build_return(Some(&last_value))
                    .map_err(|e| e.to_string())?;
            } else {
                // has_return = true: the return statement's build_return
                // went into merge_block (after match). The entry block
                // is still unterminated. Add build_return in entry block
                // so LLVM has a valid entry point. This is dead code in
                // practice (switch dispatches to arm blocks first).
                let entry_has_term = entry_block.get_terminator().is_some();
                Self::diag_mark(&format!("FIX: has_return=true, entry_has_term={}, builder_block={:?}",
                    entry_has_term,
                    self.builder.get_insert_block().map(|b| b.get_name().to_str().unwrap_or("?").to_string())));
                if !entry_has_term {
                    let return_val: inkwell::values::BasicValueEnum = self.i64_type.const_int(0, false).into();
                    self.builder.position_at_end(entry_block);
                    let r = self.builder.build_return(Some(&return_val));
                    Self::diag_mark(&format!("FIX: build_return result={:?}", r.is_ok()));
                    r.map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        })();
        
        self.scopes = saved_scopes;
        self.current_function = saved_function;
        if let Some(block) = saved_block {
            self.builder.position_at_end(block);
        } else {
            // No previous block to restore to. Position the builder at the
            // entry block of the function we just compiled so the caller
            // (compile()) can add the implicit main's terminator there.
            self.builder.position_at_end(entry_block);
        }
        result?;
        Ok(TypedValue::void(self.i64_type.const_int(0, false).into()))
    }

    fn compile_call(&mut self, call: &ast::CallExpression) -> Result<TypedValue<'ctx>, String> {
        let func_name = match call.function.as_ref() {
            ast::Expression::Identifier(id) => id.value.clone(),
            ast::Expression::ModuleAccess(ma) => ma.name.clone(),
            _ => return Err(format!("Can only call named functions, got: {:?}", call.function)),
        };
        // Generic function call → monomorphize (lazy per call-site type).
        if self.generic_defs.contains_key(&func_name) {
            return self.compile_generic_call(&func_name, call);
        }
        // List builtins: element type (Int vs String) is known only at the
        // call site, so dispatch here before the generic argument loop.
        if func_name.starts_with("list_") {
            return self.compile_list_call(&func_name, call);
        }
        // Map builtins: key/value element types are known only at the call
        // site, so dispatch here like list builtins.
        if func_name.starts_with("map_") {
            return self.compile_map_call(&func_name, call);
        }
        // F6 Actor builtins: send(handle, msg), call(handle, msg) -> i64
        if func_name == "send" || func_name == "call" {
            return self.compile_actor_call(&func_name, call);
        }
        // Enum variant constructor: Variant(args...) or Variant
        if let Some(enum_name) = self.find_enum_for_variant(&func_name) {
            return self.compile_enum_constructor(&enum_name, &func_name, call);
        }
        let mut args: Vec<BasicValueEnum> = Vec::new();
        for arg in &call.arguments {
            args.push(self.compile_expression(arg)?.value);
        }
        let args_meta: Vec<_> = args.iter().map(|a| (*a).into()).collect();

        let function = if let Some(f) = self.functions.get(&func_name) {
            *f
        } else if let Some(f) = self.module.get_function(&func_name) {
            f
        } else {
            return Err(format!("Unknown function: {}", func_name));
        };
        let call_result = self.builder.build_call(function, &args_meta, "calltmp")
            .map_err(|e| e.to_string())?;
        let ret_type = self.fn_types.get(&func_name).cloned().unwrap_or(AhaType::Int);
        let val = call_result.try_as_basic_value()
            .left()
            .ok_or_else(|| "Function call did not return a value".to_string())?;
        Ok(TypedValue::new(val, ret_type))
    }

    /// Compile actor_send / actor_call builtin calls via the threaded runtime.
    /// call(a, msg) -> actor_call(handle, msg) -> blocking request-response.
    /// send(a, msg) -> actor_send(handle, msg) -> fire-and-forget.
    fn compile_actor_call(&mut self, func_name: &str, call: &ast::CallExpression) -> Result<TypedValue<'ctx>, String> {
        if call.arguments.len() != 2 {
            return Err(format!("{} expects 2 arguments (handle, msg)", func_name));
        }
        let handle = self.compile_expression(&call.arguments[0])?.value;
        let msg = self.compile_expression(&call.arguments[1])?.value;

        let runtime_fn_name = if func_name == "send" { "actor_send" } else { "actor_call" };
        let function = *self.functions.get(runtime_fn_name)
            .ok_or_else(|| format!("{} not declared", runtime_fn_name))?;

        let args_meta: Vec<BasicMetadataValueEnum> = vec![handle.into(), msg.into()];
        let call_result = self.builder.build_call(function, &args_meta, "actor_tmp")
            .map_err(|e| e.to_string())?;

        if func_name == "call" {
            let val = call_result.try_as_basic_value()
                .left()
                .ok_or_else(|| "actor_call did not return a value".to_string())?;
            Ok(TypedValue::int(val))
        } else {
            Ok(TypedValue::void(self.i64_type.const_int(0, false).into()))
        }
    }

    /// Compile a list_* builtin call. The LLVM-level dispatch depends on
    /// the list's element type (Int lists store i64 elements; String lists
    /// store {i8*, i64} elements), which is only known at the call site.
    fn compile_list_call(&mut self, func_name: &str, call: &ast::CallExpression) -> Result<TypedValue<'ctx>, String> {
        // list_new / list_new_string take no list arg — infer from the name.
        if func_name == "list_new" || func_name == "list_new_string" {
            return self.compile_call_generic_args(func_name, call);
        }

        // All other list builtins take the list as the first argument.
        let list_tv = self.compile_expression(&call.arguments[0])?;
        let elem_type = match &list_tv.aha_type {
            AhaType::List(inner) => (**inner).clone(),
            other => return Err(format!(
                "list builtin '{}' expects a List as first argument, got {}",
                func_name, other
            )),
        };
        let list_handle = list_tv.value.into_int_value();

        match func_name {
            "list_len" | "list_free" => {
                if func_name == "list_free" {
                    if let ast::Expression::Identifier(id) = &call.arguments[0] {
                        self.mark_freed(&id.value);
                    }
                }
                let args_meta: Vec<_> = [list_handle.into()].iter().map(|a: &inkwell::values::BasicValueEnum| (*a).into()).collect();
                let function = *self.functions.get(func_name).expect("list builtin not declared");
                let call_result = self.builder.build_call(function, &args_meta, "calltmp")
                    .map_err(|e| e.to_string())?;
                let val = call_result.try_as_basic_value()
                    .left()
                    .ok_or("list builtin returned void")?;
                let ret_type = self.fn_types.get(func_name).cloned().unwrap_or(AhaType::Int);
                Ok(TypedValue::new(val, ret_type))
            }
            "list_push" => {
                // Compile the value argument. For String lists, split the
                // string struct and call list_push_string(list, ptr, len).
                let value_tv = self.compile_expression(&call.arguments[1])?;
                if elem_type.is_string() {
                    if !value_tv.aha_type.is_string() {
                        return Err(format!(
                            "list_push on List<String> requires a string value, got {}",
                            value_tv.aha_type
                        ));
                    }
                    let s_ptr = self.extract_str_ptr(&value_tv)?;
                    let s_len = self.extract_str_len(&value_tv)?;
                    let args_meta: Vec<_> = [
                        list_handle.into(),
                        s_ptr.into(),
                        s_len.into(),
                    ].iter().map(|a: &inkwell::values::BasicValueEnum| (*a).into()).collect();
                    let function = *self.functions.get("list_push_string").expect("list_push_string not declared");
                    self.builder.build_call(function, &args_meta, "calltmp")
                        .map_err(|e| e.to_string())?;
                    Ok(list_tv)
                } else {
                    if value_tv.aha_type.is_string() {
                        return Err(format!(
                            "list_push on List<Int> requires an int value, got string"
                        ));
                    }
                    let args_meta: Vec<_> = [
                        list_handle.into(),
                        value_tv.value.into(),
                    ].iter().map(|a: &inkwell::values::BasicValueEnum| (*a).into()).collect();
                    let function = *self.functions.get("list_push").expect("list_push not declared");
                    self.builder.build_call(function, &args_meta, "calltmp")
                        .map_err(|e| e.to_string())?;
                    Ok(list_tv)
                }
            }
            "list_get" => {
                let index_tv = self.compile_expression(&call.arguments[1])?;
                if elem_type.is_string() {
                    let args_meta: Vec<_> = [
                        list_handle.into(),
                        index_tv.value.into(),
                    ].iter().map(|a: &inkwell::values::BasicValueEnum| (*a).into()).collect();
                    let function = *self.functions.get("list_get_string").expect("list_get_string not declared");
                    let call_result = self.builder.build_call(function, &args_meta, "calltmp")
                        .map_err(|e| e.to_string())?;
                    let val = call_result.try_as_basic_value()
                        .left()
                        .ok_or("list_get_string returned void")?;
                    Ok(TypedValue::string(val))
                } else {
                    let args_meta: Vec<_> = [
                        list_handle.into(),
                        index_tv.value.into(),
                    ].iter().map(|a: &inkwell::values::BasicValueEnum| (*a).into()).collect();
                    let function = *self.functions.get("list_get").expect("list_get not declared");
                    let call_result = self.builder.build_call(function, &args_meta, "calltmp")
                        .map_err(|e| e.to_string())?;
                    let val = call_result.try_as_basic_value()
                        .left()
                        .ok_or("list_get returned void")?;
                    Ok(TypedValue::int(val))
                }
            }
            _ => Err(format!("Unknown list builtin: {}", func_name)),
        }
    }

    /// Compile a map_* builtin call.  LLVM-level dispatch depends on
    /// key/value element types (Int=i64, String={i8*,i64}), known only
    /// at the call site.
    fn compile_map_call(&mut self, func_name: &str, call: &ast::CallExpression) -> Result<TypedValue<'ctx>, String> {
        // map_new variants take no map arg — infer from name.
        if func_name == "map_new"
            || func_name == "map_string_key_new"
            || func_name == "map_string_val_new"
            || func_name == "map_strings_new"
        {
            return self.compile_call_generic_args(func_name, call);
        }

        // All other map builtins take the map as first argument.
        let map_tv = self.compile_expression(&call.arguments[0])?;
        let (key_type, val_type) = match &map_tv.aha_type {
            AhaType::Map(k, v) => ((**k).clone(), (**v).clone()),
            other => return Err(format!(
                "map builtin '{}' expects a Map as first argument, got {}",
                func_name, other
            )),
        };
        let map_handle = map_tv.value.into_int_value();

        match func_name {
            "map_len" | "map_free" | "map_string_key_len" | "map_string_key_free"
            | "map_string_val_len" | "map_string_val_free"
            | "map_strings_len" | "map_strings_free" => {
                if func_name.contains("free") {
                    if let ast::Expression::Identifier(id) = &call.arguments[0] {
                        self.mark_freed(&id.value);
                    }
                }
                let args_meta: Vec<_> = [map_handle.into()]
                    .iter().map(|a: &inkwell::values::BasicValueEnum| (*a).into()).collect();
                let function = *self.functions.get(func_name).expect("map builtin not declared");
                let call_result = self.builder.build_call(function, &args_meta, "calltmp")
                    .map_err(|e| e.to_string())?;
                let val = call_result.try_as_basic_value()
                    .left().ok_or("map builtin returned void")?;
                let ret_type = self.fn_types.get(func_name).cloned().unwrap_or(AhaType::Int);
                Ok(TypedValue::new(val, ret_type))
            }

            "map_set" | "map_string_key_set" | "map_string_val_set" | "map_strings_set" => {
                let key_tv = self.compile_expression(&call.arguments[1])?;
                let val_tv = self.compile_expression(&call.arguments[2])?;
                let mut args: Vec<BasicValueEnum> = vec![map_handle.into()];
                // Key arg(s)
                if key_type.is_string() {
                    args.push(self.extract_str_ptr(&key_tv)? .into());
                    args.push(self.extract_str_len(&key_tv)? .into());
                } else {
                    args.push(key_tv.value);
                }
                // Val arg(s)
                if val_type.is_string() {
                    args.push(self.extract_str_ptr(&val_tv)? .into());
                    args.push(self.extract_str_len(&val_tv)? .into());
                } else {
                    args.push(val_tv.value);
                }
                let args_meta: Vec<_> = args.iter().map(|a: &BasicValueEnum| (*a).into()).collect();
                let function = *self.functions.get(func_name).expect("map_set not declared");
                let call_result = self.builder.build_call(function, &args_meta, "calltmp")
                    .map_err(|e| e.to_string())?;
                let val = call_result.try_as_basic_value()
                    .left().ok_or("map_set returned void")?;
                Ok(TypedValue::new(val, AhaType::Map(Box::new(key_type), Box::new(val_type))))
            }

            "map_get" | "map_string_key_get" | "map_string_val_get" | "map_strings_get" => {
                let key_tv = self.compile_expression(&call.arguments[1])?;
                let mut args: Vec<BasicValueEnum> = vec![map_handle.into()];
                if key_type.is_string() {
                    args.push(self.extract_str_ptr(&key_tv)? .into());
                    args.push(self.extract_str_len(&key_tv)? .into());
                } else {
                    args.push(key_tv.value);
                }
                let args_meta: Vec<_> = args.iter().map(|a: &BasicValueEnum| (*a).into()).collect();
                let function = *self.functions.get(func_name).expect("map_get not declared");
                let call_result = self.builder.build_call(function, &args_meta, "calltmp")
                    .map_err(|e| e.to_string())?;
                let val = call_result.try_as_basic_value()
                    .left().ok_or("map_get returned void")?;
                let ret_type = if val_type.is_string() { AhaType::String } else { AhaType::Int };
                Ok(TypedValue::new(val, ret_type))
            }

            "map_contains" | "map_string_key_contains" | "map_string_val_contains" | "map_strings_contains" => {
                let key_tv = self.compile_expression(&call.arguments[1])?;
                let mut args: Vec<BasicValueEnum> = vec![map_handle.into()];
                if key_type.is_string() {
                    args.push(self.extract_str_ptr(&key_tv)? .into());
                    args.push(self.extract_str_len(&key_tv)? .into());
                } else {
                    args.push(key_tv.value);
                }
                let args_meta: Vec<_> = args.iter().map(|a: &BasicValueEnum| (*a).into()).collect();
                let function = *self.functions.get(func_name).expect("map_contains not declared");
                let call_result = self.builder.build_call(function, &args_meta, "calltmp")
                    .map_err(|e| e.to_string())?;
                let val = call_result.try_as_basic_value()
                    .left().ok_or("map_contains returned void")?;
                Ok(TypedValue::int(val))
            }

            "map_remove" | "map_string_key_remove" | "map_string_val_remove" | "map_strings_remove" => {
                let key_tv = self.compile_expression(&call.arguments[1])?;
                let mut args: Vec<BasicValueEnum> = vec![map_handle.into()];
                if key_type.is_string() {
                    args.push(self.extract_str_ptr(&key_tv)? .into());
                    args.push(self.extract_str_len(&key_tv)? .into());
                } else {
                    args.push(key_tv.value);
                }
                let args_meta: Vec<_> = args.iter().map(|a: &BasicValueEnum| (*a).into()).collect();
                let function = *self.functions.get(func_name).expect("map_remove not declared");
                let call_result = self.builder.build_call(function, &args_meta, "calltmp")
                    .map_err(|e| e.to_string())?;
                let val = call_result.try_as_basic_value()
                    .left().ok_or("map_remove returned void")?;
                Ok(TypedValue::new(val, AhaType::Map(Box::new(key_type), Box::new(val_type))))
            }

            _ => Err(format!("Unknown map builtin: {}", func_name)),
        }
    }

    /// Compile a call that takes no list argument (list_new, list_new_string)
    /// — falls through to the generic argument loop.
    fn compile_call_generic_args(&mut self, func_name: &str, call: &ast::CallExpression) -> Result<TypedValue<'ctx>, String> {
        let mut args: Vec<BasicValueEnum> = Vec::new();
        for arg in &call.arguments {
            args.push(self.compile_expression(arg)?.value);
        }
        let args_meta: Vec<_> = args.iter().map(|a| (*a).into()).collect();
        let function = if let Some(f) = self.functions.get(func_name) {
            *f
        } else if let Some(f) = self.module.get_function(func_name) {
            f
        } else {
            return Err(format!("Unknown function: {}", func_name));
        };
        let call_result = self.builder.build_call(function, &args_meta, "calltmp")
            .map_err(|e| e.to_string())?;
        let ret_type = self.fn_types.get(func_name).cloned().unwrap_or(AhaType::Int);
        let val = call_result.try_as_basic_value()
            .left()
            .ok_or_else(|| "Function call did not return a value".to_string())?;
        Ok(TypedValue::new(val, ret_type))
    }
    /// Infer a generic function's return type with concrete type
    /// parameters bound. Param types come from the resolved hints,
    /// so `fn id<T>(x: T) -> T { x }` returns the concrete arg type.
    fn infer_generic_return_type(&self, func: &ast::FunctionLiteral, param_types: &[AhaType]) -> AhaType {
        let scope: HashMap<String, AhaType> = func.parameters.iter().enumerate()
            .map(|(i, p)| (p.value.clone(), param_types.get(i).cloned().unwrap_or(AhaType::Int)))
            .collect();
        for stmt in &func.body.statements {
            if let ast::Statement::Return(ret) = stmt {
                return self.infer_expr_type_with_scope(&ret.return_value, &scope);
            }
        }
        for stmt in func.body.statements.iter().rev() {
            if let ast::Statement::Expression(expr_stmt) = stmt {
                return self.infer_expr_type_with_scope(&expr_stmt.expression, &scope);
            }
        }
        AhaType::Int
    }

    /// Monomorphize and call a generic function.
    /// Each unique (generic name, concrete type params) combination gets
    /// its own LLVM function (`max_Int`, `max_String`, ...), compiled
    /// lazily at the first call site and cached in `functions`.
    fn compile_generic_call(&mut self, func_name: &str, call: &ast::CallExpression) -> Result<TypedValue<'ctx>, String> {
        let generic = self.generic_defs.get(func_name).cloned()
            .ok_or_else(|| format!("Unknown generic function: {}", func_name))?;

        // Compile the arguments first (still in the caller's block).
        let mut args: Vec<BasicValueEnum> = Vec::new();
        let mut arg_types: Vec<AhaType> = Vec::new();
        for arg in &call.arguments {
            let tv = self.compile_expression(arg)?;
            arg_types.push(tv.aha_type.clone());
            args.push(tv.value);
        }

        // Bind generic type params (T, U, ...) from param hints to the
        // concrete argument types at matching positions.
        // Handles both direct hints (T) and container hints (List<T>).
        let mut type_params: HashMap<String, AhaType> = HashMap::new();
        for (i, hint) in generic.param_type_hints.iter().enumerate() {
            if let Some(h) = hint {
                if generic.type_params.contains(h) && !type_params.contains_key(h) {
                    let t = arg_types.get(i).cloned().unwrap_or(AhaType::Int);
                    type_params.insert(h.clone(), t);
                } else if let Some(inner) = h.strip_prefix("List<").and_then(|s| s.strip_suffix('>')) {
                    if generic.type_params.iter().any(|tp| tp == inner) && !type_params.contains_key(inner) {
                        if let Some(AhaType::List(inner_type)) = arg_types.get(i) {
                            type_params.insert(inner.to_string(), *inner_type.clone());
                        }
                    }
                }
            }
        }

        // Mangled name: deterministic order by type-param name.
        let mut keyed: Vec<(String, AhaType)> = type_params.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        let suffix: String = keyed.iter().map(|(_, t)| format!("_{}", t)).collect();
        let mangled = format!("{}{}", func_name, suffix);

        // Cache hit — just call the already-instantiated function.
        if let Some(f) = self.functions.get(&mangled) {
            let args_meta: Vec<_> = args.iter().map(|a| (*a).into()).collect();
            let call_result = self.builder.build_call(*f, &args_meta, "calltmp")
                .map_err(|e| e.to_string())?;
            let ret_type = self.fn_types.get(&mangled).cloned().unwrap_or(AhaType::Int);
            let val = call_result.try_as_basic_value()
                .left()
                .ok_or_else(|| "Generic call did not return a value".to_string())?;
            return Ok(TypedValue::new(val, ret_type));
        }

        // Activate type-param bindings for hint resolution & body compile.
        let saved_tpm = std::mem::replace(&mut self.type_param_map, type_params);

        let mut param_aha_types: Vec<AhaType> = Vec::new();
        for (i, _p) in generic.parameters.iter().enumerate() {
            let t = match generic.param_type_hints.get(i) {
                Some(Some(h)) => self.resolve_hint_type(h.as_str()),
                Some(None) | None => arg_types.get(i).cloned().unwrap_or(AhaType::Int),
            };
            param_aha_types.push(t);
        }

        let return_type = match &generic.return_type_hint {
            Some(h) => self.resolve_hint_type(h.as_str()),
            None => self.infer_generic_return_type(&generic, &param_aha_types),
        };

        let param_types: Result<Vec<_>, _> = param_aha_types.iter()
            .map(|t| self.aha_type_to_llvm_type(t))
            .collect();
        let param_types = param_types?;
        let fn_type = self.build_fn_type(&return_type, &param_types)?;
        let function = self.module.add_function(&mangled, fn_type, None);
        self.functions.insert(mangled.clone(), function);
        self.fn_types.insert(mangled.clone(), return_type.clone());

        // Compile the body with the concrete type params bound.
        let saved_block = self.builder.get_insert_block();
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
        let saved_function = self.current_function;

        let entry_block = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry_block);
        self.current_function = Some(function);
        Self::diag_mark(&format!("COMPILE_FN_ENTRY: func='{}', entry='{}', fn_blocks={}",
            func_name,
            entry_block.get_name().to_str().unwrap_or("?"),
            function.get_basic_blocks().len()));

        let result = (|| -> Result<(), String> {
            for (i, param) in generic.parameters.iter().enumerate() {
                let param_value = function.get_nth_param(i as u32)
                    .ok_or("Failed to get parameter")?;
                let aha_type = &param_aha_types[i];
                let alloc_type = self.aha_type_to_llvm_type(aha_type)?;
                let alloca = self.builder.build_alloca(alloc_type, &param.value)
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(alloca, param_value)
                    .map_err(|e| e.to_string())?;
                self.insert_variable(param.value.clone(), alloca, aha_type.clone());
                self.mark_param(&param.value);
            }

            // Escape analysis: find variables returned from this function.
            let mut escaped = std::collections::HashSet::new();
            for stmt in generic.body.statements.iter() {
                if let ast::Statement::Return(ret) = stmt {
                    escaped = Self::find_heap_vars_in_expr(&ret.return_value);
                    break;
                }
            }
            if escaped.is_empty() {
                if let Some(ast::Statement::Expression(es)) = generic.body.statements.last() {
                    escaped = Self::find_heap_vars_in_expr(&es.expression);
                }
            }

            // Pre-scan: find last-use points for each heap variable.
            let mut last_uses = Self::find_last_uses(&generic.body.statements);
            for var in &escaped {
                last_uses.remove(var);
            }

            let mut has_return = false;
            let mut last_value: BasicValueEnum<'ctx> = match &return_type {
                AhaType::String => self.string_type.const_zero().into(),
                AhaType::Struct(name) => {
                    self.struct_llvm_type(name)?.const_zero().into()
                }
                AhaType::Enum(name) => {
                    self.enum_llvm_type(name)?.const_zero().into()
                }
                _ => self.i64_type.const_int(0, false).into(),
            };

            for (stmt_idx, stmt) in generic.body.statements.iter().enumerate() {
                if let ast::Statement::Return(_) = stmt {
                    self.compile_statement(stmt)?;
                    has_return = true;
                    break;
                } else if let ast::Statement::Expression(expr_stmt) = stmt {
                    let tv = self.compile_expression(&expr_stmt.expression)?;
                    last_value = tv.value;
                } else {
                    self.compile_statement(stmt)?;
                }
                // Phase 2: insert free calls at last-use points
                for (var_name, &last_idx) in &last_uses {
                    if last_idx == stmt_idx {
                        self.insert_free_for_var(var_name);
                    }
                }
            }

            if !has_return && self.has_heap_locals() {
                self.insert_cleanup_inline(&escaped);
                self.builder.build_return(Some(&last_value))
                    .map_err(|e| e.to_string())?;
            } else if !has_return {
                self.builder.build_return(Some(&last_value))
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })();

        self.scopes = saved_scopes;
        self.current_function = saved_function;
        self.type_param_map = saved_tpm;
        if let Some(block) = saved_block {
            self.builder.position_at_end(block);
        }
        result?;

        let args_meta: Vec<_> = args.iter().map(|a| (*a).into()).collect();
        let call_result = self.builder.build_call(function, &args_meta, "calltmp")
            .map_err(|e| e.to_string())?;
        let val = call_result.try_as_basic_value()
            .left()
            .ok_or_else(|| "Generic call did not return a value".to_string())?;
        Ok(TypedValue::new(val, return_type))
    }

    fn compile_while_expression(&mut self, while_expr: &ast::WhileExpression) -> Result<TypedValue<'ctx>, String> {
        let function = self.builder.get_insert_block()
            .expect("Builder not in a block!")
            .get_parent()
            .unwrap();
        let condition_block = self.context.append_basic_block(function, "while_cond");
        let body_block = self.context.append_basic_block(function, "while_body");
        let after_block = self.context.append_basic_block(function, "while_after");
        self.builder.build_unconditional_branch(condition_block).map_err(|e| e.to_string())?;

        self.builder.position_at_end(condition_block);
        let condition_val = self.compile_expression(&while_expr.condition)?;
        let condition_bool = self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            condition_val.value.into_int_value(),
            self.i64_type.const_int(0, false),
            "while_cond_bool"
        ).map_err(|e| e.to_string())?;
        self.builder.build_conditional_branch(condition_bool, body_block, after_block).map_err(|e| e.to_string())?;

        self.builder.position_at_end(body_block);
        self.loop_stack.push((condition_block, after_block));
        self.compile_block_statement(&while_expr.body)?;
        self.loop_stack.pop();

        let body_end = self.builder.get_insert_block().unwrap();
        if body_end.get_terminator().is_none() {
            self.builder.build_unconditional_branch(condition_block).map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(after_block);
        Ok(TypedValue::void(self.i64_type.const_int(0, false).into()))
    }

    fn compile_if_expression(&mut self, if_expr: &ast::IfExpression) -> Result<TypedValue<'ctx>, String> {
        let condition_val = self.compile_expression(&if_expr.condition)?;
        let condition_bool = self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            condition_val.value.into_int_value(),
            self.i64_type.const_int(0, false),
            "if_cond_bool"
        ).map_err(|e| e.to_string())?;
        
        let function = self.builder.get_insert_block().expect("Builder not in block").get_parent().unwrap();
        let consequence_block = self.context.append_basic_block(function, "consequence");
        let alternative_block = self.context.append_basic_block(function, "alternative");
        let merge_block = self.context.append_basic_block(function, "merge");
        self.builder.build_conditional_branch(condition_bool, consequence_block, alternative_block)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(consequence_block);
        let consequence_tv = self.compile_block_statement(&if_expr.consequence)?;
        let consequence_end_block = self.builder.get_insert_block().unwrap();
        let consequence_terminated = consequence_end_block.get_terminator().is_some();
        if !consequence_terminated {
            self.builder.build_unconditional_branch(merge_block).map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(alternative_block);
        let alternative_tv = if let Some(alt_block) = &if_expr.alternative {
            self.compile_block_statement(alt_block)?
        } else {
            TypedValue::int(self.i64_type.const_int(0, false).into())
        };
        let alternative_end_block = self.builder.get_insert_block().unwrap();
        let alternative_terminated = alternative_end_block.get_terminator().is_some();
        if !alternative_terminated {
            self.builder.build_unconditional_branch(merge_block).map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(merge_block);

        if consequence_terminated && alternative_terminated {
            return Ok(TypedValue::int(self.i64_type.const_int(0, false).into()));
        }

        // Pick the phi LLVM type based on the branch types — i64 for Int/Bool,
        // string_type for String, struct type for Struct.
        let phi_type: inkwell::types::BasicTypeEnum<'ctx> = match (&consequence_tv.aha_type, &alternative_tv.aha_type) {
            (AhaType::String, _) | (_, AhaType::String) => self.string_type.into(),
            (AhaType::Struct(name), _) => self.struct_llvm_type(name)?.into(),
            (_, AhaType::Struct(name)) => self.struct_llvm_type(name)?.into(),
            (AhaType::Enum(name), _) => self.enum_llvm_type(name)?.into(),
            (_, AhaType::Enum(name)) => self.enum_llvm_type(name)?.into(),
            _ => self.i64_type.into(),
        };
        let phi_node = self.builder.build_phi(phi_type, "iftmp")
            .map_err(|e| e.to_string())?;
        if !consequence_terminated {
            phi_node.add_incoming(&[(&consequence_tv.value as &dyn inkwell::values::BasicValue, consequence_end_block)]);
        }
        if !alternative_terminated {
            phi_node.add_incoming(&[(&alternative_tv.value as &dyn inkwell::values::BasicValue, alternative_end_block)]);
        }
        Ok(TypedValue::new(phi_node.as_basic_value(), consequence_tv.aha_type))
    }
    
    fn compile_block_statement(&mut self, block: &ast::BlockStatement) -> Result<TypedValue<'ctx>, String> {
        self.enter_scope();
        let mut last = TypedValue::int(self.i64_type.const_int(0, false).into());
        for statement in &block.statements {
            if let ast::Statement::Expression(expr_stmt) = statement {
                last = self.compile_expression(&expr_stmt.expression)?;
            } else {
                self.compile_statement(statement)?;
            }
        }
        self.exit_scope();
        Ok(last)
    }

    fn compile_prefix_expression(&mut self, prefix: &ast::PrefixExpression) -> Result<TypedValue<'ctx>, String> {
        let right = self.compile_expression(&prefix.right)?;
        let result_type = right.aha_type.check_prefix_op(&prefix.operator)?;
        match prefix.operator.as_str() {
            "-" => {
                let neg = self.builder.build_int_neg(right.value.into_int_value(), "negtmp")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue::new(neg.into(), result_type))
            },
            "!" => {
                let is_zero = self.builder.build_int_compare(
                    inkwell::IntPredicate::EQ, right.value.into_int_value(),
                    self.i64_type.const_int(0, false), "is_zero"
                ).map_err(|e| e.to_string())?;
                let result = self.builder.build_int_z_extend(is_zero, self.i64_type, "nottmp")
                    .map_err(|e| e.to_string())?;
                Ok(TypedValue::new(result.into(), result_type))
            },
            _ => Err(format!("Unknown prefix operator: {}", prefix.operator)),
        }
    }

    fn compile_assignment(&mut self, assign: &ast::AssignmentExpression) -> Result<TypedValue<'ctx>, String> {
        let typed_val = self.compile_expression(&assign.value)?;

        // Handle field access: p.x = value
        if let ast::Expression::FieldAccess(fa) = &*assign.target {
            // Compile the object expression to get the struct value
            let object = self.compile_expression(&fa.object)?;
            let struct_name = match &object.aha_type {
                AhaType::Struct(name) => name.clone(),
                other => return Err(format!(
                    "Field access '.{}' on non-struct type {}", fa.field.value, other
                )),
            };
            let idx = self.field_index(&struct_name, &fa.field.value)?;
            let declared = self.field_type(&struct_name, &fa.field.value)?;

            // Type-check: the assigned value must match the field's declared type
            if declared == AhaType::String && !typed_val.aha_type.is_string() {
                return Err(format!(
                    "Field '{}' of '{}' expects a string, got {}",
                    fa.field.value, struct_name, typed_val.aha_type
                ));
            }
            if declared != AhaType::String && typed_val.aha_type.is_string() {
                return Err(format!(
                    "Field '{}' of '{}' expects {}, got string",
                    fa.field.value, struct_name, declared
                ));
            }

            // Load the struct from the variable, update the field, store back
            let struct_name_for_var = struct_name.clone();
            let struct_val = object.value.into_struct_value();
            let new_struct = self.builder
                .build_insert_value(struct_val, typed_val.value, idx, "mutfield")
                .map_err(|e| e.to_string())?
                .into_struct_value();

            // If the object expression is a variable, store the updated struct back
            if let ast::Expression::Identifier(id) = &*fa.object {
                if let Some(info) = self.lookup_variable(&id.value) {
                    self.builder.build_store(info.ptr, new_struct)
                        .map_err(|e| e.to_string())?;
                }
                // else: it's a temporary, no store needed
            }

            return Ok(TypedValue::struct_val(new_struct.into(), struct_name_for_var));
        }

        // Handle list indexing: xs[i] = value
        if let ast::Expression::Index(index_expr) = &*assign.target {
            let list_tv = self.compile_expression(&index_expr.left)?;
            let elem_type = match &list_tv.aha_type {
                AhaType::List(inner) => (**inner).clone(),
                other => return Err(format!(
                    "Index assignment target is not a List, got {}", other
                )),
            };
            let index_tv = self.compile_expression(&index_expr.index)?;

            // For String lists, the value must be a string; store the full
            // {i8*, i64} struct at data[index*elem_size].
            if elem_type.is_string() {
                if !typed_val.aha_type.is_string() {
                    return Err(format!(
                        "Assignment to List<String> element requires a string, got {}",
                        typed_val.aha_type
                    ));
                }
                let list_handle = list_tv.value.into_int_value();
                let hdr_ptr = self.builder.build_int_to_ptr(
                    list_handle,
                    self.list_header_type.ptr_type(inkwell::AddressSpace::default()),
                    "list_hdr"
                ).map_err(|e| e.to_string())?;
                let es_ptr = self.builder.build_struct_gep(hdr_ptr, 3, "es_ptr")
                    .map_err(|e| e.to_string())?;
                let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr")
                    .map_err(|e| e.to_string())?;
                let elem_size = self.builder.build_load(es_ptr, "es").map_err(|e| e.to_string())?.into_int_value();
                let data = self.builder.build_load(data_ptr, "data").map_err(|e| e.to_string())?.into_pointer_value();
                let byte_off = self.builder.build_int_mul(index_tv.value.into_int_value(), elem_size, "boff").map_err(|e| e.to_string())?;
                let elem_ptr = unsafe { self.builder.build_gep(data, &[byte_off], "elem_ptr") }
                    .map_err(|e| e.to_string())?;
                // Bitcast i8* element pointer to i64* for typed stores.
                let elem_i64_ptr = self.builder.build_bitcast(elem_ptr, self.i64_type.ptr_type(inkwell::AddressSpace::default()), "elem_i64_ptr")
                    .map_err(|e| e.to_string())?.into_pointer_value();
                let s_ptr = self.extract_str_ptr(&typed_val)?;
                let s_len = self.extract_str_len(&typed_val)?;
                let ptr_as_i64 = self.builder.build_ptr_to_int(s_ptr, self.i64_type, "ptr_as_i64")
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(elem_i64_ptr, ptr_as_i64).map_err(|e| e.to_string())?;
                let len_ptr = unsafe { self.builder.build_gep(elem_i64_ptr, &[self.i64_type.const_int(1, false)], "slen_ptr") }
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(len_ptr, s_len).map_err(|e| e.to_string())?;
                return Ok(list_tv);
            }

            // Int list: write the i64 directly at data[index*elem_size].
            let list_handle = list_tv.value.into_int_value();
            let hdr_ptr = self.builder.build_int_to_ptr(
                list_handle,
                self.list_header_type.ptr_type(inkwell::AddressSpace::default()),
                "list_hdr"
            ).map_err(|e| e.to_string())?;
            let es_ptr = self.builder.build_struct_gep(hdr_ptr, 3, "es_ptr")
                .map_err(|e| e.to_string())?;
            let data_ptr = self.builder.build_struct_gep(hdr_ptr, 0, "data_ptr")
                .map_err(|e| e.to_string())?;
            let elem_size = self.builder.build_load(es_ptr, "es").map_err(|e| e.to_string())?.into_int_value();
            let data = self.builder.build_load(data_ptr, "data").map_err(|e| e.to_string())?.into_pointer_value();
            let byte_off = self.builder.build_int_mul(index_tv.value.into_int_value(), elem_size, "boff").map_err(|e| e.to_string())?;
            let elem_ptr = unsafe { self.builder.build_gep(data, &[byte_off], "elem_ptr") }
                .map_err(|e| e.to_string())?;
            // Bitcast i8* element pointer to i64* before storing the i64.
            let elem_i64_ptr = self.builder.build_bitcast(elem_ptr, self.i64_type.ptr_type(inkwell::AddressSpace::default()), "elem_i64_ptr")
                .map_err(|e| e.to_string())?.into_pointer_value();
            self.builder.build_store(elem_i64_ptr, typed_val.value).map_err(|e| e.to_string())?;
            return Ok(list_tv);
        }

        // Handle plain variable: x = value
        if let ast::Expression::Identifier(id) = &*assign.target {
            if let Some(info) = self.lookup_variable(&id.value) {
                let ptr = info.ptr;
                self.builder.build_store(ptr, typed_val.value)
                    .map_err(|e| e.to_string())?;
                return Ok(typed_val);
            }
            return Err(format!("Cannot assign to undefined variable: '{}'", id.value));
        }

        Err(format!(
            "Cannot assign to expression of type {:?}",
            assign.target
        ))
    }

    fn compile_for_expression(&mut self, for_expr: &ast::ForExpression) -> Result<TypedValue<'ctx>, String> {
        let function = self.builder.get_insert_block().expect("Builder not in a block!").get_parent().unwrap();
        let (start_val, end_val) = match &*for_expr.iterable {
            ast::Expression::Range(range) => {
                let start = self.compile_expression(&range.start)?;
                let end = self.compile_expression(&range.end)?;
                (start, end)
            },
            _ => return Err("for loop currently only supports range expressions (start..end)".to_string()),
        };
        let loop_var_ptr = self.builder.build_alloca(self.i64_type, &for_expr.variable.value)
            .map_err(|e| e.to_string())?;
        self.builder.build_store(loop_var_ptr, start_val.value).map_err(|e| e.to_string())?;
        self.insert_variable(for_expr.variable.value.clone(), loop_var_ptr, AhaType::Int);

        let cond_block = self.context.append_basic_block(function, "for_cond");
        let body_block = self.context.append_basic_block(function, "for_body");
        let increment_block = self.context.append_basic_block(function, "for_incr");
        let after_block = self.context.append_basic_block(function, "for_after");
        self.builder.build_unconditional_branch(cond_block).map_err(|e| e.to_string())?;

        self.builder.position_at_end(cond_block);
        let current_val = self.builder.build_load(loop_var_ptr, "loop_var").map_err(|e| e.to_string())?;
        let cond = self.builder.build_int_compare(
            inkwell::IntPredicate::SLT, current_val.into_int_value(),
            end_val.value.into_int_value(), "for_cond"
        ).map_err(|e| e.to_string())?;
        self.builder.build_conditional_branch(cond, body_block, after_block).map_err(|e| e.to_string())?;

        // 'continue' must jump to the increment block, not the condition
        // block, otherwise the loop variable never advances (infinite loop).
        self.builder.position_at_end(body_block);
        self.loop_stack.push((increment_block, after_block));
        self.compile_block_statement(&for_expr.body)?;
        self.loop_stack.pop();

        let body_end = self.builder.get_insert_block().unwrap();
        if body_end.get_terminator().is_none() {
            self.builder.build_unconditional_branch(increment_block).map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(increment_block);
        let current = self.builder.build_load(loop_var_ptr, "cur").map_err(|e| e.to_string())?;
        let next = self.builder.build_int_add(current.into_int_value(), self.i64_type.const_int(1, false), "next")
            .map_err(|e| e.to_string())?;
        self.builder.build_store(loop_var_ptr, next).map_err(|e| e.to_string())?;
        self.builder.build_unconditional_branch(cond_block).map_err(|e| e.to_string())?;

        self.builder.position_at_end(after_block);
        Ok(TypedValue::void(self.i64_type.const_int(0, false).into()))
    }

    fn compile_range_expression(&mut self, range: &ast::RangeExpression) -> Result<TypedValue<'ctx>, String> {
        self.compile_expression(&range.start)
    }

    // --- Struct support ---

    /// Walk top-level statements and record every struct definition's
    /// field order + declared type so literals and field access can
    /// resolve layout and check types.
    fn register_structs(&mut self, statements: &[ast::Statement]) {
        for stmt in statements {
            let fields = match stmt {
                ast::Statement::Struct(def) => &def.fields,
                ast::Statement::Actor(def) => &def.fields,
                _ => continue,
            };
            let field_data: Vec<(String, AhaType)> = fields.iter()
                .map(|f| {
                    let t = f.type_hint.as_deref()
                        .and_then(AhaType::from_hint)
                        .unwrap_or(AhaType::Int);
                    (f.name.value.clone(), t)
                })
                .collect();
            let name = match stmt {
                ast::Statement::Struct(def) => def.name.value.clone(),
                ast::Statement::Actor(def) => def.name.value.clone(),
                _ => unreachable!(),
            };
            self.struct_defs.insert(name, field_data);
        }
    }

    /// Declared type of a struct field.
    fn field_type(&self, struct_name: &str, field: &str) -> Result<AhaType, String> {
        let fields = self.struct_defs.get(struct_name)
            .ok_or_else(|| format!("Unknown struct type '{}'", struct_name))?;
        fields.iter()
            .find(|(f, _)| f == field)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| format!("Struct '{}' has no field '{}'", struct_name, field))
    }

    /// LLVM type for a registered struct. Each field uses its declared
    /// type: String → {i8*, i64}, everything else → i64.
    fn struct_llvm_type(&self, name: &str) -> Result<inkwell::types::StructType<'ctx>, String> {
        let fields = self.struct_defs.get(name)
            .ok_or_else(|| format!("Unknown struct type '{}'", name))?;
        let field_types: Vec<inkwell::types::BasicTypeEnum<'ctx>> = fields.iter()
            .map(|(_, t)| match t {
                AhaType::String => self.string_type.into(),
                _ => self.i64_type.into(),
            })
            .collect();
        Ok(self.context.struct_type(&field_types, false))
    }

    /// Index of a field within a struct's layout.
    fn field_index(&self, struct_name: &str, field: &str) -> Result<u32, String> {
        let fields = self.struct_defs.get(struct_name)
            .ok_or_else(|| format!("Unknown struct type '{}'", struct_name))?;
        fields.iter().position(|(f, _)| f == field)
            .map(|i| i as u32)
            .ok_or_else(|| format!("Struct '{}' has no field '{}'", struct_name, field))
    }

    // --- Enum support ---

    /// Walk top-level statements and record every enum definition's
    /// variant names + payload types for constructors and match.
    fn register_enums(&mut self, statements: &[ast::Statement]) {
        for stmt in statements {
            if let ast::Statement::Enum(def) = stmt {
                let variants: Vec<(String, Vec<AhaType>)> = def.variants.iter()
                    .map(|v| {
                        let types: Vec<AhaType> = v.payload_types.iter()
                            .map(|t| {
                                // Check if t is a known enum name (nested enum)
                                if self.enum_defs.contains_key(t.as_str()) {
                                    AhaType::Enum(t.clone())
                                } else {
                                    AhaType::from_hint(t).unwrap_or(AhaType::Int)
                                }
                            })
                            .collect();
                        (v.name.value.clone(), types)
                    })
                    .collect();
                self.enum_defs.insert(def.name.value.clone(), variants);
            }
        }
    }

    /// Find which enum owns a variant name by scanning all registered enums.
    fn find_enum_for_variant(&self, variant_name: &str) -> Option<String> {
        for (enum_name, variants) in &self.enum_defs {
            if variants.iter().any(|(name, _)| name == variant_name) {
                return Some(enum_name.clone());
            }
        }
        None
    }

    /// LLVM struct type for an enum: {i64 tag, i64, i64, ...} where
    /// the number of i64 slots after the tag equals the max payload size.
    fn enum_llvm_type(&self, name: &str) -> Result<inkwell::types::StructType<'ctx>, String> {
        let variants = self.enum_defs.get(name)
            .ok_or_else(|| format!("Unknown enum type '{}'", name))?;
        // Count total payload slots, expanding nested enums recursively.
        let max_payload: usize = variants.iter()
            .map(|(_, types)| -> usize {
                types.iter().map(|t| match t {
                    AhaType::Enum(inner) => {
                        // Nested enum: tag + its own payload slots
                        let inner_variants = self.enum_defs.get(inner).map_or(1, |v| {
                            1 + v.iter().map(|(_, ts)| ts.len()).max().unwrap_or(0)
                        });
                        inner_variants
                    }
                    _ => 1,
                }).sum()
            })
            .max()
            .unwrap_or(0);
        let mut field_types: Vec<inkwell::types::BasicTypeEnum<'ctx>> =
            Vec::with_capacity(1 + max_payload);
        field_types.push(self.i64_type.into()); // tag
        for _ in 0..max_payload {
            field_types.push(self.i64_type.into()); // payload slots
        }
        Ok(self.context.struct_type(&field_types, false))
    }

    /// Variant tag index (0-based, declaration order).
    fn variant_tag(&self, enum_name: &str, variant_name: &str) -> Result<u64, String> {
        let variants = self.enum_defs.get(enum_name)
            .ok_or_else(|| format!("Unknown enum type '{}'", enum_name))?;
        variants.iter()
            .position(|(name, _)| name == variant_name)
            .map(|i| i as u64)
            .ok_or_else(|| format!("Enum '{}' has no variant '{}'", enum_name, variant_name))
    }

    /// Payload types for a variant.
    fn variant_payload(&self, enum_name: &str, variant_name: &str) -> Result<Vec<AhaType>, String> {
        let variants = self.enum_defs.get(enum_name)
            .ok_or_else(|| format!("Unknown enum type '{}'", enum_name))?;
        variants.iter()
            .find(|(name, _)| name == variant_name)
            .map(|(_, types)| types.clone())
            .ok_or_else(|| format!("Enum '{}' has no variant '{}'", enum_name, variant_name))
    }

    /// Compile an enum constructor call: `Variant(args...)` or `Variant`.
    /// Called from compile_call when the name matches a registered enum variant.
    fn compile_enum_constructor(&mut self, enum_name: &str, variant_name: &str, call: &ast::CallExpression) -> Result<TypedValue<'ctx>, String> {
        let tag = self.variant_tag(enum_name, variant_name)?;
        let payload_types = self.variant_payload(enum_name, variant_name)?;
        let enum_type = self.enum_llvm_type(enum_name)?;

        // Verify argument count matches payload arity.
        if call.arguments.len() != payload_types.len() {
            return Err(format!(
                "Enum variant '{}::{}' expects {} arguments, got {}",
                enum_name, variant_name, payload_types.len(), call.arguments.len()
            ));
        }

        let mut val = enum_type.const_zero();
        // Set tag (field 0).
        val = self.builder.build_insert_value(val, self.i64_type.const_int(tag, false), 0, "tag")
            .map_err(|e| e.to_string())?
            .into_struct_value();

        // Set payload fields (field 1, 2, ...).
        let mut field_idx: u32 = 1;
        for (i, arg) in call.arguments.iter().enumerate() {
            let tv = self.compile_expression(arg)?;
            let expected = &payload_types[i];
            if !Self::types_compatible(&tv.aha_type, expected) {
                return Err(format!(
                    "Enum variant '{}::{}' arg {} expects {}, got {}",
                    enum_name, variant_name, i, expected, tv.aha_type
                ));
            }
            if let AhaType::Enum(_) = &tv.aha_type {
                // Flatten nested enum: extract tag + payload fields as separate i64s
                let inner_struct = tv.value.into_struct_value();
                let inner_tag = self.builder.build_extract_value(inner_struct, 0, "inner_tag")
                    .map_err(|e| e.to_string())?;
                val = self.builder.build_insert_value(val, inner_tag, field_idx, "payload")
                    .map_err(|e| e.to_string())?
                    .into_struct_value();
                field_idx += 1;
                // Extract inner payload slots
                let inner_variants = self.enum_defs.get(match &tv.aha_type {
                    AhaType::Enum(n) => n.as_str(),
                    _ => unreachable!(),
                });
                let inner_max_payload = inner_variants.map_or(0, |v| {
                    v.iter().map(|(_, ts)| ts.len()).max().unwrap_or(0)
                });
                for j in 0..inner_max_payload {
                    let slot = self.builder.build_extract_value(inner_struct, (j + 1) as u32, "inner_payload")
                        .map_err(|e| e.to_string())?;
                    val = self.builder.build_insert_value(val, slot, field_idx, "payload")
                        .map_err(|e| e.to_string())?
                        .into_struct_value();
                    field_idx += 1;
                }
            } else {
                val = self.builder.build_insert_value(val, tv.value, field_idx, "payload")
                    .map_err(|e| e.to_string())?
                    .into_struct_value();
                field_idx += 1;
            }
        }

        Ok(TypedValue::new(val.into(), AhaType::Enum(enum_name.to_string())))
    }

    /// Loose type compatibility check for enum payloads (Int~Bool, same name).
    fn types_compatible(a: &AhaType, b: &AhaType) -> bool {
        if a == b { return true; }
        match (a, b) {
            (AhaType::Int, AhaType::Bool) | (AhaType::Bool, AhaType::Int) => true,
            _ => false,
        }
    }

    /// Compile: match expr { Pattern => body, ... }
    fn compile_match_expression(&mut self, m: &ast::MatchExpression) -> Result<TypedValue<'ctx>, String> {
        let scrutinee = self.compile_expression(&m.value)?;
        let enum_name = match &scrutinee.aha_type {
            AhaType::Enum(name) => name.clone(),
            _ => return Err(format!(
                "match requires an enum value, got {}", scrutinee.aha_type
            )),
        };

        let enum_type = self.enum_llvm_type(&enum_name)?;
        let current_fn = self.current_function.ok_or("match outside function")?;
        Self::diag_mark(&format!("MATCH: current_fn='{}', builder_block='{}'",
            current_fn.get_name().to_str().unwrap_or("?"),
            self.builder.get_insert_block().map(|b| b.get_name().to_str().unwrap_or("?").to_string()).unwrap_or("?".to_string())));
        let merge_block = self.context.append_basic_block(current_fn, "match.merge");

        // Load the tag (field 0) for branching.
        let tag_val = self.builder.build_extract_value(scrutinee.value.into_struct_value(), 0, "tag")
            .map_err(|e| e.to_string())?
            .into_int_value();

        // Create blocks for each arm.
        let arm_count = m.arms.len();
        let mut arm_blocks = Vec::with_capacity(arm_count);
        for i in 0..arm_count {
            let bb = self.context.append_basic_block(current_fn, &format!("match.arm{}", i));
            arm_blocks.push(bb);
        }

        // Build switch cases: collect all (IntValue, BasicBlock) pairs.
        // For exhaustive matches (no wildcard), default goes to an unreachable
        // dead block — never executed but satisfies LLVM IR predecessor requirements.
        let default_bb = if let Some(wi) = m.arms.iter().position(|a| matches!(a.pattern, ast::Pattern::Wildcard)) {
            arm_blocks[wi]
        } else {
            let dead = self.context.append_basic_block(current_fn, "match.dead");
            self.builder.position_at_end(dead);
            self.builder.build_unreachable().map_err(|e| e.to_string())?;
            dead
        };
        let mut cases: Vec<(inkwell::values::IntValue<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> = Vec::new();
        for (i, arm) in m.arms.iter().enumerate() {
            if let ast::Pattern::EnumUnit(name) | ast::Pattern::EnumTuple(name, _) = &arm.pattern {
                let tag = self.variant_tag(&enum_name, name)?;
                let case_val = self.i64_type.const_int(tag, false);
                cases.push((case_val, arm_blocks[i]));
            }
        }
        self.builder.build_switch(tag_val, default_bb, &cases)
            .map_err(|e| e.to_string())?;

        // Compile each arm body.
        let mut results: Vec<(BasicValueEnum<'ctx>, AhaType, inkwell::basic_block::BasicBlock<'ctx>)> = Vec::new();
        for (i, arm) in m.arms.iter().enumerate() {
            self.builder.position_at_end(arm_blocks[i]);
            self.enter_scope();

            // Destructure bindings for tuple patterns.
            if let ast::Pattern::EnumTuple(name, bindings) = &arm.pattern {
                let payload = self.variant_payload(&enum_name, name)?;
                let mut field_idx: u32 = 1;
                for (j, binding) in bindings.iter().enumerate() {
                    if j >= payload.len() { break; }
                    if let AhaType::Enum(inner_name) = &payload[j] {
                        // Reconstruct nested enum from flattened fields
                        let inner_type = self.enum_llvm_type(inner_name)?;
                        let mut inner_val = inner_type.const_zero();
                        // Extract inner tag
                        let inner_tag = self.builder.build_extract_value(
                            scrutinee.value.into_struct_value(),
                            field_idx,
                            "inner_tag",
                        ).map_err(|e| e.to_string())?;
                        inner_val = self.builder.build_insert_value(inner_val, inner_tag, 0, "tag")
                            .map_err(|e| e.to_string())?
                            .into_struct_value();
                        field_idx += 1;
                        // Extract inner payload slots
                        let inner_variants = self.enum_defs.get(inner_name.as_str());
                        let inner_max_payload = inner_variants.map_or(0, |v| {
                            v.iter().map(|(_, ts)| ts.len()).max().unwrap_or(0)
                        });
                        for k in 0..inner_max_payload {
                            let slot = self.builder.build_extract_value(
                                scrutinee.value.into_struct_value(),
                                field_idx,
                                "inner_payload",
                            ).map_err(|e| e.to_string())?;
                            inner_val = self.builder.build_insert_value(inner_val, slot, (k + 1) as u32, "payload")
                                .map_err(|e| e.to_string())?
                                .into_struct_value();
                            field_idx += 1;
                        }
                        let ptr = self.builder.build_alloca(inner_type, binding)
                            .map_err(|e| e.to_string())?;
                        self.builder.build_store(ptr, inner_val).map_err(|e| e.to_string())?;
                        self.insert_variable(binding.clone(), ptr, payload[j].clone());
                    } else {
                        let field_val = self.builder.build_extract_value(
                            scrutinee.value.into_struct_value(),
                            field_idx,
                            "destructure",
                        ).map_err(|e| e.to_string())?;
                        let ptr = self.builder.build_alloca(
                            self.aha_type_to_llvm_type(&payload[j])?,
                            binding,
                        ).map_err(|e| e.to_string())?;
                        self.builder.build_store(ptr, field_val).map_err(|e| e.to_string())?;
                        self.insert_variable(binding.clone(), ptr, payload[j].clone());
                        field_idx += 1;
                    }
                }
            }

            let tv = self.compile_expression(&arm.body)?;
            self.exit_scope();
            if !self.builder.get_insert_block().unwrap().get_terminator().is_some() {
                self.builder.build_unconditional_branch(merge_block).map_err(|e| e.to_string())?;
            }
            let block = self.builder.get_insert_block().unwrap();
            results.push((tv.value, tv.aha_type, block));
        }

        // Merge: phi node across all arms.
        self.builder.position_at_end(merge_block);
        let result_type = &results[0].1;
        let phi_llvm_type = self.aha_type_to_llvm_type(result_type)?;
        let phi = self.builder.build_phi(phi_llvm_type, "match.result").map_err(|e| e.to_string())?;
        for (val, _typ, block) in &results {
            // ponytail: every arm block branches to merge — add all unconditionally
            phi.add_incoming(&[(val as &dyn inkwell::values::BasicValue, *block)]);
        }

        Ok(TypedValue::new(phi.as_basic_value(), result_type.clone()))
    }

    fn compile_struct_literal(&mut self, lit: &ast::StructLiteral) -> Result<TypedValue<'ctx>, String> {
        let struct_name = lit.name.value.clone();
        let struct_type = self.struct_llvm_type(&struct_name)?;

        // Compile each provided field's value and place it at the correct
        // index defined by the struct declaration order. Missing fields
        // default to 0. field_index() rejects unknown fields.
        let mut struct_val = struct_type.const_zero();
        for (field_ident, value_expr) in &lit.fields {
            let idx = self.field_index(&struct_name, &field_ident.value)?;
            let declared = self.field_type(&struct_name, &field_ident.value)?;
            let value = self.compile_expression(value_expr)?;
            // Type-check: a field declared `string` must be given a string
            // literal/variable; everything else is stored as i64.
            if declared == AhaType::String && !value.aha_type.is_string() {
                return Err(format!(
                    "Field '{}' of '{}' expects a string, got {}",
                    field_ident.value, struct_name, value.aha_type
                ));
            }
            if declared != AhaType::String && value.aha_type.is_string() {
                return Err(format!(
                    "Field '{}' of '{}' expects {}, got string",
                    field_ident.value, struct_name, declared
                ));
            }
            struct_val = self.builder
                .build_insert_value(struct_val, value.value, idx, "structfield")
                .map_err(|e| e.to_string())?
                .into_struct_value();
        }

        Ok(TypedValue::struct_val(struct_val.into(), struct_name))
    }

    fn compile_field_access(&mut self, access: &ast::FieldAccess) -> Result<TypedValue<'ctx>, String> {
        let object = self.compile_expression(&access.object)?;
        let struct_name = match &object.aha_type {
            AhaType::Struct(name) => name.clone(),
            other => return Err(format!(
                "Field access '.{}' on non-struct type {}", access.field.value, other
            )),
        };
        let idx = self.field_index(&struct_name, &access.field.value)?;
        let declared = self.field_type(&struct_name, &access.field.value)?;
        let struct_val = object.value.into_struct_value();
        let field_val = self.builder
            .build_extract_value(struct_val, idx, "fieldval")
            .map_err(|e| e.to_string())?;
        match declared {
            AhaType::String => Ok(TypedValue::string(field_val)),
            _ => Ok(TypedValue::int(field_val)),
        }
    }


    pub fn print_llvm_ir(&self) {
        self.module.print_to_stderr();
    }

    // Get LLVM IR as string for saving to file
    pub fn get_llvm_ir(&self) -> String {
        self.module.print_to_string().to_string()
    }
    
    pub fn run_jit(&self) -> Result<i64, String> {
        let execution_engine = self.module.create_jit_execution_engine(inkwell::OptimizationLevel::None)
            .map_err(|e| format!("Failed to create JIT engine: {}", e))?;

        // Register native runtime functions so the JIT can call them.
        // Without add_global_mapping, MCJIT can't resolve #[no_mangle] symbols
        // in test binaries on Linux.
        if let Some(f) = self.module.get_function("actor_spawn") {
            execution_engine.add_global_mapping(&f, crate::runtime::actor_spawn as usize);
        }
        if let Some(f) = self.module.get_function("actor_send") {
            execution_engine.add_global_mapping(&f, crate::runtime::actor_send as usize);
        }
        if let Some(f) = self.module.get_function("actor_call") {
            execution_engine.add_global_mapping(&f, crate::runtime::actor_call as usize);
        }

        let function_name = "main";
        let _function = self.module.get_function(function_name)
            .ok_or_else(|| format!("Function '{}' not found", function_name))?;

        unsafe {
            let compiled_fn: unsafe extern "C" fn() -> i64 = execution_engine.get_function_address(function_name)
                .map_err(|e| format!("Failed to get function address: {}", e))
                .map(|addr| std::mem::transmute(addr))?;

            Ok(compiled_fn())
        }
    }

    /// Rename the LLVM `main` function to `new_name`.
    /// Used during AOT compilation to free up the name for a C-compatible wrapper.
    pub fn rename_main(&mut self, new_name: &str) {
        if let Some(f) = self.module.get_function("main") {
            f.as_global_value().set_name(new_name);
        }
    }

    /// Add a C-compatible `main` wrapper that calls `__aha_main() -> i64`.
    /// Must be called after `rename_main("__aha_main")`.
    pub fn add_c_main_wrapper(&mut self) {
        let i32_type = self.context.i32_type();

        // Declare __aha_main() -> i64
        let aha_main_fn = self.module.get_function("__aha_main")
            .expect("__aha_main not found — call rename_main first");
        let wrapper_type = i32_type.fn_type(&[], false);
        let wrapper_fn = self.module.add_function("main", wrapper_type, None);

        let entry = self.context.append_basic_block(wrapper_fn, "entry");
        self.builder.position_at_end(entry);

        let call = self.builder.build_call(aha_main_fn, &[], "aha_result")
            .expect("failed to call __aha_main");
        let result = call.try_as_basic_value().left().unwrap().into_int_value();

        // Truncate i64 to i32 for C main return
        let truncated = self.builder.build_int_truncate(result, i32_type, "ret")
            .expect("truncate failed");
        self.builder.build_return(Some(&truncated)).expect("return failed");
    }

    /// Emit object file (.o) for AOT compilation.
    /// Returns the path to the written object file.
    pub fn emit_object_file(&self, path: &std::path::Path) -> Result<(), String> {
        use inkwell::targets::{Target, TargetMachine, FileType, InitializationConfig, RelocMode, CodeModel};

        // Initialize native target
        Target::initialize_native(&InitializationConfig::default())
            .map_err(|e| format!("Failed to init native target: {}", e))?;
        Target::initialize_x86(&InitializationConfig::default());
        Target::initialize_aarch64(&InitializationConfig::default());

        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple)
            .map_err(|e| format!("Failed to get target: {}", e))?;

        let cpu = TargetMachine::get_host_cpu_name()
            .to_string_lossy().to_string();
        let features = TargetMachine::get_host_cpu_features()
            .to_string_lossy().to_string();

        let target_machine = target.create_target_machine(
            &triple,
            &cpu,
            &features,
            inkwell::OptimizationLevel::Default,
            RelocMode::Default,
            CodeModel::Default,
        ).ok_or("Failed to create target machine")?;

        target_machine.write_to_file(&self.module, FileType::Object, path)
            .map_err(|e| format!("Failed to write object file: {}", e))?;

        Ok(())
    }
}


