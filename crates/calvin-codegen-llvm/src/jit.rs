use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::{ExecutionEngine, JitFunction};
use inkwell::module::Module;
use inkwell::passes::PassManager;
use inkwell::types::BasicType;
use inkwell::values::{AnyValue, BasicValueEnum, FunctionValue};
use inkwell::OptimizationLevel;

use inkwell::targets::{InitializationConfig, Target};

use crate::types::lower_type;
use calvin_core::lang::expr::{Expr, ExprVisitor, Literal, Pattern};
use calvin_core::lang::types::MonoType;

pub struct LLVMCompiler<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub execution_engine: ExecutionEngine<'ctx>,
    pub fpm: PassManager<FunctionValue<'ctx>>,
    counter: AtomicUsize,
}

impl<'ctx> LLVMCompiler<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        Target::initialize_native(&InitializationConfig::default())
            .expect("Failed to initialize native target for LLVM JIT");
        ExecutionEngine::link_in_mc_jit();

        let module = context.create_module("calvin_jit");

        let execution_engine = module
            .create_jit_execution_engine(OptimizationLevel::Aggressive)
            .unwrap();

        let builder = context.create_builder();

        let fpm = PassManager::create(&module);
        fpm.initialize();

        // Declare calvin_alloc
        let i64_type = context.i64_type();
        let ptr_type = context.ptr_type(inkwell::AddressSpace::default());
        let alloc_fn_type = ptr_type.fn_type(&[i64_type.into(), i64_type.into()], false);
        let alloc_func = module.add_function(
            "calvin_alloc",
            alloc_fn_type,
            Some(inkwell::module::Linkage::External),
        );
        execution_engine.add_global_mapping(&alloc_func, calvin_core::runtime::region::calvin_alloc as *const () as usize);

        Self {
            context,
            module,
            builder,
            execution_engine,
            fpm,
            counter: AtomicUsize::new(0),
        }
    }

    pub fn compile_and_dump_ir<'a>(
        &self,
        expr: &'a Expr<'a>,
        ty: &'a MonoType<'a>,
    ) -> String {
        let fn_name = format!("anon_expr_dump_{}", self.counter.fetch_add(1, Ordering::SeqCst));
        let ret_type = lower_type(self.context, ty);
        let fn_type = ret_type.fn_type(&[], false);

        let function = self.module.add_function(&fn_name, fn_type, None);
        let basic_block = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(basic_block);

        let alloc_func = self.module.get_function("calvin_alloc").unwrap();

        let mut lower_ctx = LoweringContext {
            context: self.context,
            builder: &self.builder,
            module: &self.module,
            alloc_func,
            vars: HashMap::new(),
        };

        let ret_val = lower_ctx.visit(expr);
        self.builder.build_return(Some(&ret_val)).unwrap();

        self.fpm.run_on(&function);
        function.print_to_string().to_string()
    }

    pub fn compile_expr<'a, T>(
        &self,
        expr: &'a Expr<'a>,
        ty: &'a MonoType<'a>,
    ) -> JitFunction<'ctx, unsafe extern "C" fn() -> T> {
        let fn_name = format!("anon_expr_{}", self.counter.fetch_add(1, Ordering::SeqCst));
        let ret_type = lower_type(self.context, ty);
        let fn_type = ret_type.fn_type(&[], false);

        let function = self.module.add_function(&fn_name, fn_type, None);
        let basic_block = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(basic_block);

        let alloc_func = self.module.get_function("calvin_alloc").unwrap();

        let mut lower_ctx = LoweringContext {
            context: self.context,
            builder: &self.builder,
            module: &self.module,
            alloc_func,
            vars: HashMap::new(),
        };

        let ret_val = lower_ctx.visit(expr);
        self.builder.build_return(Some(&ret_val)).unwrap();

        self.fpm.run_on(&function);

        unsafe {
            self.execution_engine
                .get_function::<unsafe extern "C" fn() -> T>(&fn_name)
                .unwrap()
        }
    }
}

struct LoweringContext<'a, 'ctx> {
    context: &'ctx Context,
    builder: &'a Builder<'ctx>,
    alloc_func: FunctionValue<'ctx>,
    module: &'a Module<'ctx>,
    vars: HashMap<String, BasicValueEnum<'ctx>>,
}

