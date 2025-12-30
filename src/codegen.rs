// src/codegen.rs

use crate::ast;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::values::{PointerValue, BasicValueEnum, FunctionValue};
use inkwell::types::IntType;
use std::collections::HashMap;

pub struct CodeGenerator<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    variables: HashMap<String, PointerValue<'ctx>>,
    functions: HashMap<String, FunctionValue<'ctx>>,  // NEW: store compiled functions
    i64_type: IntType<'ctx>,
    current_function: Option<FunctionValue<'ctx>>,    // NEW: track current function
}

impl<'ctx> CodeGenerator<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let module = context.create_module("aha_module");
        let builder = context.create_builder();
        let i64_type = context.i64_type();

        CodeGenerator {
            context,
            module,
            builder,
            variables: HashMap::new(),
            functions: HashMap::new(),
            i64_type,
            current_function: None,
        }
    }

    pub fn compile(&mut self, program: &ast::Program) -> Result<(), String> {
        let fn_type = self.i64_type.fn_type(&[], false);
        let function = self.module.add_function("main", fn_type, None);
        let basic_block = self.context.append_basic_block(function, "entry");

        self.builder.position_at_end(basic_block);

        for statement in &program.statements {
            self.compile_statement(statement)?;
        }

        if let Some(last_stmt) = program.statements.last() {
            if let ast::Statement::Expression(expr_stmt) = last_stmt {
                let return_val = self.compile_expression(&expr_stmt.expression)?;
                let _ = self.builder.build_return(Some(&return_val));
                return Ok(());
            }
        }
        
        let zero = self.i64_type.const_int(0, false);
        let _ = self.builder.build_return(Some(&zero));
        
        Ok(())
    }

    fn compile_statement(&mut self, statement: &ast::Statement) -> Result<(), String> {
        match statement {
            ast::Statement::Let(let_stmt) => {
                let value = self.compile_expression(&let_stmt.value)?;
                let pointer = self.builder.build_alloca(self.i64_type, &let_stmt.name.value)
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(pointer, value)
                    .map_err(|e| e.to_string())?;
                self.variables.insert(let_stmt.name.value.clone(), pointer);
            },
            ast::Statement::Expression(expr_stmt) => {
                self.compile_expression(&expr_stmt.expression)?;
            },
            ast::Statement::Return(ret_stmt) => {
                let return_val = self.compile_expression(&ret_stmt.return_value)?;
                self.builder.build_return(Some(&return_val))
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    fn compile_expression(&mut self, expression: &ast::Expression) -> Result<BasicValueEnum<'ctx>, String> {
        match expression {
            ast::Expression::Integer(int_lit) => {
                Ok(self.i64_type.const_int(int_lit.value as u64, false).into())
            },
            ast::Expression::Identifier(ident) => {
                if let Some(pointer) = self.variables.get(&ident.value) {
                    let loaded_val = self.builder.build_load(*pointer, &ident.value)
                        .map_err(|e| e.to_string())?;
                    Ok(loaded_val)
                } else {
                    Err(format!("Variable '{}' not found", ident.value))
                }
            },
            ast::Expression::Infix(infix) => {
                let left = self.compile_expression(&infix.left)?;
                let right = self.compile_expression(&infix.right)?;
                
                match infix.operator.as_str() {
                    "+" => Ok(self.builder.build_int_add(left.into_int_value(), right.into_int_value(), "addtmp")
                        .map_err(|e| e.to_string())?.into()),
                    "-" => Ok(self.builder.build_int_sub(left.into_int_value(), right.into_int_value(), "subtmp")
                        .map_err(|e| e.to_string())?.into()),
                    "*" => Ok(self.builder.build_int_mul(left.into_int_value(), right.into_int_value(), "multmp")
                        .map_err(|e| e.to_string())?.into()),
                    "/" => Ok(self.builder.build_int_signed_div(left.into_int_value(), right.into_int_value(), "divtmp")
                        .map_err(|e| e.to_string())?.into()),
                    "==" => {
                        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::EQ, left.into_int_value(), right.into_int_value(), "eqtmp")
                            .map_err(|e| e.to_string())?;
                        Ok(self.builder.build_int_z_extend(cmp, self.i64_type, "eqzext") // NAMA METHOD Y BENAR
                            .map_err(|e| e.to_string())?.into())
                    },
                    "!=" => {
                        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::NE, left.into_int_value(), right.into_int_value(), "netmp")
                            .map_err(|e| e.to_string())?;
                        Ok(self.builder.build_int_z_extend(cmp, self.i64_type, "nezext") // NAMA METHOD Y BENAR
                            .map_err(|e| e.to_string())?.into())
                    },
                    "<" => {
                        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::SLT, left.into_int_value(), right.into_int_value(), "lttmp")
                            .map_err(|e| e.to_string())?;
                        Ok(self.builder.build_int_z_extend(cmp, self.i64_type, "ltzext") // NAMA METHOD Y BENAR
                            .map_err(|e| e.to_string())?.into())
                    },
                    ">" => {
                        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::SGT, left.into_int_value(), right.into_int_value(), "gttmp")
                            .map_err(|e| e.to_string())?;
                        Ok(self.builder.build_int_z_extend(cmp, self.i64_type, "gtzext")
                            .map_err(|e| e.to_string())?.into())
                    },
                    "<=" => {
                        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::SLE, left.into_int_value(), right.into_int_value(), "letmp")
                            .map_err(|e| e.to_string())?;
                        Ok(self.builder.build_int_z_extend(cmp, self.i64_type, "lezext")
                            .map_err(|e| e.to_string())?.into())
                    },
                    ">=" => {
                        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::SGE, left.into_int_value(), right.into_int_value(), "getmp")
                            .map_err(|e| e.to_string())?;
                        Ok(self.builder.build_int_z_extend(cmp, self.i64_type, "gezext")
                            .map_err(|e| e.to_string())?.into())
                    },
                    _ => Err(format!("Unknown operator: {}", infix.operator)),
                }
            },
            ast::Expression::If(if_expr) => self.compile_if_expression(if_expr),
            ast::Expression::While(while_expr) => self.compile_while_expression(while_expr),
            ast::Expression::Boolean(bool_lit) => {
                let val = if bool_lit.value { 1 } else { 0 };
                Ok(self.i64_type.const_int(val, false).into())
            },
            ast::Expression::Function(func_lit) => self.compile_function(func_lit),
            ast::Expression::Call(call_expr) => self.compile_call(call_expr),
            _ => Err("Expression type not yet implemented".to_string()),
        }
    }

    // NEW: Compile function definition
    fn compile_function(&mut self, func: &ast::FunctionLiteral) -> Result<BasicValueEnum<'ctx>, String> {
        let func_name = func.name.as_ref()
            .map(|id| id.value.clone())
            .unwrap_or_else(|| format!("anonymous_{}", self.functions.len()));
        
        // Create function type based on parameter count
        let param_types: Vec<_> = func.parameters.iter()
            .map(|_| self.i64_type.into())
            .collect();
        let fn_type = self.i64_type.fn_type(&param_types, false);
        
        // Add function to module
        let function = self.module.add_function(&func_name, fn_type, None);
        self.functions.insert(func_name.clone(), function);
        
        // Save current state
        let saved_block = self.builder.get_insert_block();
        let saved_vars = self.variables.clone();
        let saved_function = self.current_function;
        
        // Set up function body
        let entry_block = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry_block);
        self.current_function = Some(function);
        self.variables.clear();
        
        // Bind parameters to variables
        for (i, param) in func.parameters.iter().enumerate() {
            let param_value = function.get_nth_param(i as u32)
                .ok_or("Failed to get parameter")?;
            let alloca = self.builder.build_alloca(self.i64_type, &param.value)
                .map_err(|e| e.to_string())?;
            self.builder.build_store(alloca, param_value)
                .map_err(|e| e.to_string())?;
            self.variables.insert(param.value.clone(), alloca);
        }
        
        // Compile function body
        let mut last_value = self.i64_type.const_int(0, false).into();
        for stmt in &func.body.statements {
            if let ast::Statement::Expression(expr_stmt) = stmt {
                last_value = self.compile_expression(&expr_stmt.expression)?;
            } else {
                self.compile_statement(stmt)?;
            }
        }
        
        // Return last expression value
        self.builder.build_return(Some(&last_value))
            .map_err(|e| e.to_string())?;
        
        // Restore state
        self.variables = saved_vars;
        self.current_function = saved_function;
        if let Some(block) = saved_block {
            self.builder.position_at_end(block);
        }
        
        // Return zero (function definitions don't produce a value in expression context)
        Ok(self.i64_type.const_int(0, false).into())
    }

    // NEW: Compile function call
    fn compile_call(&mut self, call: &ast::CallExpression) -> Result<BasicValueEnum<'ctx>, String> {
        // Get function name from expression
        let func_name = match call.function.as_ref() {
            ast::Expression::Identifier(id) => &id.value,
            _ => return Err("Can only call named functions".to_string()),
        };
        
        // Look up function
        let function = self.functions.get(func_name)
            .or_else(|| self.module.get_function(func_name).as_ref())
            .ok_or_else(|| format!("Unknown function: {}", func_name))?;
        
        // Compile arguments
        let mut args: Vec<BasicValueEnum> = Vec::new();
        for arg in &call.arguments {
            args.push(self.compile_expression(arg)?);
        }
        
        // Convert to BasicMetadataValueEnum
        let args_meta: Vec<_> = args.iter()
            .map(|a| (*a).into())
            .collect();
        
        // Build call
        let call_result = self.builder.build_call(*function, &args_meta, "calltmp")
            .map_err(|e| e.to_string())?;
        
        // Get return value
        call_result.try_as_basic_value()
            .left()
            .ok_or_else(|| "Function call did not return a value".to_string())
    }

    // NEW: Compile while loop
    fn compile_while_expression(&mut self, while_expr: &ast::WhileExpression) -> Result<BasicValueEnum<'ctx>, String> {
        let function = self.builder.get_insert_block()
            .expect("Builder not in a block!")
            .get_parent()
            .unwrap();
        
        // Create blocks for while loop
        let condition_block = self.context.append_basic_block(function, "while_cond");
        let body_block = self.context.append_basic_block(function, "while_body");
        let after_block = self.context.append_basic_block(function, "while_after");
        
        // Jump to condition block
        self.builder.build_unconditional_branch(condition_block)
            .map_err(|e| e.to_string())?;
        
        // Build condition block
        self.builder.position_at_end(condition_block);
        let condition_val = self.compile_expression(&while_expr.condition)?;
        
        // Convert to i1 for branch (non-zero = true)
        let condition_bool = self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            condition_val.into_int_value(),
            self.i64_type.const_int(0, false),
            "while_cond_bool"
        ).map_err(|e| e.to_string())?;
        
        self.builder.build_conditional_branch(condition_bool, body_block, after_block)
            .map_err(|e| e.to_string())?;
        
        // Build body block
        self.builder.position_at_end(body_block);
        self.compile_block_statement(&while_expr.body)?;
        
        // Jump back to condition
        self.builder.build_unconditional_branch(condition_block)
            .map_err(|e| e.to_string())?;
        
        // Continue after loop
        self.builder.position_at_end(after_block);
        
        // While loops return 0 (unit type)
        Ok(self.i64_type.const_int(0, false).into())
    }

    fn compile_if_expression(&mut self, if_expr: &ast::IfExpression) -> Result<BasicValueEnum<'ctx>, String> {
        let condition_val = self.compile_expression(&if_expr.condition)?;
        
        let function = self.builder.get_insert_block().expect("Error: Builder is not in a block!").get_parent().unwrap();
        let consequence_block = self.context.append_basic_block(function, "consequence");
        let alternative_block = self.context.append_basic_block(function, "alternative");
        let merge_block = self.context.append_basic_block(function, "merge");

        self.builder.build_conditional_branch(condition_val.into_int_value(), consequence_block, alternative_block)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(consequence_block);
        let consequence_val = self.compile_block_statement(&if_expr.consequence)?;
        self.builder.build_unconditional_branch(merge_block)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(alternative_block);
        let alternative_val = if let Some(alt_block) = &if_expr.alternative {
            self.compile_block_statement(alt_block)?
        } else {
            self.i64_type.const_int(0, false).into()
        };
        self.builder.build_unconditional_branch(merge_block)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(merge_block);
        let phi_node = self.builder.build_phi(self.i64_type, "iftmp")
            .map_err(|e| e.to_string())?;
        
        phi_node.add_incoming(&[(&consequence_val, consequence_block), (&alternative_val, alternative_block)]);
        
        Ok(phi_node.as_basic_value())
    }
    
    fn compile_block_statement(&mut self, block: &ast::BlockStatement) -> Result<BasicValueEnum<'ctx>, String> {
        let mut last_value = self.i64_type.const_int(0, false).into();
        for statement in &block.statements {
            if let ast::Statement::Expression(expr_stmt) = statement {
                last_value = self.compile_expression(&expr_stmt.expression)?;
            } else {
                self.compile_statement(statement)?;
            }
        }
        Ok(last_value)
    }

    pub fn print_llvm_ir(&self) {
        self.module.print_to_stderr();
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




