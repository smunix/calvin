use std::collections::HashMap;
use std::mem;
use std::rc::Rc;

use cranelift_codegen::ir::{self, InstBuilder};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use crate::types::lower_type;
use calvin_core::lang::expr::{Expr, ExprVisitor, Literal, Pattern};
use calvin_core::lang::types::MonoType;

pub struct JITCompiler<'ctx> {
    module: JITModule,
    ctx: Context,
    builder_ctx: cranelift_frontend::FunctionBuilderContext,
    alloc_func: FuncId,
    counter: std::sync::atomic::AtomicUsize,
    fn_defs: Rc<HashMap<String, &'ctx Expr<'ctx>>>,
}

impl<'ctx> Default for JITCompiler<'ctx> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'ctx> JITCompiler<'ctx> {
    pub fn new() -> Self {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
            panic!("host machine is not supported: {}", msg);
        });
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .unwrap();

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        builder.symbol(
            "calvin_alloc",
            calvin_core::runtime::region::calvin_alloc as *const u8,
        );

        let mut module = JITModule::new(builder);

        let mut sig_alloc = module.make_signature();
        sig_alloc.returns.push(ir::AbiParam::new(ir::types::I64)); // pointer
        sig_alloc.params.push(ir::AbiParam::new(ir::types::I64)); // size
        sig_alloc.params.push(ir::AbiParam::new(ir::types::I64)); // align

        let alloc_func = module
            .declare_function("calvin_alloc", Linkage::Import, &sig_alloc)
            .unwrap();

        Self {
            module,
            ctx: Context::new(),
            builder_ctx: cranelift_frontend::FunctionBuilderContext::new(),
            alloc_func,
            counter: std::sync::atomic::AtomicUsize::new(0),
            fn_defs: Rc::new(HashMap::new()),
        }
    }

    pub fn with_fn_defs(mut self, fn_defs: Rc<HashMap<String, &'ctx Expr<'ctx>>>) -> Self {
        self.fn_defs = fn_defs;
        self
    }

    pub fn compile_and_dump_ir(&mut self, expr: &'ctx Expr<'ctx>, ty: &'ctx MonoType<'ctx>) -> String {
        self.ctx.clear();
        let ret_type = lower_type(ty);
        self.ctx
            .func
            .signature
            .returns
            .push(ir::AbiParam::new(ret_type));
        self.ctx.func.signature.call_conv = self.module.target_config().default_call_conv;
        {
            let mut builder =
                cranelift_frontend::FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);
            let mut lower_ctx = LoweringContext {
                builder,
                vars: HashMap::new(),
                fn_defs: &self.fn_defs,
                module: &mut self.module,
                alloc_func: self.alloc_func,
            };
            let mut ret_val = lower_ctx.visit(expr);
            let val_ty = lower_ctx.builder.func.dfg.value_type(ret_val);
            if val_ty != ret_type {
                if val_ty.is_int() && ret_type.is_int() {
                    if val_ty.bits() < ret_type.bits() {
                        ret_val = lower_ctx.builder.ins().uextend(ret_type, ret_val);
                    } else {
                        ret_val = lower_ctx.builder.ins().ireduce(ret_type, ret_val);
                    }
                } else if val_ty.is_int() && ret_type.is_float() {
                    ret_val = lower_ctx.builder.ins().fcvt_from_sint(ret_type, ret_val);
                } else if val_ty.is_float() && ret_type.is_int() {
                    ret_val = lower_ctx.builder.ins().fcvt_to_sint(ret_type, ret_val);
                }
            }
            lower_ctx.builder.ins().return_(&[ret_val]);
            lower_ctx.builder.finalize();
        }
        self.ctx.func.display().to_string()
    }

    pub fn compile_expr<T>(&mut self, expr: &'ctx Expr<'ctx>, ty: &'ctx MonoType<'ctx>) -> fn() -> T {
        self.ctx.clear();
        let ret_type = match ty.chase() {
            calvin_core::lang::types::MonoType::Prim(calvin_core::lang::types::Prim::Double | calvin_core::lang::types::Prim::Float) => ir::types::F64,
            _ => ir::types::I64,
        };
        self.ctx
            .func
            .signature
            .returns
            .push(ir::AbiParam::new(ret_type));
        self.ctx.func.signature.call_conv = self.module.target_config().default_call_conv;

        let fn_name = format!("anon_expr_{}", self.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
        let func_id = self
            .module
            .declare_function(&fn_name, Linkage::Export, &self.ctx.func.signature)
            .unwrap();

        {
            let mut builder =
                cranelift_frontend::FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            let mut lower_ctx = LoweringContext {
                builder,
                vars: HashMap::new(),
                fn_defs: &self.fn_defs,
                module: &mut self.module,
                alloc_func: self.alloc_func,
            };

            let mut ret_val = lower_ctx.visit(expr);
            let val_ty = lower_ctx.builder.func.dfg.value_type(ret_val);
            if val_ty != ret_type {
                if val_ty.is_int() && ret_type.is_int() {
                    if val_ty.bits() < ret_type.bits() {
                        ret_val = lower_ctx.builder.ins().uextend(ret_type, ret_val);
                    } else {
                        ret_val = lower_ctx.builder.ins().ireduce(ret_type, ret_val);
                    }
                } else if val_ty.is_int() && ret_type.is_float() {
                    ret_val = lower_ctx.builder.ins().fcvt_from_sint(ret_type, ret_val);
                } else if val_ty.is_float() && ret_type.is_int() {
                    ret_val = lower_ctx.builder.ins().fcvt_to_sint(ret_type, ret_val);
                }
            }
            lower_ctx.builder.ins().return_(&[ret_val]);
            lower_ctx.builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx).unwrap();
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions().unwrap();

        let code = self.module.get_finalized_function(func_id);

        unsafe { mem::transmute_copy(&code) }
    }
}