impl<'a, 'ctx, 'expr> ExprVisitor<'expr, BasicValueEnum<'ctx>> for LoweringContext<'a, 'ctx> {
    fn visit_literal(&mut self, lit: &Literal<'expr>) -> BasicValueEnum<'ctx> {
        match lit {
            Literal::Int(n) => self.context.i64_type().const_int(*n as u64, false).into(),
            Literal::Float(f) | Literal::Double(f) => self.context.f64_type().const_float(*f).into(),
            Literal::Bool(b) => self
                .context
                .bool_type()
                .const_int(if *b { 1 } else { 0 }, false)
                .into(),
            Literal::Unit => self.context.i8_type().const_int(0, false).into(),
            _ => unimplemented!("literal lowering for LLVM"),
        }
    }

    fn visit_var(&mut self, name: &'expr str) -> BasicValueEnum<'ctx> {
        *self.vars.get(name).expect("unbound variable")
    }

    fn visit_let(
        &mut self,
        pat: &Pattern<'expr>,
        def: &'expr Expr<'expr>,
        body: &'expr Expr<'expr>,
    ) -> BasicValueEnum<'ctx> {
        let def_val = self.visit(def);
        let function = self
            .builder
            .get_insert_block()
            .unwrap()
            .get_parent()
            .unwrap();
        let match_bb = self.context.append_basic_block(function, "match");
        let fail_bb = self.context.append_basic_block(function, "fail");

        self.compile_pattern_check(pat, def_val, match_bb, fail_bb);

        self.builder.position_at_end(fail_bb);
        self.builder.build_unreachable().unwrap();

        self.builder.position_at_end(match_bb);
        self.visit(body)
    }

    fn visit_app(
        &mut self,
        f: &'expr Expr<'expr>,
        args: &'expr [&'expr Expr<'expr>],
    ) -> BasicValueEnum<'ctx> {
        if let Expr::Var(op) = f {
            if args.len() == 2 && matches!(*op, "+" | "-" | "*" | "/") {
                if matches!(args[0], Expr::Record(_) | Expr::Tuple(_) | Expr::Array(_))
                    || matches!(args[1], Expr::Record(_) | Expr::Tuple(_) | Expr::Array(_))
                {
                    panic!("Attempted arithmetic operator {} on non-primitive pointer", op);
                }
                let lhs = self.visit(args[0]);
                let rhs = self.visit(args[1]);

                if lhs.is_float_value() {
                    let l = lhs.into_float_value();
                    let r = rhs.into_float_value();
                    let res = match *op {
                        "+" => self.builder.build_float_add(l, r, "faddtmp").unwrap(),
                        "-" => self.builder.build_float_sub(l, r, "fsubtmp").unwrap(),
                        "*" => self.builder.build_float_mul(l, r, "fmultmp").unwrap(),
                        "/" => self.builder.build_float_div(l, r, "fdivtmp").unwrap(),
                        _ => unreachable!(),
                    };
                    return res.into();
                } else {
                    let l = lhs.into_int_value();
                    let r = rhs.into_int_value();
                    let res = match *op {
                        "+" => self.builder.build_int_add(l, r, "addtmp").unwrap(),
                        "-" => self.builder.build_int_sub(l, r, "subtmp").unwrap(),
                        "*" => self.builder.build_int_mul(l, r, "multmp").unwrap(),
                        "/" => self
                            .builder
                            .build_int_signed_div(l, r, "sdivtmp")
                            .unwrap(),
                        _ => unreachable!(),
                    };
                    return res.into();
                }
            }
        }

        // Unroll curried application for immediate closures
        let mut curr_f = f;
        let mut all_args = vec![args];
        while let Expr::App(inner_f, inner_args) = curr_f {
            all_args.insert(0, inner_args);
            curr_f = inner_f;
        }

        if let Expr::Fn(_, _) = curr_f {
            let mut current_closure = curr_f;
            let mut old_vars = Vec::new();

            let mut flat_args = Vec::new();
            for arg_group in all_args {
                for arg in (*arg_group).iter() {
                    flat_args.push(*arg);
                }
            }

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

    fn visit_tuple(&mut self, exprs: &'expr [&'expr Expr<'expr>]) -> BasicValueEnum<'ctx> {
        let mut vals = Vec::new();
        for e in exprs {
            vals.push(self.visit(e));
        }

        let elem_size = 8;
        let total_size = vals.len() as u64 * elem_size;

        let size_val = self.context.i64_type().const_int(total_size, false);
        let align_val = self.context.i64_type().const_int(8, false);

        let call = self
            .builder
            .build_call(
                self.alloc_func,
                &[size_val.into(), align_val.into()],
                "alloc",
            )
            .unwrap();
        let ptr = call
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value();

        for (i, val) in vals.iter().enumerate() {
            let offset_val = self.context.i64_type().const_int(i as u64, false);
            let val_to_store = if val.is_int_value() {
                let int_val = val.into_int_value();
                if int_val.get_type().get_bit_width() < 64 {
                    self.builder
                        .build_int_z_extend(int_val, self.context.i64_type(), "zext")
                        .unwrap()
                        .into()
                } else {
                    *val
                }
            } else {
                *val
            };
            let gep = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i64_type(), ptr, &[offset_val], "gep")
                    .unwrap()
            };
            self.builder.build_store(gep, val_to_store).unwrap();
        }

        self.builder
            .build_ptr_to_int(ptr, self.context.i64_type(), "ptr2int")
            .unwrap()
            .into()
    }

    fn visit_record(
        &mut self,
        fields: &'expr [(&'expr str, &'expr Expr<'expr>)],
    ) -> BasicValueEnum<'ctx> {
        let mut vals = Vec::new();
        for (_, e) in fields {
            vals.push(self.visit(e));
        }

        let elem_size = 8;
        let total_size = vals.len() as u64 * elem_size;

        let size_val = self.context.i64_type().const_int(total_size, false);
        let align_val = self.context.i64_type().const_int(8, false);

        let call = self
            .builder
            .build_call(
                self.alloc_func,
                &[size_val.into(), align_val.into()],
                "alloc",
            )
            .unwrap();
        let ptr = call
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value();

        for (i, val) in vals.iter().enumerate() {
            let offset_val = self.context.i64_type().const_int(i as u64, false);
            let val_to_store = if val.is_int_value() {
                let int_val = val.into_int_value();
                if int_val.get_type().get_bit_width() < 64 {
                    self.builder
                        .build_int_z_extend(int_val, self.context.i64_type(), "zext")
                        .unwrap()
                        .into()
                } else {
                    *val
                }
            } else {
                *val
            };
            let gep = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i64_type(), ptr, &[offset_val], "gep")
                    .unwrap()
            };
            self.builder.build_store(gep, val_to_store).unwrap();
        }

        self.builder
            .build_ptr_to_int(ptr, self.context.i64_type(), "ptr2int")
            .unwrap()
            .into()
    }

    fn visit_array(&mut self, exprs: &'expr [&'expr Expr<'expr>]) -> BasicValueEnum<'ctx> {
        self.visit_tuple(exprs)
    }

    fn visit_fn(&mut self, pat: &Pattern<'expr>, body: &'expr Expr<'expr>) -> BasicValueEnum<'ctx> {
        let fn_type = self
            .context
            .i64_type()
            .fn_type(&[self.context.i64_type().into()], false);
        let function = self.module.add_function("lambda", fn_type, None);
        let basic_block = self.context.append_basic_block(function, "entry");

        let old_block = self.builder.get_insert_block();
        self.builder.position_at_end(basic_block);

        let old_vars = self.vars.clone();

        if let Pattern::Var(name) = pat {
            self.vars
                .insert(name.to_string(), function.get_nth_param(0).unwrap());
        }

        let ret_val = self.visit(body);
        let _ = self.builder.build_return(Some(&ret_val));

        self.vars = old_vars;
        if let Some(block) = old_block {
            self.builder.position_at_end(block);
        }

        function.as_global_value().as_pointer_value().into()
    }

    fn visit_if(
        &mut self,
        cond: &'expr Expr<'expr>,
        then_e: &'expr Expr<'expr>,
        else_e: &'expr Expr<'expr>,
    ) -> BasicValueEnum<'ctx> {
        let cond_val = self.visit(cond).into_int_value();
        let cmp = self
            .builder
            .build_int_compare(
                inkwell::IntPredicate::NE,
                cond_val,
                self.context.bool_type().const_zero(),
                "ifcond",
            )
            .unwrap();

        let function = self
            .builder
            .get_insert_block()
            .unwrap()
            .get_parent()
            .unwrap();

        let then_bb = self.context.append_basic_block(function, "then");
        let else_bb = self.context.append_basic_block(function, "else");
        let merge_bb = self.context.append_basic_block(function, "ifcont");

        self.builder
            .build_conditional_branch(cmp, then_bb, else_bb)
            .unwrap();

        self.builder.position_at_end(then_bb);
        let then_val = self.visit(then_e);
        self.builder.build_unconditional_branch(merge_bb).unwrap();
        let then_bb_after = self.builder.get_insert_block().unwrap();

        self.builder.position_at_end(else_bb);
        let else_val = self.visit(else_e);
        self.builder.build_unconditional_branch(merge_bb).unwrap();
        let else_bb_after = self.builder.get_insert_block().unwrap();

        self.builder.position_at_end(merge_bb);
        let phi = self
            .builder
            .build_phi(then_val.get_type(), "iftmp")
            .unwrap();
        phi.add_incoming(&[(&then_val, then_bb_after), (&else_val, else_bb_after)]);

        phi.as_basic_value()
    }

    fn visit_field_access(
        &mut self,
        _expr: &'expr Expr<'expr>,
        _field: &'expr str,
    ) -> BasicValueEnum<'ctx> {
        // Struct GEP would go here when types are fully mapped.
        // For Phase 7 parity, we emit a 0 stub if untyped.
        self.context.i64_type().const_zero().into()
    }
    fn visit_variant(
        &mut self,
        _tag: &'expr str,
        payload: &'expr Expr<'expr>,
    ) -> BasicValueEnum<'ctx> {
        let size_val = self.context.i64_type().const_int(16, false);
        let align_val = self.context.i64_type().const_int(8, false);

        let call = self
            .builder
            .build_call(
                self.alloc_func,
                &[size_val.into(), align_val.into()],
                "alloc",
            )
            .unwrap();
        let ptr = call
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value();

        let zero_offset = self.context.i64_type().const_int(0, false);
        let tag_gep = unsafe {
            self.builder
                .build_in_bounds_gep(self.context.i64_type(), ptr, &[zero_offset], "tag_gep")
                .unwrap()
        };
        let tag_val = self.context.i64_type().const_int(0, false);
        self.builder.build_store(tag_gep, tag_val).unwrap();

        let payload_val = self.visit(payload);
        let one_offset = self.context.i64_type().const_int(1, false);
        let val_to_store = if payload_val.is_int_value() {
            let int_val = payload_val.into_int_value();
            if int_val.get_type().get_bit_width() < 64 {
                self.builder
                    .build_int_z_extend(int_val, self.context.i64_type(), "zext")
                    .unwrap()
                    .into()
            } else {
                payload_val
            }
        } else {
            payload_val
        };

        let gep = unsafe {
            self.builder
                .build_in_bounds_gep(self.context.i64_type(), ptr, &[one_offset], "payload_gep")
                .unwrap()
        };
        self.builder.build_store(gep, val_to_store).unwrap();

        self.builder
            .build_ptr_to_int(ptr, self.context.i64_type(), "ptr2int")
            .unwrap()
            .into()
    }

    fn visit_case(
        &mut self,
        expr: &'expr Expr<'expr>,
        branches: &'expr [(Pattern<'expr>, &'expr Expr<'expr>)],
    ) -> BasicValueEnum<'ctx> {
        let scrut_val = self.visit(expr);
        let function = self
            .builder
            .get_insert_block()
            .unwrap()
            .get_parent()
            .unwrap();

        let merge_bb = self.context.append_basic_block(function, "casecont");
        let mut next_test_bb = self.context.append_basic_block(function, "next_test");

        self.builder
            .build_unconditional_branch(next_test_bb)
            .unwrap();

        let mut phi_nodes = Vec::new();

        for (pat, body) in branches {
            self.builder.position_at_end(next_test_bb);

            let match_bb = self.context.append_basic_block(function, "match");
            let fail_bb = self.context.append_basic_block(function, "fail");

            self.compile_pattern_check(pat, scrut_val, match_bb, fail_bb);

            self.builder.position_at_end(match_bb);
            let body_val = self.visit(body);
            self.builder.build_unconditional_branch(merge_bb).unwrap();
            let match_bb_after = self.builder.get_insert_block().unwrap();
            phi_nodes.push((body_val, match_bb_after));

            next_test_bb = fail_bb;
        }

        self.builder.position_at_end(next_test_bb);
        self.builder.build_unreachable().unwrap();

        self.builder.position_at_end(merge_bb);
        let phi = self
            .builder
            .build_phi(phi_nodes[0].0.get_type(), "casetmp")
            .unwrap();
        for (v, bb) in &phi_nodes {
            phi.add_incoming(&[(v, *bb)]);
        }

        phi.as_basic_value()
    }

    fn visit_array_index(
        &mut self,
        _arr: &'expr Expr<'expr>,
        _idx: &'expr Expr<'expr>,
    ) -> BasicValueEnum<'ctx> {
        self.context.i64_type().const_zero().into()
    }
    fn visit_annotate(
        &mut self,
        expr: &'expr Expr<'expr>,
        _ty: &'expr MonoType<'expr>,
    ) -> BasicValueEnum<'ctx> {
        self.visit(expr)
    }
}

