use std::rc::Rc;

use calvin_core::context::TypeContext;
use calvin_core::lang::expr::{Expr, ExprVisitor};
use calvin_core::lang::typeclass::TypeClassRegistry;
use calvin_core::lang::typeinf::{TypeEnv, TypeInference};
use calvin_core::lang::types::{MonoType, Prim};
use calvin_parse::lexer::Token;
use calvin_parse::parser::expr_parser;
use chumsky::input::{Input, Stream};
use chumsky::Parser;

use calvin_codegen_cranelift::jit::JITCompiler as CraneliftJIT;
use calvin_codegen_llvm::jit::LLVMCompiler;
use inkwell::context::Context;

#[derive(Clone)]
pub enum BackendChoice {
    Cranelift,
    Llvm,
}

fn format_type_error(err: &calvin_core::lang::typeinf::TypeError, src: &str) -> String {
    match err {
        calvin_core::lang::typeinf::TypeError::TypeMismatch => "Type error: TypeMismatch".to_string(),
        calvin_core::lang::typeinf::TypeError::OccursCheckFailed => "Type error: OccursCheckFailed".to_string(),
        calvin_core::lang::typeinf::TypeError::UnboundVariable(v) => format!("Type error: UnboundVariable({})", v),
        calvin_core::lang::typeinf::TypeError::UnsatisfiableConstraint { class_name, args, explanation } => {
            let cst_str = format!("{} {}", class_name, args.join(" "));
            let span_end = src.trim().len();
            let mut out = format!("stdin:1,1-{}: Constraint not satisfiable: {}\n1 {}", span_end, cst_str, src.trim());
            if let Some(expl) = explanation {
                out.push('\n');
                out.push_str(expl);
            }
            out
        }
    }
}

fn format_unresolved_constraints_error<'a>(
    residuals: &[(&str, Vec<&'a calvin_core::lang::types::MonoType<'a>>)],
) -> String {
    let mut names = std::collections::HashMap::new();
    let mut var_idx = 0;
    for (_, cargs) in residuals {
        for arg in cargs {
            let mut set = std::collections::HashSet::new();
            arg.free_tvars(&mut set);
            for id in set {
                if !names.contains_key(&id) {
                    let name = if var_idx < 26 {
                        ((b'a' + var_idx as u8) as char).to_string()
                    } else {
                        format!("t{}", var_idx - 26)
                    };
                    names.insert(id, name);
                    var_idx += 1;
                }
            }
        }
    }
    let cst_strs: Vec<String> = residuals
        .iter()
        .map(|(name, args)| {
            let args_str = args
                .iter()
                .map(|a| calvin_core::lang::types::format_mono(a, &names))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} {}", name, args_str)
        })
        .collect();

    format!(
        "Failed to compile expression due to unresolved type constraint{}: {}",
        if cst_strs.len() > 1 { "s" } else { "" },
        cst_strs.join(", ")
    )
}

pub struct Compiler<'ctx> {
    ctx: &'ctx TypeContext,
    cranelift_jit: Option<CraneliftJIT>,
    llvm_jit: Option<LLVMCompiler<'static>>,
    base_env: Rc<TypeEnv<'ctx>>,
    classes: Rc<TypeClassRegistry<'ctx>>,
}

impl<'ctx> Compiler<'ctx> {
    pub fn new(ctx: &'ctx TypeContext, backend: BackendChoice) -> Self {
        let (base_env, classes) = calvin_boot::init_bootstrap(ctx)
            .expect("Failed to initialize bootstrap environment from boot/*.hob");
        match backend {
            BackendChoice::Cranelift => Self {
                ctx,
                cranelift_jit: Some(CraneliftJIT::new()),
                llvm_jit: None,
                base_env,
                classes,
            },
            BackendChoice::Llvm => {
                let llvm_ctx: &'static Context = Box::leak(Box::new(Context::create()));
                Self {
                    ctx,
                    cranelift_jit: None,
                    llvm_jit: Some(LLVMCompiler::new(llvm_ctx)),
                    base_env,
                    classes,
                }
            }
        }
    }