struct LoweringContext<'a, 'm, 'ctx> {
    builder: cranelift_frontend::FunctionBuilder<'a>,
    vars: HashMap<String, ir::Value>,
    fn_defs: &'a HashMap<String, &'ctx Expr<'ctx>>,
    module: &'m mut JITModule,
    alloc_func: FuncId,
}

impl<'a, 'm, 'ctx> LoweringContext<'a, 'm, 'ctx> {
    fn reconcile_types(&mut self, lhs: ir::Value, rhs: ir::Value) -> (ir::Value, ir::Value) {
        let lhs_ty = self.builder.func.dfg.value_type(lhs);
        let rhs_ty = self.builder.func.dfg.value_type(rhs);
        if lhs_ty == rhs_ty {
            return (lhs, rhs);
        }
        if lhs_ty.is_int() && rhs_ty.is_int() {
            if lhs_ty.bits() < rhs_ty.bits() {
                let lhs_ext = self.builder.ins().sextend(rhs_ty, lhs);
                (lhs_ext, rhs)
            } else {
                let rhs_ext = self.builder.ins().sextend(lhs_ty, rhs);
                (lhs, rhs_ext)
            }
        } else if lhs_ty.is_float() && rhs_ty.is_int() {
            let rhs_f = self.builder.ins().fcvt_from_sint(lhs_ty, rhs);
            (lhs, rhs_f)
        } else if lhs_ty.is_int() && rhs_ty.is_float() {
            let lhs_f = self.builder.ins().fcvt_from_sint(rhs_ty, lhs);
            (lhs_f, rhs)
        } else {
            (lhs, rhs)
        }
    }
}