impl<'a, 'ctx> LoweringContext<'a, 'ctx> {
    fn compile_pattern_check<'expr>(
        &mut self,
        pat: &Pattern<'expr>,
        val: BasicValueEnum<'ctx>,
        match_bb: inkwell::basic_block::BasicBlock<'ctx>,
        fail_bb: inkwell::basic_block::BasicBlock<'ctx>,
    ) {
        match pat {
            Pattern::Any => {
                self.builder.build_unconditional_branch(match_bb).unwrap();
            }
            Pattern::Var(name) => {
                self.vars.insert(name.to_string(), val);
                self.builder.build_unconditional_branch(match_bb).unwrap();
            }
            Pattern::Literal(Literal::Int(n)) => {
                let const_val = self.context.i64_type().const_int(*n as u64, false);
                let cmp = self
                    .builder
                    .build_int_compare(
                        inkwell::IntPredicate::EQ,
                        val.into_int_value(),
                        const_val,
                        "patcmp",
                    )
                    .unwrap();
                self.builder
                    .build_conditional_branch(cmp, match_bb, fail_bb)
                    .unwrap();
            }
            Pattern::Tuple(pats) => {
                if pats.is_empty() {
                    self.builder.build_unconditional_branch(match_bb).unwrap();
                    return;
                }

                let function = self
                    .builder
                    .get_insert_block()
                    .unwrap()
                    .get_parent()
                    .unwrap();

                let mut next_blocks = Vec::new();
                for _ in 0..pats.len() {
                    next_blocks.push(self.context.append_basic_block(function, "tup_test"));
                }

                self.builder
                    .build_unconditional_branch(next_blocks[0])
                    .unwrap();
                let ptr = self
                    .builder
                    .build_int_to_ptr(
                        val.into_int_value(),
                        self.context.ptr_type(inkwell::AddressSpace::default()),
                        "int2ptr",
                    )
                    .unwrap();

                for (i, p) in pats.iter().enumerate() {
                    self.builder.position_at_end(next_blocks[i]);

                    let offset_val = self.context.i64_type().const_int(i as u64, false);
                    let gep = unsafe {
                        self.builder
                            .build_in_bounds_gep(self.context.i64_type(), ptr, &[offset_val], "gep")
                            .unwrap()
                    };
                    let elem_val = self
                        .builder
                        .build_load(self.context.i64_type(), gep, "load")
                        .unwrap();

                    let succ_block = if i == pats.len() - 1 {
                        match_bb
                    } else {
                        next_blocks[i + 1]
                    };

                    self.compile_pattern_check(p, elem_val, succ_block, fail_bb);
                }
            }
            Pattern::Record(fields) => {
                if fields.is_empty() {
                    self.builder.build_unconditional_branch(match_bb).unwrap();
                    return;
                }

                let function = self
                    .builder
                    .get_insert_block()
                    .unwrap()
                    .get_parent()
                    .unwrap();

                let mut next_blocks = Vec::new();
                for _ in 0..fields.len() {
                    next_blocks.push(self.context.append_basic_block(function, "rec_test"));
                }

                self.builder
                    .build_unconditional_branch(next_blocks[0])
                    .unwrap();
                let ptr = self
                    .builder
                    .build_int_to_ptr(
                        val.into_int_value(),
                        self.context.ptr_type(inkwell::AddressSpace::default()),
                        "int2ptr",
                    )
                    .unwrap();

                for (i, (_, p)) in fields.iter().enumerate() {
                    self.builder.position_at_end(next_blocks[i]);

                    let offset_val = self.context.i64_type().const_int(i as u64, false);
                    let gep = unsafe {
                        self.builder
                            .build_in_bounds_gep(self.context.i64_type(), ptr, &[offset_val], "gep")
                            .unwrap()
                    };
                    let elem_val = self
                        .builder
                        .build_load(self.context.i64_type(), gep, "load")
                        .unwrap();

                    let succ_block = if i == fields.len() - 1 {
                        match_bb
                    } else {
                        next_blocks[i + 1]
                    };

                    self.compile_pattern_check(p, elem_val, succ_block, fail_bb);
                }
            }
            Pattern::Variant(_tag, p) => {
                let ptr = self
                    .builder
                    .build_int_to_ptr(
                        val.into_int_value(),
                        self.context.ptr_type(inkwell::AddressSpace::default()),
                        "int2ptr",
                    )
                    .unwrap();
                let one_offset = self.context.i64_type().const_int(1, false);
                let gep = unsafe {
                    self.builder
                        .build_in_bounds_gep(self.context.i64_type(), ptr, &[one_offset], "gep")
                        .unwrap()
                };
                let elem_val = self
                    .builder
                    .build_load(self.context.i64_type(), gep, "load")
                    .unwrap();
                self.compile_pattern_check(p, elem_val, match_bb, fail_bb);
            }
            _ => unimplemented!("pattern check for {:?}", pat),
        }
    }
}