    fn make_type_inf(&self) -> TypeInference<'ctx> {
        TypeInference::with_env_and_classes(self.ctx, self.base_env.clone(), self.classes.clone())
    }

    fn parse_internal(&self, src: &str) -> Result<&'ctx Expr<'ctx>, String> {
        use logos::Logos;

        // Allocate the string onto the bump arena so its lifetime becomes 'ctx.
        // This is necessary because the REPL loop provides short-lived strings,
        // but the AST nodes keep references to the source code substrings.
        let src: &'ctx str = self.ctx.arena().alloc_str(src);

        let token_iter = Token::lexer(src).spanned().map(|(tok, span)| match tok {
            Ok(t) => Ok((t, span)),
            Err(e) => Err((e, span)),
        });

        let mut tokens = Vec::new();
        for t in token_iter {
            tokens.push(t.map_err(|(e, span)| format!("Lex error at {:?}: {:?}", span, e))?);
        }

        let eof = src.len()..src.len();
        let token_stream = Stream::from_iter(tokens).map(eof, |(t, s)| (t, s));

        let parser = expr_parser(self.ctx);
        match parser.parse(token_stream).into_result() {
            Ok(e) => Ok(e),
            Err(errs) => Err(format!("Parse error: {:?}", errs)),
        }
    }

    pub fn parse(&self, src: &str) -> Result<String, String> {
        let expr = self.parse_internal(src)?;
        calvin_core::lang::unsweeten::unsweeten(self.ctx, expr)
    }

    pub fn type_of(&mut self, src: &str) -> Result<String, String> {
        let expr = self.parse_internal(src)?;
        let mut type_inf = self.make_type_inf();
        let ty = type_inf
            .visit(expr)
            .map_err(|e| format_type_error(&e, src))?;
            
        type_inf.solve_constraints().map_err(|e| format_type_error(&e, src))?;

        let residuals = type_inf.residual_constraints();
        Ok(calvin_core::lang::types::format_qual_type(ty, &residuals))
    }

    pub fn dump_ir(&mut self, src: &str) -> Result<String, String> {
        let expr = self.parse_internal(src)?;
        let mut type_inf = self.make_type_inf();
        let ty = type_inf
            .visit(expr)
            .map_err(|e| format_type_error(&e, src))?;
            
        type_inf.solve_constraints().map_err(|e| format_type_error(&e, src))?;

        let residuals = type_inf.residual_constraints();
        if !residuals.is_empty() {
            return Err(format_unresolved_constraints_error(&residuals));
        }

        if let Some(ref mut jit) = self.cranelift_jit {
            Ok(jit.compile_and_dump_ir(expr, ty))
        } else if let Some(ref jit) = self.llvm_jit {
            Ok(jit.compile_and_dump_ir(expr, ty))
        } else {
            Err("No JIT backend initialized".to_string())
        }
    }

    pub fn disassemble(&mut self, src: &str) -> Result<String, String> {
        let expr = self.parse_internal(src)?;
        let mut type_inf = self.make_type_inf();
        let ty = type_inf
            .visit(expr)
            .map_err(|e| format_type_error(&e, src))?;
            
        type_inf.solve_constraints().map_err(|e| format_type_error(&e, src))?;

        let residuals = type_inf.residual_constraints();
        if !residuals.is_empty() {
            return Err(format_unresolved_constraints_error(&residuals));
        }

        let func_ptr = if let Some(ref mut jit) = self.cranelift_jit {
            match ty.chase() {
                MonoType::Prim(Prim::Double | Prim::Float) => {
                    jit.compile_expr::<f64>(expr, ty) as *const u8
                }
                _ => jit.compile_expr::<i64>(expr, ty) as *const u8,
            }
        } else if let Some(ref jit) = self.llvm_jit {
            match ty.chase() {
                MonoType::Prim(Prim::Double | Prim::Float) => {
                    let jit_fn = jit.compile_expr::<f64>(expr, ty);
                    unsafe { jit_fn.as_raw() as *const u8 }
                }
                _ => {
                    let jit_fn = jit.compile_expr::<i64>(expr, ty);
                    unsafe { jit_fn.as_raw() as *const u8 }
                }
            }
        } else {
            return Err("Backend not initialized".to_string());
        };
        let ptr = func_ptr;

        // Read 64 bytes for disassembly scaffold
        let code = unsafe { std::slice::from_raw_parts(ptr, 64) };

        use capstone::prelude::*;
        let cs = Capstone::new()
            .x86()
            .mode(arch::x86::ArchMode::Mode64)
            .syntax(arch::x86::ArchSyntax::Intel)
            .detail(true)
            .build()
            .map_err(|e| format!("Capstone error: {:?}", e))?;

        let insns = cs
            .disasm_all(code, ptr as u64)
            .map_err(|e| format!("Disasm error: {:?}", e))?;

        let mut out = String::new();
        for i in insns.as_ref() {
            out.push_str(&format!(
                "0x{:x}:\t{}\t{}\n",
                i.address(),
                i.mnemonic().unwrap_or(""),
                i.op_str().unwrap_or("")
            ));
            if i.mnemonic() == Some("ret") {
                break;
            } // Stop at first return
        }
        Ok(out)
    }

    pub fn eval_dynamic(&mut self, src: &str) -> Result<String, String> {
        let expr = self.parse_internal(src)?;
        let mut type_inf = self.make_type_inf();
        let ty = type_inf
            .visit(expr)
            .map_err(|e| format_type_error(&e, src))?;
            
        type_inf.solve_constraints().map_err(|e| format_type_error(&e, src))?;

        let residuals = type_inf.residual_constraints();
        if !residuals.is_empty() {
            return Err(format_unresolved_constraints_error(&residuals));
        }

        if let MonoType::Fn(_, _) = ty.chase() {
            return Ok("<closure>".to_string());
        }

        if let Some(ref mut jit) = self.cranelift_jit {
            match ty.chase() {
                MonoType::Prim(Prim::Double | Prim::Float) => {
                    let func_ptr = jit.compile_expr::<f64>(expr, ty);
                    let res = func_ptr();
                    Ok(unsafe { calvin_core::runtime::value::format_runtime_value(res.to_bits(), ty) })
                }
                _ => {
                    let func_ptr = jit.compile_expr::<i64>(expr, ty);
                    let res = func_ptr();
                    Ok(unsafe { calvin_core::runtime::value::format_runtime_value(res as u64, ty) })
                }
            }
        } else if let Some(ref jit) = self.llvm_jit {
            match ty.chase() {
                MonoType::Prim(Prim::Double | Prim::Float) => {
                    let jit_fn = jit.compile_expr::<f64>(expr, ty);
                    let res = unsafe { jit_fn.call() };
                    Ok(unsafe { calvin_core::runtime::value::format_runtime_value(res.to_bits(), ty) })
                }
                _ => {
                    let jit_fn = jit.compile_expr::<i64>(expr, ty);
                    let res = unsafe { jit_fn.call() };
                    Ok(unsafe { calvin_core::runtime::value::format_runtime_value(res as u64, ty) })
                }
            }
        } else {
            Err("Backend not initialized".to_string())
        }
    }
}