impl<'a, 'm, 'ctx> ExprVisitor<'ctx, ir::Value> for LoweringContext<'a, 'm, 'ctx> {
    fn visit_literal(&mut self, lit: &Literal<'ctx>) -> ir::Value {
        match lit {
            Literal::Int(n) => self.builder.ins().iconst(ir::types::I64, *n),
            Literal::Float(f) | Literal::Double(f) => self.builder.ins().f64const(*f),
            Literal::Bool(b) => self
                .builder
                .ins()
                .iconst(ir::types::I8, if *b { 1 } else { 0 }),
            Literal::Char(c) => self.builder.ins().iconst(ir::types::I32, *c as i64),
            Literal::Unit => self.builder.ins().iconst(ir::types::I8, 0),
            _ => unimplemented!("literal lowering for {:?}", lit),
        }
    }

    fn visit_var(&mut self, name: &'ctx str) -> ir::Value {
        if let Some(val) = self.vars.get(name) {
            *val
        } else if let Some(def_expr) = self.fn_defs.get(name) {
            self.visit(def_expr)
        } else {
            panic!("unbound variable: {}", name)
        }
    }

    fn visit_let(
        &mut self,
        pat: &Pattern<'ctx>,
        def: &'ctx Expr<'ctx>,
        body: &'ctx Expr<'ctx>,
    ) -> ir::Value {
        let def_val = self.visit(def);

        let match_block = self.builder.create_block();
        let fail_block = self.builder.create_block();

        self.compile_pattern_check(pat, def_val, match_block, fail_block);

        self.builder.switch_to_block(fail_block);
        self.builder.seal_block(fail_block);
        self.builder.ins().trap(ir::TrapCode::unwrap_user(1));

        self.builder.switch_to_block(match_block);
        self.builder.seal_block(match_block);

        self.visit(body)
    }

    fn visit_app(&mut self, f: &'ctx Expr<'ctx>, args: &'ctx [&'ctx Expr<'ctx>]) -> ir::Value {
        // Unroll curried application for immediate closures
        let mut curr_f = f;
        let mut all_args = vec![args];
        while let Expr::App(inner_f, inner_args) = curr_f {
            all_args.insert(0, inner_args);
            curr_f = inner_f;
        }

        let mut flat_args = Vec::new();
        for arg_group in all_args {
            for arg in (*arg_group).iter() {
                flat_args.push(*arg);
            }
        }

        if flat_args.len() == 1 {
            if let Expr::Tuple(elems) = flat_args[0] {
                flat_args = elems.to_vec();
            }
        }

        if let Expr::Var(op) = curr_f {
            // Unary operator: not
            if flat_args.len() == 1 && *op == "not" {
                let arg_val = self.visit(flat_args[0]);
                let val_ty = self.builder.func.dfg.value_type(arg_val);
                let zero = self.builder.ins().iconst(val_ty, 0);
                return self.builder.ins().icmp(ir::condcodes::IntCC::Equal, arg_val, zero);
            }

            // Binary logical operators: and, or
            if flat_args.len() == 2 && matches!(*op, "and" | "or") {
                let lhs = self.visit(flat_args[0]);
                let rhs = self.visit(flat_args[1]);
                let (lhs, rhs) = self.reconcile_types(lhs, rhs);
                return match *op {
                    "and" => self.builder.ins().band(lhs, rhs),
                    "or" => self.builder.ins().bor(lhs, rhs),
                    _ => unreachable!(),
                };
            }

            // Binary comparison operators
            if flat_args.len() == 2 && matches!(*op, "==" | "!=" | "===" | "!==" | "<" | "<=" | ">" | ">=" | "~") {
                let lhs = self.visit(flat_args[0]);
                let rhs = self.visit(flat_args[1]);
                let (lhs, rhs) = self.reconcile_types(lhs, rhs);
                let is_float = self.builder.func.dfg.value_type(lhs).is_float();
                return if is_float {
                    let cc = match *op {
                        "==" | "===" | "~" => ir::condcodes::FloatCC::Equal,
                        "!=" | "!==" => ir::condcodes::FloatCC::NotEqual,
                        "<" => ir::condcodes::FloatCC::LessThan,
                        "<=" => ir::condcodes::FloatCC::LessThanOrEqual,
                        ">" => ir::condcodes::FloatCC::GreaterThan,
                        ">=" => ir::condcodes::FloatCC::GreaterThanOrEqual,
                        _ => unreachable!(),
                    };
                    self.builder.ins().fcmp(cc, lhs, rhs)
                } else {
                    let cc = match *op {
                        "==" | "===" | "~" => ir::condcodes::IntCC::Equal,
                        "!=" | "!==" => ir::condcodes::IntCC::NotEqual,
                        "<" => ir::condcodes::IntCC::SignedLessThan,
                        "<=" => ir::condcodes::IntCC::SignedLessThanOrEqual,
                        ">" => ir::condcodes::IntCC::SignedGreaterThan,
                        ">=" => ir::condcodes::IntCC::SignedGreaterThanOrEqual,
                        _ => unreachable!(),
                    };
                    self.builder.ins().icmp(cc, lhs, rhs)
                };
            }

            // Binary arithmetic operators
            if flat_args.len() == 2 && matches!(*op, "+" | "-" | "*" | "/" | "%") {
                if matches!(flat_args[0], Expr::Record(_) | Expr::Tuple(_) | Expr::Array(_))
                    || matches!(flat_args[1], Expr::Record(_) | Expr::Tuple(_) | Expr::Array(_))
                {
                    panic!("Attempted arithmetic operator {} on non-primitive pointer", op);
                }
                let lhs = self.visit(flat_args[0]);
                let rhs = self.visit(flat_args[1]);
                let (lhs, rhs) = self.reconcile_types(lhs, rhs);
                let is_float = self.builder.func.dfg.value_type(lhs).is_float();
                return match *op {
                    "+" => if is_float { self.builder.ins().fadd(lhs, rhs) } else { self.builder.ins().iadd(lhs, rhs) },
                    "-" => if is_float { self.builder.ins().fsub(lhs, rhs) } else { self.builder.ins().isub(lhs, rhs) },
                    "*" => if is_float { self.builder.ins().fmul(lhs, rhs) } else { self.builder.ins().imul(lhs, rhs) },
                    "/" => if is_float { self.builder.ins().fdiv(lhs, rhs) } else { self.builder.ins().sdiv(lhs, rhs) },
                    "%" => if is_float { unimplemented!("float rem") } else { self.builder.ins().srem(lhs, rhs) },
                    _ => unreachable!(),
                };
            }

            // Look up in fn_defs
            if let Some(def_expr) = self.fn_defs.get(*op) {
                curr_f = def_expr;
                while let Expr::App(inner_f, inner_args) = curr_f {
                    for arg in (*inner_args).iter().rev() {
                        flat_args.insert(0, *arg);
                    }
                    curr_f = inner_f;
                }
            }
        }

        if let Expr::Fn(_, _) = curr_f {
            let mut current_closure = curr_f;
            let mut old_vars = Vec::new();

            if flat_args.len() == 1 {
                if let Expr::Tuple(elems) = flat_args[0] {
                    if matches!(current_closure, Expr::Fn(Pattern::Var(_), _)) {
                        flat_args = elems.to_vec();
                    }
                }
            }

            let mut arg_idx = 0;
            while arg_idx < flat_args.len() {
                if let Expr::Fn(pat, body) = current_closure {
                    match pat {
                        Pattern::Var(name) => {
                            let arg_val = self.visit(flat_args[arg_idx]);
                            arg_idx += 1;
                            let old = self.vars.get(*name).copied();
                            self.vars.insert(name.to_string(), arg_val);
                            old_vars.push((name.to_string(), old));
                            current_closure = body;
                        }
                        Pattern::Tuple(pats) => {
                            let cur_arg = flat_args[arg_idx];
                            if let Expr::Tuple(elems) = cur_arg {
                                arg_idx += 1;
                                for (p, elem) in pats.iter().zip(elems.iter()) {
                                    let v = self.visit(elem);
                                    if let Pattern::Var(name) = p {
                                        let old = self.vars.get(*name).copied();
                                        self.vars.insert(name.to_string(), v);
                                        old_vars.push((name.to_string(), old));
                                    }
                                }
                                current_closure = body;
                            } else if flat_args.len() - arg_idx >= pats.len() {
                                for p in *pats {
                                    let v = self.visit(flat_args[arg_idx]);
                                    arg_idx += 1;
                                    if let Pattern::Var(name) = p {
                                        let old = self.vars.get(*name).copied();
                                        self.vars.insert(name.to_string(), v);
                                        old_vars.push((name.to_string(), old));
                                    }
                                }
                                current_closure = body;
                            } else {
                                unimplemented!("Partial application of tuple closure");
                            }
                        }
                        _ => unimplemented!("Unsupported pattern in closure: {:?}", pat),
                    }
                } else {
                    unimplemented!("Full application lowering requires environment packing (too many args)");
                }
            }

            let res = self.visit(current_closure);

            // Restore environment
            for (name, old) in old_vars.into_iter().rev() {
                if let Some(val) = old {
                    self.vars.insert(name, val);
                } else {
                    self.vars.remove(&name);
                }
            }

            return res;
        }

        unimplemented!("Full application lowering requires environment packing")
    }

    fn visit_tuple(&mut self, exprs: &'ctx [&'ctx Expr<'ctx>]) -> ir::Value {
        let mut vals = Vec::new();
        for e in exprs {
            vals.push(self.visit(e));
        }

        let elem_size = 8;
        let total_size = vals.len() as i64 * elem_size;

        let size_val = self.builder.ins().iconst(ir::types::I64, total_size);
        let align_val = self.builder.ins().iconst(ir::types::I64, 8);

        let local_alloc = self
            .module
            .declare_func_in_func(self.alloc_func, self.builder.func);
        let call = self.builder.ins().call(local_alloc, &[size_val, align_val]);
        let ptr = self.builder.inst_results(call)[0];

        for (i, val) in vals.iter().enumerate() {
            let offset = (i as i32) * (elem_size as i32);
            let val_ty = self.builder.func.dfg.value_type(*val);
            let val_to_store = if val_ty.is_int() && val_ty.bits() < 64 {
                self.builder.ins().uextend(ir::types::I64, *val)
            } else {
                *val
            };
            self.builder
                .ins()
                .store(cranelift_codegen::ir::MemFlags::new(), val_to_store, ptr, offset);
        }

        ptr
    }

    fn visit_array(&mut self, exprs: &'ctx [&'ctx Expr<'ctx>]) -> ir::Value {
        self.visit_tuple(exprs) // For now, tuples and fixed arrays are memory-identical blocks.
    }

    fn visit_record(&mut self, fields: &'ctx [(&'ctx str, &'ctx Expr<'ctx>)]) -> ir::Value {
        let mut vals = Vec::new();
        for (_, e) in fields {
            vals.push(self.visit(e));
        }

        let elem_size = 8;
        let total_size = vals.len() as i64 * elem_size;

        let size_val = self.builder.ins().iconst(ir::types::I64, total_size);
        let align_val = self.builder.ins().iconst(ir::types::I64, 8);

        let local_alloc = self
            .module
            .declare_func_in_func(self.alloc_func, self.builder.func);
        let call = self.builder.ins().call(local_alloc, &[size_val, align_val]);
        let ptr = self.builder.inst_results(call)[0];

        for (i, val) in vals.iter().enumerate() {
            let offset = (i as i32) * (elem_size as i32);
            let val_ty = self.builder.func.dfg.value_type(*val);
            let val_to_store = if val_ty.is_int() && val_ty.bits() < 64 {
                self.builder.ins().uextend(ir::types::I64, *val)
            } else {
                *val
            };
            self.builder
                .ins()
                .store(cranelift_codegen::ir::MemFlags::new(), val_to_store, ptr, offset);
        }

        ptr
    }

    fn visit_fn(&mut self, _pat: &Pattern<'ctx>, _body: &'ctx Expr<'ctx>) -> ir::Value {
        // Closure packing requires defining a new Cranelift Function in the JIT module.
        // For Phase 7, we emit a 0 ptr stub if unresolved, allowing unqualifier to run.
        self.builder.ins().iconst(ir::types::I64, 0)
    }

    fn visit_if(
        &mut self,
        cond: &'ctx Expr<'ctx>,
        then_e: &'ctx Expr<'ctx>,
        else_e: &'ctx Expr<'ctx>,
    ) -> ir::Value {
        let cond_val = self.visit(cond);

        let then_block = self.builder.create_block();
        let else_block = self.builder.create_block();
        let merge_block = self.builder.create_block();

        let cond_val = if self.builder.func.dfg.value_type(cond_val) != ir::types::I8 {
            let val_ty = self.builder.func.dfg.value_type(cond_val);
            let zero = self.builder.ins().iconst(val_ty, 0);
            self.builder.ins().icmp(ir::condcodes::IntCC::NotEqual, cond_val, zero)
        } else {
            cond_val
        };

        self.builder
            .ins()
            .brif(cond_val, then_block, &[], else_block, &[]);

        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        let then_val = self.visit(then_e);
        let ty = self.builder.func.dfg.value_type(then_val);
        let merge_param = self.builder.append_block_param(merge_block, ty);
        self.builder.ins().jump(merge_block, &[then_val]);

        self.builder.switch_to_block(else_block);
        self.builder.seal_block(else_block);
        let mut else_val = self.visit(else_e);
        let else_ty = self.builder.func.dfg.value_type(else_val);
        if ty != else_ty {
            if ty.is_int() && else_ty.is_int() {
                if else_ty.bits() < ty.bits() {
                    else_val = self.builder.ins().sextend(ty, else_val);
                } else {
                    else_val = self.builder.ins().ireduce(ty, else_val);
                }
            }
        }
        self.builder.ins().jump(merge_block, &[else_val]);

        self.builder.switch_to_block(merge_block);
        self.builder.seal_block(merge_block);

        merge_param
    }

    fn visit_field_access(&mut self, _expr: &'ctx Expr<'ctx>, _field: &'ctx str) -> ir::Value {
        self.builder.ins().iconst(ir::types::I64, 0)
    }

    fn visit_variant(&mut self, _tag: &'ctx str, payload: &'ctx Expr<'ctx>) -> ir::Value {
        let size_val = self.builder.ins().iconst(ir::types::I64, 16);
        let align_val = self.builder.ins().iconst(ir::types::I64, 8);

        let local_alloc = self
            .module
            .declare_func_in_func(self.alloc_func, self.builder.func);
        let call = self.builder.ins().call(local_alloc, &[size_val, align_val]);
        let ptr = self.builder.inst_results(call)[0];

        // Store tag at offset 0
        let tag_val = self.builder.ins().iconst(ir::types::I64, 0);
        self.builder
            .ins()
            .store(cranelift_codegen::ir::MemFlags::new(), tag_val, ptr, 0);

        // Store payload at offset 8
        let payload_val = self.visit(payload);
        let val_ty = self.builder.func.dfg.value_type(payload_val);
        let payload_to_store = if val_ty.is_int() && val_ty.bits() < 64 {
            self.builder.ins().uextend(ir::types::I64, payload_val)
        } else {
            payload_val
        };
        self.builder
            .ins()
            .store(cranelift_codegen::ir::MemFlags::new(), payload_to_store, ptr, 8);

        ptr
    }

    fn visit_case(
        &mut self,
        expr: &'ctx Expr<'ctx>,
        branches: &'ctx [(Pattern<'ctx>, &'ctx Expr<'ctx>)],
    ) -> ir::Value {
        let scrut_val = self.visit(expr);
        let merge_block = self.builder.create_block();

        let mut next_test_block = self.builder.create_block();
        self.builder.ins().jump(next_test_block, &[]);

        let mut result_type = None;

        for (pat, body) in branches {
            self.builder.switch_to_block(next_test_block);
            self.builder.seal_block(next_test_block);

            let match_block = self.builder.create_block();
            let fail_block = self.builder.create_block();

            self.compile_pattern_check(pat, scrut_val, match_block, fail_block);

            self.builder.switch_to_block(match_block);
            self.builder.seal_block(match_block);

            let body_val = self.visit(body);

            if result_type.is_none() {
                result_type = Some(self.builder.func.dfg.value_type(body_val));
                self.builder
                    .append_block_param(merge_block, result_type.unwrap());
            }

            self.builder.ins().jump(merge_block, &[body_val]);

            next_test_block = fail_block;
        }

        self.builder.switch_to_block(next_test_block);
        self.builder.seal_block(next_test_block);
        self.builder.ins().trap(ir::TrapCode::unwrap_user(1));

        self.builder.switch_to_block(merge_block);
        self.builder.seal_block(merge_block);

        self.builder.block_params(merge_block)[0]
    }

    fn visit_array_index(
        &mut self,
        _arr: &'ctx Expr<'ctx>,
        _idx: &'ctx Expr<'ctx>,
    ) -> ir::Value {
        self.builder.ins().iconst(ir::types::I64, 0)
    }
    fn visit_annotate(
        &mut self,
        expr: &'ctx Expr<'ctx>,
        _ty: &'ctx MonoType<'ctx>,
    ) -> ir::Value {
        self.visit(expr)
    }
}

