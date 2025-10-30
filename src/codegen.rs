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
            i64_type,
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
                    _ => Err(format!("Unknown operator: {}", infix.operator)),
                }
            },
            ast::Expression::If(if_expr) => self.compile_if_expression(if_expr),
            _ => Err("Expression type not yet implemented".to_string()),
        }
    }

    // Fungsi baru untuk kompilasi if expression
    fn compile_if_expression(&mut self, if_expr: &ast::IfExpression) -> Result<BasicValueEnum<'ctx>, String> {
    // 1. Kompilasi kondisi
    let condition_val = self.compile_expression(&if_expr.condition)?;
    
    // 2. Buat blok-blok yang dibutuhkan
    let function = self.builder.get_insert_block().expect("Error: Builder is not in a block!").get_parent().unwrap();
    let consequence_block = self.context.append_basic_block(function, "consequence");
    let alternative_block = self.context.append_basic_block(function, "alternative");
    let merge_block = self.context.append_basic_block(function, "merge");

    // 3. Buat instruksi lompatan bersyarat
    self.builder.build_conditional_branch(condition_val.into_int_value(), consequence_block, alternative_block);

    // 4. Kompilasi blok consequence
    self.builder.position_at_end(consequence_block);
    let consequence_val = self.compile_block_statement(&if_expr.consequence)?;
    self.builder.build_br(merge_block);

    // 5. Kompilasi blok alternative
    self.builder.position_at_end(alternative_block);
    let alternative_val = if let Some(alt_block) = &if_expr.alternative {
        self.compile_block_statement(alt_block)?
    } else {
        // Jika tidak ada else, gunakan nilai default (0)
        self.i64_type.const_int(0, false).into()
    };
    self.builder.build_br(merge_block);

    // 6. Kompilasi blok merge dengan PHI node
    self.builder.position_at_end(merge_block);
    let phi_node = self.builder.build_phi(self.i64_type, "iftmp");
    
    phi_node.add_incoming(&[(&consequence_val, consequence_block), (&alternative_val, alternative_block)]);
    
    // BARI INI ADALAH BARIS YANG BENAR:
    Ok(phi_node.as_basic_value())
}
    
    // Fungsi baru untuk kompilasi block statement
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

