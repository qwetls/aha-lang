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
    current_function: Option<FunctionValue<'ctx>>,
    /// Stack of (continue_block, break_block) for nested loops
    loop_stack: Vec<(inkwell::basic_block::BasicBlock<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)>,
    /// Inferred parameter types per function: func_name → vec of AhaType
    param_type_map: HashMap<String, Vec<AhaType>>,
}

impl<'ctx> CodeGenerator<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let module = context.create_module("aha_module");
        let builder = context.create_builder();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(inkwell::AddressSpace::default());
        // String = {i8*, i64}
        let string_type = context.struct_type(&[i8_ptr_type.into(), i64_type.into()], false);

        CodeGenerator {
            context,
            module,
            builder,
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            fn_types: HashMap::new(),
            i64_type,
            string_type,
            current_function: None,
            loop_stack: Vec::new(),
            param_type_map: HashMap::new(),
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
                                if i < existing.len() && matches!(t, AhaType::String) {
                                    existing[i] = AhaType::String;
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
                self.scan_expr_for_calls(&assign.value);
            }
            ast::Expression::Function(func) => {
                self.scan_block_for_calls(&func.body);
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
            _ => {}
        }
    }

    fn scan_block_for_calls(&mut self, block: &ast::BlockStatement) {
        for stmt in &block.statements {
            self.scan_call_sites(std::slice::from_ref(stmt));
        }
    }

    /// Pre-declare all user functions so forward references work.
    /// Creates the LLVM function value with correct param types but
    /// does NOT compile the body — bodies are compiled later.
    fn predeclare_functions(&mut self, statements: &[ast::Statement]) {
        for stmt in statements {
            if let ast::Statement::Expression(ast::ExpressionStatement {
                expression: ast::Expression::Function(func),
            }) = stmt
            {
                if let Some(name) = &func.name {
                    let func_name = name.value.clone();
                    if self.functions.contains_key(&func_name) {
                        continue;
                    }
                    let param_types: Vec<_> = func.parameters.iter()
                        .map(|p| {
                            let inferred = self.param_type_map.get(&func_name);
                            match inferred {
                                Some(types) => {
                                    let idx = func.parameters.iter().position(|param| param.value == p.value).unwrap_or(0);
                                    if idx < types.len() && matches!(types[idx], AhaType::String) {
                                        self.string_type.into()
                                    } else {
                                        self.i64_type.into()
                                    }
                                }
                                None => self.i64_type.into(),
                            }
                        })
                        .collect();
                    let fn_type = self.i64_type.fn_type(&param_types, false);
                    let function = self.module.add_function(&func_name, fn_type, None);
                    self.functions.insert(func_name.clone(), function);
                    self.fn_types.insert(func_name, AhaType::Int);
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
                self.lookup_variable(&id.value)
                    .map(|info| info.var_type.clone())
                    .unwrap_or(AhaType::Int)
            }
            ast::Expression::Infix(infix) => {
                let lt = self.infer_expr_type(&infix.left);
                let rt = self.infer_expr_type(&infix.right);
                match infix.operator.as_str() {
                    "+" if lt == AhaType::String || rt == AhaType::String => AhaType::String,
                    "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" => AhaType::Bool,
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
                    if let Some(types) = self.param_type_map.get(&id.value) {
                        if let Some(rt) = types.last() {
                            return rt.clone();
                        }
                    }
                    if id.value == "len" {
                        return AhaType::Int;
                    }
                }
                AhaType::Int
            }
            _ => AhaType::Int,
        }
    }

    /// Get i8* pointer type (used frequently for strings)
    fn i8_ptr_type(&self) -> inkwell::types::PointerType<'ctx> {
        self.context.i8_type().ptr_type(inkwell::AddressSpace::default())
    }

    pub fn compile(&mut self, program: &ast::Program) -> Result<(), String> {
        self.declare_printf();
        self.declare_c_runtime();

        // Pre-pass: scan all statements for call expressions to infer
        // parameter types. This lets us type function params as String
        // when a string is passed at a call site.
        self.scan_call_sites(&program.statements);

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

    fn compile_statement(&mut self, statement: &ast::Statement) -> Result<(), String> {
        match statement {
            ast::Statement::Let(let_stmt) => {
                let typed_val = self.compile_expression(&let_stmt.value)?;
                let alloc_type: inkwell::types::BasicTypeEnum<'ctx> = match &typed_val.aha_type {
                    AhaType::String => self.string_type.into(),
                    _ => self.i64_type.into(),
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

    /// Declare C runtime functions (malloc, strlen, memcpy, strcmp, sprintf)
    fn declare_c_runtime(&mut self) {
        let i8_ptr = self.i8_ptr_type();
        let i64_t = self.i64_type;
        // malloc
        let malloc_ty = i8_ptr.fn_type(&[i64_t.into()], false);
        let malloc_fn = self.module.add_function("malloc", malloc_ty, None);
        self.functions.insert("malloc".to_string(), malloc_fn);
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
        Ok(TypedValue::bool_val(ext.into()))
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
        Ok(TypedValue::bool_val(ext.into()))
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
    fn compile_function(&mut self, func: &ast::FunctionLiteral) -> Result<TypedValue<'ctx>, String> {
        let func_name = func.name.as_ref()
            .map(|id| id.value.clone())
            .unwrap_or_else(|| format!("anonymous_{}", self.functions.len()));

        // Infer param types from call sites: scan all call expressions
        // in already-compiled code for this function name
        let param_aha_types = self.infer_param_types(&func_name, &func.parameters);

        // Reuse pre-declared function if it exists (for forward references)
        let function = if let Some(f) = self.functions.get(&func_name) {
            *f
        } else {
            let param_types: Vec<_> = param_aha_types.iter()
                .map(|t| match t {
                    AhaType::String => self.string_type.into(),
                    _ => self.i64_type.into(),
                })
                .collect();
            let fn_type = self.i64_type.fn_type(&param_types, false);
            let function = self.module.add_function(&func_name, fn_type, None);
            self.functions.insert(func_name.clone(), function);
            function
        };
        self.fn_types.insert(func_name.clone(), AhaType::Int);

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
                let alloc_type: inkwell::types::BasicTypeEnum<'ctx> = match aha_type {
                    AhaType::String => self.string_type.into(),
                    _ => self.i64_type.into(),
                };
                let alloca = self.builder.build_alloca(alloc_type, &param.value)
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(alloca, param_value)
                    .map_err(|e| e.to_string())?;
                self.insert_variable(param.value.clone(), alloca, aha_type.clone());
            }
            
            let mut has_return = false;
            let mut last_value: BasicValueEnum<'ctx> = self.i64_type.const_int(0, false).into();
            
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

        let phi_node = self.builder.build_phi(self.i64_type, "iftmp")
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
        if let Some(info) = self.lookup_variable(&assign.name.value) {
            let ptr = info.ptr;
            self.builder.build_store(ptr, typed_val.value)
                .map_err(|e| e.to_string())?;
            Ok(typed_val)
        } else {
            Err(format!("Cannot assign to undefined variable: '{}'", assign.name.value))
        }
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
        let after_block = self.context.append_basic_block(function, "for_after");
        self.builder.build_unconditional_branch(cond_block).map_err(|e| e.to_string())?;

        self.builder.position_at_end(cond_block);
        let current_val = self.builder.build_load(loop_var_ptr, "loop_var").map_err(|e| e.to_string())?;
        let cond = self.builder.build_int_compare(
            inkwell::IntPredicate::SLT, current_val.into_int_value(),
            end_val.value.into_int_value(), "for_cond"
        ).map_err(|e| e.to_string())?;
        self.builder.build_conditional_branch(cond, body_block, after_block).map_err(|e| e.to_string())?;

        self.builder.position_at_end(body_block);
        self.loop_stack.push((cond_block, after_block));
        self.compile_block_statement(&for_expr.body)?;
        self.loop_stack.pop();

        let body_end = self.builder.get_insert_block().unwrap();
        if body_end.get_terminator().is_none() {
            let current = self.builder.build_load(loop_var_ptr, "cur").map_err(|e| e.to_string())?;
            let next = self.builder.build_int_add(current.into_int_value(), self.i64_type.const_int(1, false), "next")
                .map_err(|e| e.to_string())?;
            self.builder.build_store(loop_var_ptr, next).map_err(|e| e.to_string())?;
            self.builder.build_unconditional_branch(cond_block).map_err(|e| e.to_string())?;
        }

        self.builder.position_at_end(after_block);
        Ok(TypedValue::void(self.i64_type.const_int(0, false).into()))
    }

    fn compile_range_expression(&mut self, range: &ast::RangeExpression) -> Result<TypedValue<'ctx>, String> {
        self.compile_expression(&range.start)
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


