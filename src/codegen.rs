// src/codegen.rs

use crate::ast;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::values::{IntValue, PointerValue, BasicValueEnum};
use inkwell::types::IntType;
use std::collections::HashMap;

pub struct CodeGenerator<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    // Symbol table untuk melacak variabel
    variables: HashMap<String, PointerValue<'ctx>>,
    // Tipe data integer utama kita (i64)
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

    // Fungsi utama untuk mengompilasi seluruh program
    // (tetap Result<(), String> sesuai instruksi)
    pub fn compile(&mut self, program: &ast::Program) -> Result<(), String> {
        // Buat fungsi `main` implisit
        let fn_type = self.i64_type.fn_type(&[], false);
        let function = self.module.add_function("main", fn_type, None);
        let basic_block = self.context.append_basic_block(function, "entry");

        self.builder.position_at_end(basic_block);

        // Kompilasi setiap pernyataan dalam program
        for statement in &program.statements {
            self.compile_statement(statement)?;
        }

        // Kembalikan nilai ekspresi terakhir jika ada
        if let Some(last_stmt) = program.statements.last() {
            if let ast::Statement::Expression(expr_stmt) = last_stmt {
                let return_val = self.compile_expression(&expr_stmt.expression)?;
                self.builder.build_return(Some(&return_val));
                return Ok(());
            }
        }
        
        // Jika tidak ada ekspresi, kembalikan 0
        let zero = self.i64_type.const_int(0, false);
        self.builder.build_return(Some(&zero));
        
        Ok(())
    }

    // Tambah propagasi error Result di statement
    fn compile_statement(&mut self, statement: &ast::Statement) -> Result<(), String> {
        match statement {
            ast::Statement::Let(let_stmt) => {
                // Kompilasi nilai di sebelah kanan
                let value = self.compile_expression(&let_stmt.value)?;
                
                // Alokasikan ruang di stack untuk variabel (pakai ? sesuai instruksi)
                let pointer = self.builder.build_alloca(self.i64_type, &let_stmt.name.value)?;
                
                // Simpan nilai ke dalam alokasi tersebut (pakai ? sesuai instruksi)
                self.builder.build_store(pointer, value)?;
                
                // Simpan pointer ke symbol table
                self.variables.insert(let_stmt.name.value.clone(), pointer);
            },
            ast::Statement::Expression(expr_stmt) => {
                // Kompilasi ekspresi, tapi abaikan hasilnya untuk saat ini
                self.compile_expression(&expr_stmt.expression)?;
            },
            ast::Statement::Return(_) => {
                // TODO: Implementasi return
                return Err("Return statement not yet implemented".to_string());
            }
        }
        Ok(())
    }

    // Tambah Result<BasicValueEnum<'ctx>, String> + ? di builder aritmatika
    fn compile_expression(&mut self, expression: &ast::Expression) -> Result<BasicValueEnum<'ctx>, String> {
        match expression {
            ast::Expression::Integer(int_lit) => {
                Ok(self.i64_type.const_int(int_lit.value as u64, false).into())
            },
            ast::Expression::Identifier(ident) => {
                // Cari variabel di symbol table
                if let Some(pointer) = self.variables.get(&ident.value) {
                    // Muat nilai dari memori
                    Ok(self.builder.build_load(*pointer, &ident.value))
                } else {
                    Err(format!("Variable '{}' not found", ident.value))
                }
            },
            ast::Expression::Infix(infix) => {
                // Kompilasi sisi kiri dan kanan secara rekursif
                let left = self.compile_expression(&infix.left)?;
                let right = self.compile_expression(&infix.right)?;
                
                // Generate instruksi LLVM berdasarkan operator (pakai ? sesuai instruksi)
                match infix.operator.as_str() {
                    "+" => Ok(self.builder.build_int_add(left.into_int_value(), right.into_int_value(), "addtmp")?.into()),
                    "-" => Ok(self.builder.build_int_sub(left.into_int_value(), right.into_int_value(), "subtmp")?.into()),
                    "*" => Ok(self.builder.build_int_mul(left.into_int_value(), right.into_int_value(), "multmp")?.into()),
                    "/" => Ok(self.builder.build_int_signed_div(left.into_int_value(), right.into_int_value(), "divtmp")?.into()),
                    // TODO: Tambahkan operator lainnya
                    _ => Err(format!("Unknown operator: {}", infix.operator)),
                }
            },
            // TODO: Implementasi tipe ekspresi lainnya (Boolean, Prefix, If, dll.)
            _ => Err("Expression type not yet implemented".to_string()),
        }
    }

    // Fungsi untuk mencetak LLVM IR yang dihasilkan (untuk debugging)
    pub fn print_llvm_ir(&self) {
        self.module.print_to_stderr();
    }
    
    // Fungsi untuk menjalankan kode dengan JIT (Just-In-Time) compiler
    pub fn run_jit(&self) -> Result<i64, String> {
        let execution_engine = self.module.create_jit_execution_engine(inkwell::OptimizationLevel::None)
            .map_err(|e| format!("Failed to create JIT engine: {}", e))?;
            
        let function_name = "main";
        let function = self.module.get_function(function_name)
            .ok_or_else(|| format!("Function '{}' not found", function_name))?;
            
        unsafe {
            let compiled_fn: unsafe extern "C" fn() -> i64 = execution_engine.get_function_address(function_name)
                .map_err(|e| format!("Failed to get function address: {}", e))
                .map(|addr| std::mem::transmute(addr))?;
                
            Ok(compiled_fn())
        }
    }
}