impl<'a, 'm, 'ctx> LoweringContext<'a, 'm, 'ctx> {
    fn compile_pattern_check(
        &mut self,
        pat: &Pattern<'ctx>,
        val: ir::Value,
        match_block: ir::Block,
        fail_block: ir::Block,
    ) {
        use cranelift_codegen::ir::InstBuilder;
        match pat {
            Pattern::Any => {
                self.builder.ins().jump(match_block, &[]);
            }
            Pattern::Var(name) => {
                self.vars.insert(name.to_string(), val);
                self.builder.ins().jump(match_block, &[]);
            }
            Pattern::Literal(Literal::Int(n)) => {
                let const_val = self.builder.ins().iconst(ir::types::I64, *n);
                let (val, const_val) = self.reconcile_types(val, const_val);
                let cmp = self
                    .builder
                    .ins()
                    .icmp(ir::condcodes::IntCC::Equal, val, const_val);
                self.builder
                    .ins()
                    .brif(cmp, match_block, &[], fail_block, &[]);
            }
            Pattern::Literal(Literal::Char(c)) => {
                let const_val = self.builder.ins().iconst(ir::types::I32, *c as i64);
                let (val, const_val) = self.reconcile_types(val, const_val);
                let cmp = self
                    .builder
                    .ins()
                    .icmp(ir::condcodes::IntCC::Equal, val, const_val);
                self.builder
                    .ins()
                    .brif(cmp, match_block, &[], fail_block, &[]);
            }
            Pattern::Literal(Literal::Bool(b)) => {
                let const_val = self.builder.ins().iconst(ir::types::I8, if *b { 1 } else { 0 });
                let (val, const_val) = self.reconcile_types(val, const_val);
                let cmp = self
                    .builder
                    .ins()
                    .icmp(ir::condcodes::IntCC::Equal, val, const_val);
                self.builder
                    .ins()
                    .brif(cmp, match_block, &[], fail_block, &[]);
            }
            Pattern::Tuple(pats) => {
                if pats.is_empty() {
                    self.builder.ins().jump(match_block, &[]);
                    return;
                }

                let mut next_blocks = Vec::new();
                for _ in 0..pats.len() {
                    next_blocks.push(self.builder.create_block());
                }

                self.builder.ins().jump(next_blocks[0], &[]);

                for (i, p) in pats.iter().enumerate() {
                    self.builder.switch_to_block(next_blocks[i]);
                    self.builder.seal_block(next_blocks[i]);

                    let offset = (i * 8) as i32;
                    // Assume memory fields are I64 for now
                    let elem_val = self.builder.ins().load(
                        ir::types::I64,
                        cranelift_codegen::ir::MemFlags::new(),
                        val,
                        offset,
                    );

                    let succ_block = if i == pats.len() - 1 {
                        match_block
                    } else {
                        next_blocks[i + 1]
                    };

                    self.compile_pattern_check(p, elem_val, succ_block, fail_block);
                }
            }
            Pattern::Record(fields) => {
                if fields.is_empty() {
                    self.builder.ins().jump(match_block, &[]);
                    return;
                }

                let mut next_blocks = Vec::new();
                for _ in 0..fields.len() {
                    next_blocks.push(self.builder.create_block());
                }

                self.builder.ins().jump(next_blocks[0], &[]);

                for (i, (_, p)) in fields.iter().enumerate() {
                    self.builder.switch_to_block(next_blocks[i]);
                    self.builder.seal_block(next_blocks[i]);

                    let offset = (i * 8) as i32;
                    let elem_val = self.builder.ins().load(
                        ir::types::I64,
                        cranelift_codegen::ir::MemFlags::new(),
                        val,
                        offset,
                    );

                    let succ_block = if i == fields.len() - 1 {
                        match_block
                    } else {
                        next_blocks[i + 1]
                    };

                    self.compile_pattern_check(p, elem_val, succ_block, fail_block);
                }
            }
            Pattern::Variant(_tag, p) => {
                let offset = 8;
                let elem_val = self.builder.ins().load(
                    ir::types::I64,
                    cranelift_codegen::ir::MemFlags::new(),
                    val,
                    offset,
                );
                self.compile_pattern_check(p, elem_val, match_block, fail_block);
            }
            _ => unimplemented!("pattern check for {:?}", pat),
        }
    }
}
