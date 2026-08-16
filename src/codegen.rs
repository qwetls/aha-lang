// src/codegen.rs

use crate::ast;
use crate::types::{AhaType, TypedValue};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::values::{PointerValue, BasicValueEnum, FunctionValue};
use inkwell::types::{IntType, StructType};
use std::collections::HashMap;

/// Variable info stored in scope: LLVM pointer + AHA! type
#[derive(Clone, Debug)]
struct VarInfo<'ctx> {
    ptr: PointerValue<'ctx>,
    var_type: AhaType,
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
            current_function: None,
            loop_stack: Vec::new(),
            param_type_map: HashMap::new(),
            struct_defs: HashMap::new(),
            struct_var_types: HashMap::new(),
            scan_scope: Vec::new(),
            generic_defs: HashMap::new(),
            type_param_map: HashMap::new(),
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
            scope.insert(name, VarInfo { ptr, var_type });
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
                    }
                    self.scan_expr_for_calls(&let_stmt.value);
                }
                ast::Statement::Return(ret_stmt) => {
                    self.scan_expr_for_calls(&ret_stmt.return_value);
                }
                ast::Statement::Struct(_) => {}
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
        if let Some(t) = AhaType::from_hint(hint) {
            return t;
        }
        if self.struct_defs.contains_key(hint) {
            return AhaType::Struct(hint.to_string());
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
            _ => AhaType::Int,
        }
    }

    /// Infer a function's return type for the pre-declaration pass.
    /// Walks the body looking for the last expression value or an
    /// explicit `return` statement, then types that expression with
    /// the function's own params in scope (so `a + b` is String when
    /// a and b are string params).
    fn infer_function_return_type(&self, func: &ast::FunctionLiteral, func_name: &str) -> AhaType {
        let param_types = self.infer_param_types_immutable(func_name, &func.parameters);

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
    fn infer_param_types_immutable(&self, func_name: &str, params: &[ast::Identifier]) -> Vec<AhaType> {
        let mut types = vec![AhaType::Int; params.len()];
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
                if let ast::Expression::Identifier(id) = call.function.as_ref() {
                    if id.value == "list_push" || id.value == "list_push_string" {
                        if let Some(first) = call.arguments.first() {
                            return self.infer_expr_type_with_scope(first, scope);
                        }
                    }
                    if id.value == "list_get" || id.value == "list_get_string" {
                        if let Some(first) = call.arguments.first() {
                            let list_type = self.infer_expr_type_with_scope(first, scope);
                            if let AhaType::List(inner) = list_type {
                                return *inner;
                            }
                        }
                        return AhaType::Int;
                    }
                    if id.value == "len" {
                        return AhaType::Int;
                    }
                    if let Some(rt) = self.fn_types.get(&id.value) {
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

    pub fn compile(&mut self, program: &ast::Program) -> Result<(), String> {
        self.declare_printf();
        self.declare_c_runtime();
        // List builtins depend on malloc/realloc/free from the C runtime.
        self.create_list_builtins();

        // Register struct definitions first so struct literals and field
        // access can resolve field layout during codegen.
        self.register_structs(&program.statements);

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

        let fn_type = self.i64_type.fn_type(&[], false);
        let function = self.module.add_function("main", fn_type, None);
        let basic_block = self.context.append_basic_block(function, "entry");

        self.builder.position_at_end(basic_block);

        let mut last_value: Option<TypedValue<'ctx>> = None;

        for (i, statement) in program.statements.iter().enumerate() {
            let is_last = i == program.statements.len() - 1;
            
            if is_last {
                if let ast::Statement::Expression(expr_stmt) = statement {
                    let val = self.compile_expression(&expr_stmt.expression)?;
                    last_value = Some(val);
                    continue;
                }
            }
            self.compile_statement(statement)?;
        }
        
        let return_val = last_value
            .map(|tv| tv.value)
            .unwrap_or_else(|| self.i64_type.const_int(0, false).into());
        let _ = self.builder.build_return(Some(&return_val));
        
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
        let i64_type = self.i64_type;
        let i8_ptr = self.i8_ptr_type();
        let header = self.list_header_type;
        let header_ptr = header.ptr_type(inkwell::AddressSpace::default());

        // Helper: header pointer from a list handle (i64).
        // Used by every list_* builtin after the first.
        let header_from_handle = |builder: &Builder<'ctx>, handle: inkwell::values::IntValue<'ctx>| {
            builder.build_int_to_ptr(handle, header_ptr, "list_hdr").expect("int_to_ptr failed")
        };

        // --- list_new() -> List<Int> ---
        {
            let fn_type = i64_type.fn_type(&[], false);
            let function = self.module.add_function("list_new", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let malloc_fn = *self.functions.get("malloc").expect("malloc not declared");
            let hdr_size = i64_type.const_int(32, false); // 4 x i64 header
            let hdr = self.builder.build_call(malloc_fn, &[hdr_size.into()], "list_hdr")
                .expect("malloc failed")
                .try_as_basic_value().left().expect("malloc void")
                .into_pointer_value();

            // Zero the whole header explicitly — malloc memory is garbage.
            let zero = i64_type.const_int(0, false);
            let hdr_ptr = self.builder.build_bitcast(hdr, header_ptr, "hdr_typed")
                .expect("bitcast failed").into_pointer_value();
            let data_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[zero, zero], "data_ptr") }
                .expect("gep failed");
            self.builder.build_store(data_ptr, self.i8_ptr_type().const_null()).expect("store failed");
            let len_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[zero, i64_type.const_int(1, false)], "len_ptr") }
                .expect("gep failed");
            self.builder.build_store(len_ptr, zero).expect("store failed");
            let cap_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[zero, i64_type.const_int(2, false)], "cap_ptr") }
                .expect("gep failed");
            self.builder.build_store(cap_ptr, zero).expect("store failed");

            // elem_size = 8 (Int)
            let es_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[zero, i64_type.const_int(3, false)], "es_ptr") }
                .expect("gep failed");
            self.builder.build_store(es_ptr, i64_type.const_int(8, false)).expect("store failed");

            // Return handle as i64 (header address).
            let handle = self.builder.build_ptr_to_int(hdr, i64_type, "list_handle")
                .expect("ptr_to_int failed");
            let _ = self.builder.build_return(Some(&handle));
            self.functions.insert("list_new".to_string(), function);
        }

        // --- list_new_string() -> List<String> (elem_size 16) ---
        {
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
            let data_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[zero, zero], "data_ptr") }
                .expect("gep failed");
            self.builder.build_store(data_ptr, self.i8_ptr_type().const_null()).expect("store failed");
            let len_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[zero, i64_type.const_int(1, false)], "len_ptr") }
                .expect("gep failed");
            self.builder.build_store(len_ptr, zero).expect("store failed");
            let cap_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[zero, i64_type.const_int(2, false)], "cap_ptr") }
                .expect("gep failed");
            self.builder.build_store(cap_ptr, zero).expect("store failed");
            let es_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[zero, i64_type.const_int(3, false)], "es_ptr") }
                .expect("gep failed");
            self.builder.build_store(es_ptr, i64_type.const_int(16, false)).expect("store failed");
            let handle = self.builder.build_ptr_to_int(hdr, i64_type, "list_handle").expect("ptr_to_int failed");
            let _ = self.builder.build_return(Some(&handle));
            self.functions.insert("list_new_string".to_string(), function);
        }

        // --- list_push(list, value) -> list ---
        {
            let fn_type = i64_type.fn_type(&[i64_type.into(), i64_type.into()], false);
            let function = self.module.add_function("list_push", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("push: param 0").into_int_value();
            let value = function.get_nth_param(1).expect("push: param 1").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);

            // Load len and cap.
            let len_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(1, false)], "len_ptr") }
                .expect("gep failed");
            let cap_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(2, false)], "cap_ptr") }
                .expect("gep failed");
            let es_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(3, false)], "es_ptr") }
                .expect("gep failed");
            let data_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(0, false)], "data_ptr") }
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
            let fn_type = i64_type.fn_type(&[i64_type.into(), i8_ptr.into(), i64_type.into()], false);
            let function = self.module.add_function("list_push_string", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("push_s: param 0").into_int_value();
            let str_ptr = function.get_nth_param(1).expect("push_s: param 1").into_pointer_value();
            let str_len = function.get_nth_param(2).expect("push_s: param 2").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);

            let len_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(1, false)], "len_ptr") }
                .expect("gep failed");
            let cap_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(2, false)], "cap_ptr") }
                .expect("gep failed");
            let es_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(3, false)], "es_ptr") }
                .expect("gep failed");
            let data_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(0, false)], "data_ptr") }
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
            let fn_type = i64_type.fn_type(&[i64_type.into(), i64_type.into()], false);
            let function = self.module.add_function("list_get", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("get: param 0").into_int_value();
            let index = function.get_nth_param(1).expect("get: param 1").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);

            let len_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(1, false)], "len_ptr") }
                .expect("gep failed");
            let es_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(3, false)], "es_ptr") }
                .expect("gep failed");
            let data_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(0, false)], "data_ptr") }
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
            let fn_type = self.string_type.fn_type(&[i64_type.into(), i64_type.into()], false);
            let function = self.module.add_function("list_get_string", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("get_s: param 0").into_int_value();
            let index = function.get_nth_param(1).expect("get_s: param 1").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);

            let len_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(1, false)], "len_ptr") }
                .expect("gep failed");
            let es_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(3, false)], "es_ptr") }
                .expect("gep failed");
            let data_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(0, false)], "data_ptr") }
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
            let fn_type = i64_type.fn_type(&[i64_type.into()], false);
            let function = self.module.add_function("list_len", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("list_len: param 0").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);
            let len_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(1, false)], "len_ptr") }
                .expect("gep failed");
            let len = self.builder.build_load(len_ptr, "len").expect("load failed");
            let _ = self.builder.build_return(Some(&len));
            self.functions.insert("list_len".to_string(), function);
        }

        // --- list_free(list) -> i64 (0) ---
        {
            let fn_type = i64_type.fn_type(&[i64_type.into()], false);
            let function = self.module.add_function("list_free", fn_type, None);
            let entry = self.context.append_basic_block(function, "entry");
            self.builder.position_at_end(entry);

            let handle = function.get_nth_param(0).expect("list_free: param 0").into_int_value();
            let hdr_ptr = header_from_handle(&self.builder, handle);
            let data_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[i64_type.const_int(0, false), i64_type.const_int(0, false)], "data_ptr") }
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
                    // If the struct_defs registry has a matching name, use Struct.
                    let hint_type = if self.struct_defs.contains_key(hint) {
                        AhaType::Struct(hint.clone())
                    } else {
                        hint_type
                    };
                    // Type-check: annotation must match the inferred type.
                    // Struct("Point") vs Struct("Point") is compatible.
                    // Int annotation with String value → error.
                    let compatible = match (&hint_type, &typed_val.aha_type) {
                        (AhaType::Struct(a), AhaType::Struct(b)) => a == b,
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
                self.builder.build_return(Some(&typed_val.value))
                    .map_err(|e| e.to_string())?;
            },
            ast::Statement::Struct(_struct_def) => {
                // Struct definitions are compile-time metadata
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
    fn infer_param_types(&mut self, func_name: &str, params: &[ast::Identifier]) -> Vec<AhaType> {
        let mut types = vec![AhaType::Int; params.len()];
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
        if !func.type_params.is_empty() {
            return Ok(TypedValue::void(self.i64_type.const_int(0, false).into()));
        }

        // Infer param types from call sites: scan all call expressions
        // in already-compiled code for this function name
        let param_aha_types = self.infer_param_types(&func_name, &func.parameters);

        // Determine return type — reuse the pre-declared type if present
        // (set by predeclare_functions), otherwise infer it now.
        let return_type = self.fn_types.get(&func_name)
            .cloned()
            .unwrap_or_else(|| self.infer_function_return_type(func, &func_name));

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
            }

            let mut has_return = false;
            let mut last_value: BasicValueEnum<'ctx> = match &return_type {
                AhaType::String => self.string_type.const_zero().into(),
                AhaType::Struct(name) => {
                    self.struct_llvm_type(name)?.const_zero().into()
                }
                _ => self.i64_type.const_int(0, false).into(),
            };
            
            for stmt in &func.body.statements {
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
            }
            
            if !has_return {
                self.builder.build_return(Some(&last_value))
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
        
        self.scopes = saved_scopes;
        self.current_function = saved_function;
        if let Some(block) = saved_block {
            self.builder.position_at_end(block);
        }
        result?;
        Ok(TypedValue::void(self.i64_type.const_int(0, false).into()))
    }

    fn compile_call(&mut self, call: &ast::CallExpression) -> Result<TypedValue<'ctx>, String> {
        let func_name = match call.function.as_ref() {
            ast::Expression::Identifier(id) => id.value.clone(),
            _ => return Err("Can only call named functions".to_string()),
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
        let mut type_params: HashMap<String, AhaType> = HashMap::new();
        for (i, hint) in generic.param_type_hints.iter().enumerate() {
            if let Some(h) = hint {
                if generic.type_params.contains(h) && !type_params.contains_key(h) {
                    let t = arg_types.get(i).cloned().unwrap_or(AhaType::Int);
                    type_params.insert(h.clone(), t);
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
        for (i, p) in generic.parameters.iter().enumerate() {
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
            }

            let mut has_return = false;
            let mut last_value: BasicValueEnum<'ctx> = match &return_type {
                AhaType::String => self.string_type.const_zero().into(),
                AhaType::Struct(name) => {
                    self.struct_llvm_type(name)?.const_zero().into()
                }
                _ => self.i64_type.const_int(0, false).into(),
            };

            for stmt in &generic.body.statements {
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
            }

            if !has_return {
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
                let es_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[self.i64_type.const_int(0, false), self.i64_type.const_int(3, false)], "es_ptr") }
                    .map_err(|e| e.to_string())?;
                let data_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[self.i64_type.const_int(0, false), self.i64_type.const_int(0, false)], "data_ptr") }
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
            let es_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[self.i64_type.const_int(0, false), self.i64_type.const_int(3, false)], "es_ptr") }
                .map_err(|e| e.to_string())?;
            let data_ptr = unsafe { self.builder.build_gep(hdr_ptr, &[self.i64_type.const_int(0, false), self.i64_type.const_int(0, false)], "data_ptr") }
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
            if let ast::Statement::Struct(def) = stmt {
                let fields: Vec<(String, AhaType)> = def.fields.iter()
                    .map(|f| {
                        let t = f.type_hint.as_deref()
                            .and_then(AhaType::from_hint)
                            .unwrap_or(AhaType::Int);
                        (f.name.value.clone(), t)
                    })
                    .collect();
                self.struct_defs.insert(def.name.value.clone(), fields);
            }
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
}


