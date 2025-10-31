// src/codegen.rs

use crate::ast;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::values::{PointerValue, BasicValueEnum};
use inkwell::types::IntType;
use std::collections::HashMap;

pub struct CodeGenerator<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    variables: HashMap<String, PointerValue<'ctx>>,
    i64_type: IntType<'ctx>,
    bool_type: IntType<'ctx>, // TAMBAHKAN INI
}

impl<'ctx> CodeGenerator<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let module = context.create_module("aha_module");
        let builder = context.create_builder();
        let i64_type = context.i64_type();
        let bool_type = context.bool_type(); // INISIASI INI

        CodeGenerator {
            context,
            module,
            builder,
            variables: HashMap::new(),
            i64_type,
            bool_type, // TAMBAHKAN INI
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
                
                // Handle boolean return value - convert to i64 if needed
                let return_val = if return_val.is_int_value() && return_val.into_int_value().get_type() == self.bool_type {
                    self.builder.build_int_z_extend(return_val.into_int_value(), self.i64_type, "bool_extend")
                        .map_err(|e| e.to_string())?
                        .into()
                } else {
                    return_val
                };
                
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
                
                // Determine the type for allocation based on the value type
                let alloca_type = if value.is_int_value() && value.into_int_value().get_type() == self.bool_type {
                    self.bool_type
                } else {
                    self.i64_type
                };
                
                let pointer = self.builder.build_alloca(alloca_type, &let_stmt.name.value)
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(pointer, value)
                    .map_err(|e| e.to_string())?;
                self.variables.insert(let_stmt.name.value.clone(), pointer);
            },
            ast::Statement::Expression(expr_stmt) => {
                self.compile_expression(&expr_stmt.expression)?;
            },
            ast::Statement::Return(_) => {
                return Err("Return statement not yet implemented".to_string());
            }
        }
        Ok(())
    }

    fn compile_expression(&mut self, expression: &ast::Expression) -> Result<BasicValueEnum<'ctx>, String> {
        match expression {
            ast::Expression::Integer(int_lit) => {
                Ok(self.i64_type.const_int(int_lit.value as u64, false).into())
            },
            ast::Expression::Boolean(bool_lit) => {
                // KOMPILASI BOOLEAN LANGSUNG KE i1
                Ok(self.bool_type.const_int(bool_lit.value as u64, false).into())
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
                        // JANGAN UBAH KE i64 LAGI. KEMBALIKAN LANGSUNG HASIL BOOLEAN (i1)
                        Ok(cmp.into())
                    },
                    "!=" => {
                        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::NE, left.into_int_value(), right.into_int_value(), "netmp")
                            .map_err(|e| e.to_string())?;
                        Ok(cmp.into())
                    },
                    "<" => {
                        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::SLT, left.into_int_value(), right.into_int_value(), "lttmp")
                            .map_err(|e| e.to_string())?;
                        Ok(cmp.into())
                    },
                    ">" => {
                        let cmp = self.builder.build_int_compare(inkwell::IntPredicate::SGT, left.into_int_value(), right.into_int_value(), "gttmp")
                            .map_err(|e| e.to_string())?;
                        Ok(cmp.into())
                    },
                    _ => Err(format!("Unknown operator: {}", infix.operator)),
                }
            },
            ast::Expression::If(if_expr) => self.compile_if_expression(if_expr),
            ast::Expression::Function(fn_lit) => self.compile_function_literal(fn_lit),
            ast::Expression::Call(call_expr) => self.compile_call_expression(call_expr),
            _ => Err("Expression type not yet implemented".to_string()),
        }
    }

    // Fungsi baru untuk kompilasi definisi fungsi
    fn compile_function_literal(&mut self, fn_lit: &ast::FunctionLiteral) -> Result<BasicValueEnum<'ctx>, String> {
        // TODO: Implementasi pembuatan fungsi LLVM
        // Ini adalah bagian yang paling kompleks. Untuk saat ini, kita kembalikan error
        // agar kita bisa fokus pada pemanggilan fungsi terlebih dahulu.
        Err("Function literals as values (closures) are not yet implemented".to_string())
    }

    // Fungsi baru untuk kompilasi pemanggilan fungsi
    fn compile_call_expression(&mut self, call_expr: &ast::CallExpression) -> Result<BasicValueEnum<'ctx>, String> {
        // Compile ekspresi fungsi untuk mendapatkan nama atau nilai fungsi
        let function_value = self.compile_expression(&call_expr.function)?;
        
        // Untuk saat ini, kita asumsikan fungsi adalah identifier global
        if let ast::Expression::Identifier(ident) = &*call_expr.function {
            let function_name = &ident.value;
            let function = self.module.get_function(function_name)
                .ok_or_else(|| format!("Function '{}' not found", function_name))?;

            // Compile argumen
            let mut args = Vec::new();
            for arg in &call_expr.arguments {
                args.push(self.compile_expression(arg)?);
            }
            
            // Buat pemanggilan fungsi
            let call_site_value = self.builder.build_call(function, &args, "calltmp")
                .map_err(|e| e.to_string())?
                .try_as_basic_value()
                .left()
                .ok_or_else(|| "Failed to get return value from function call")?;
            
            Ok(call_site_value)
        } else {
            Err("Calling non-identifier functions is not yet supported".to_string())
        }
    }

    fn compile_if_expression(&mut self, if_expr: &ast::IfExpression) -> Result<BasicValueEnum<'ctx>, String> {
        let condition_val = self.compile_expression(&if_expr.condition)?;
        
        // Ensure condition is a boolean (i1)
        let condition_bool = if condition_val.is_int_value() && condition_val.into_int_value().get_type() == self.i64_type {
            // Convert i64 to i1 for condition
            let zero = self.i64_type.const_int(0, false);
            self.builder.build_int_compare(inkwell::IntPredicate::NE, condition_val.into_int_value(), zero, "bool_cond")
                .map_err(|e| e.to_string())?
        } else {
            condition_val.into_int_value()
        };
        
        let function = self.builder.get_insert_block().expect("Error: Builder is not in a block!").get_parent().unwrap();
        let consequence_block = self.context.append_basic_block(function, "consequence");
        let alternative_block = self.context.append_basic_block(function, "alternative");
        let merge_block = self.context.append_basic_block(function, "merge");

        self.builder.build_conditional_branch(condition_bool, consequence_block, alternative_block)
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
        
        // Handle phi node with proper type inference
        let consequence_type = consequence_val.get_type();
        let alternative_type = alternative_val.get_type();
        
        let phi_type = if consequence_type == alternative_type {
            consequence_type.into_int_type()
        } else {
            // Default to i64 if types differ
            self.i64_type
        };
        
        let phi_node = self.builder.build_phi(phi_type, "iftmp")
            .map_err(|e| e.to_string())?;
        
        // Convert values to phi type if needed
        let consequence_for_phi = if consequence_val.get_type().into_int_type() != phi_type {
            self.builder.build_int_z_extend(consequence_val.into_int_value(), phi_type, "conv_cons")
                .map_err(|e| e.to_string())?
                .into()
        } else {
            consequence_val
        };
        
        let alternative_for_phi = if alternative_val.get_type().into_int_type() != phi_type {
            self.builder.build_int_z_extend(alternative_val.into_int_value(), phi_type, "conv_alt")
                .map_err(|e| e.to_string())?
                .into()
        } else {
            alternative_val
        };
        
        phi_node.add_incoming(&[(&consequence_for_phi, consequence_block), (&alternative_for_phi, alternative_block)]);
        
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