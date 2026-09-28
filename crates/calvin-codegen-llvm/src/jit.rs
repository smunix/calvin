use std::collections::HashMap;
use std::rc::Rc;
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
use calvin_core::lang::types::{MonoType, Prim};

pub struct LLVMCompiler<'ctx, 'ast> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub execution_engine: ExecutionEngine<'ctx>,
    pub fpm: PassManager<FunctionValue<'ctx>>,
    counter: AtomicUsize,
    pub fn_defs: Rc<HashMap<String, &'ast Expr<'ast>>>,
}

impl<'ctx, 'ast> LLVMCompiler<'ctx, 'ast> {
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

        let alloc_func = declare_calvin_alloc(context, &module);
        execution_engine.add_global_mapping(
            &alloc_func,
            calvin_core::runtime::region::calvin_alloc as *const () as usize,
        );

        Self {
            context,
            module,
            builder,
            execution_engine,
            fpm,
            counter: AtomicUsize::new(0),
            fn_defs: Rc::new(HashMap::new()),
        }
    }

    pub fn with_fn_defs(mut self, fn_defs: Rc<HashMap<String, &'ast Expr<'ast>>>) -> Self {
        self.fn_defs = fn_defs;
        self
    }

    fn normalize_return_value(
        &self,
        ret_val: BasicValueEnum<'ctx>,
        ty: &'ast MonoType<'ast>,
    ) -> BasicValueEnum<'ctx> {
        let is_float = matches!(
            ty.chase(),
            calvin_core::lang::types::MonoType::Prim(
                calvin_core::lang::types::Prim::Double | calvin_core::lang::types::Prim::Float
            )
        );
        if is_float && ret_val.is_int_value() {
            self.builder
                .build_signed_int_to_float(
                    ret_val.into_int_value(),
                    self.context.f64_type(),
                    "sitofp",
                )
                .unwrap()
                .into()
        } else if !is_float && ret_val.is_int_value() {
            let int_val = ret_val.into_int_value();
            if int_val.get_type().get_bit_width() < 64 {
                self.builder
                    .build_int_z_extend(int_val, self.context.i64_type(), "zext")
                    .unwrap()
                    .into()
            } else {
                ret_val
            }
        } else {
            ret_val
        }
    }

    pub fn compile_and_dump_ir(&self, expr: &'ast Expr<'ast>, ty: &'ast MonoType<'ast>) -> String {
        let fn_name = format!(
            "anon_expr_dump_{}",
            self.counter.fetch_add(1, Ordering::SeqCst)
        );
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
            fn_defs: &self.fn_defs,
        };

        let raw_val = lower_ctx.visit(expr);
        let ret_val = self.normalize_return_value(raw_val, ty);
        self.builder.build_return(Some(&ret_val)).unwrap();

        self.fpm.run_on(&function);
        function.print_to_string().to_string()
    }

    pub fn compile_expr<T>(
        &self,
        expr: &'ast Expr<'ast>,
        ty: &'ast MonoType<'ast>,
    ) -> JitFunction<'ctx, unsafe extern "C" fn() -> T> {
        let fn_name = format!("anon_expr_{}", self.counter.fetch_add(1, Ordering::SeqCst));
        let is_float = matches!(
            ty.chase(),
            calvin_core::lang::types::MonoType::Prim(
                calvin_core::lang::types::Prim::Double | calvin_core::lang::types::Prim::Float
            )
        );
        let ret_type: inkwell::types::BasicTypeEnum = if is_float {
            self.context.f64_type().into()
        } else {
            self.context.i64_type().into()
        };
        let fn_type = ret_type.fn_type(&[], false);

        let expr_module = self.context.create_module(&fn_name);
        let alloc_func = declare_calvin_alloc(self.context, &expr_module);

        let function = expr_module.add_function(&fn_name, fn_type, None);
        let basic_block = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(basic_block);

        let mut lower_ctx = LoweringContext {
            context: self.context,
            builder: &self.builder,
            module: &expr_module,
            alloc_func,
            vars: HashMap::new(),
            fn_defs: &self.fn_defs,
        };

        let raw_val = lower_ctx.visit(expr);
        let ret_val = self.normalize_return_value(raw_val, ty);
        self.builder.build_return(Some(&ret_val)).unwrap();

        self.execution_engine.add_module(&expr_module).unwrap();
        self.execution_engine.add_global_mapping(
            &alloc_func,
            calvin_core::runtime::region::calvin_alloc as *const () as usize,
        );

        unsafe {
            self.execution_engine
                .get_function::<unsafe extern "C" fn() -> T>(&fn_name)
                .unwrap()
        }
    }
}

fn declare_calvin_alloc<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
) -> FunctionValue<'ctx> {
    let i64_type = context.i64_type();
    let ptr_type = context.ptr_type(inkwell::AddressSpace::default());
    let alloc_fn_type = ptr_type.fn_type(&[i64_type.into(), i64_type.into()], false);
    module.add_function(
        "calvin_alloc",
        alloc_fn_type,
        Some(inkwell::module::Linkage::External),
    )
}

struct LoweringContext<'a, 'ctx, 'ast> {
    context: &'ctx Context,
    builder: &'a Builder<'ctx>,
    alloc_func: FunctionValue<'ctx>,
    module: &'a Module<'ctx>,
    vars: HashMap<String, BasicValueEnum<'ctx>>,
    fn_defs: &'a HashMap<String, &'ast Expr<'ast>>,
}

impl<'a, 'ctx, 'ast> LoweringContext<'a, 'ctx, 'ast> {
    fn reconcile_int_types(
        &self,
        mut l: inkwell::values::IntValue<'ctx>,
        mut r: inkwell::values::IntValue<'ctx>,
    ) -> (
        inkwell::values::IntValue<'ctx>,
        inkwell::values::IntValue<'ctx>,
    ) {
        let l_bits = l.get_type().get_bit_width();
        let r_bits = r.get_type().get_bit_width();
        if l_bits < r_bits {
            l = self
                .builder
                .build_int_s_extend(l, r.get_type(), "sext")
                .unwrap();
        } else if r_bits < l_bits {
            r = self
                .builder
                .build_int_s_extend(r, l.get_type(), "sext")
                .unwrap();
        }
        (l, r)
    }

    fn reconcile_types(
        &self,
        lhs: BasicValueEnum<'ctx>,
        rhs: BasicValueEnum<'ctx>,
    ) -> (BasicValueEnum<'ctx>, BasicValueEnum<'ctx>) {
        if lhs.is_float_value() && rhs.is_float_value() {
            (lhs, rhs)
        } else if lhs.is_int_value() && rhs.is_int_value() {
            let (l, r) = self.reconcile_int_types(lhs.into_int_value(), rhs.into_int_value());
            (l.into(), r.into())
        } else if lhs.is_float_value() && rhs.is_int_value() {
            let r_f = self
                .builder
                .build_signed_int_to_float(rhs.into_int_value(), self.context.f64_type(), "sitofp")
                .unwrap();
            (lhs, r_f.into())
        } else if lhs.is_int_value() && rhs.is_float_value() {
            let l_f = self
                .builder
                .build_signed_int_to_float(lhs.into_int_value(), self.context.f64_type(), "sitofp")
                .unwrap();
            (l_f.into(), rhs)
        } else {
            (lhs, rhs)
        }
    }

    fn allocate_and_store_elements(&self, vals: &[BasicValueEnum<'ctx>]) -> BasicValueEnum<'ctx> {
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

    fn unroll_call_chain(
        &self,
        f: &'ast Expr<'ast>,
        args: &'ast [&'ast Expr<'ast>],
    ) -> (&'ast Expr<'ast>, Vec<&'ast Expr<'ast>>) {
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

        (curr_f, flat_args)
    }

    fn lower_unary_op(&mut self, op: &str, arg: &'ast Expr<'ast>) -> Option<BasicValueEnum<'ctx>> {
        if op == "not" {
            let arg_val = self.visit(arg).into_int_value();
            let zero = arg_val.get_type().const_zero();
            return Some(
                self.builder
                    .build_int_compare(inkwell::IntPredicate::EQ, arg_val, zero, "nottmp")
                    .unwrap()
                    .into(),
            );
        }

        match op {
            "id" | "convert" => Some(self.visit(arg)),
            "i2d" | "l2d" => {
                let arg_val = self.visit(arg);
                Some(if arg_val.is_int_value() {
                    self.builder
                        .build_signed_int_to_float(
                            arg_val.into_int_value(),
                            self.context.f64_type(),
                            "sitofp",
                        )
                        .unwrap()
                        .into()
                } else {
                    arg_val
                })
            }
            "i2f" | "l2f" => {
                let arg_val = self.visit(arg);
                Some(if arg_val.is_int_value() {
                    self.builder
                        .build_signed_int_to_float(
                            arg_val.into_int_value(),
                            self.context.f32_type(),
                            "sitofp",
                        )
                        .unwrap()
                        .into()
                } else {
                    arg_val
                })
            }
            "f2d" => {
                let arg_val = self.visit(arg);
                Some(if arg_val.is_float_value() {
                    let float_val = arg_val.into_float_value();
                    if float_val.get_type() == self.context.f32_type() {
                        self.builder
                            .build_float_ext(float_val, self.context.f64_type(), "fpext")
                            .unwrap()
                            .into()
                    } else {
                        arg_val
                    }
                } else {
                    arg_val
                })
            }
            "b2i" | "b2l" => {
                let arg_val = self.visit(arg);
                let target_ty = if op == "b2i" {
                    self.context.i32_type()
                } else {
                    self.context.i64_type()
                };
                Some(if arg_val.is_int_value() {
                    let int_val = arg_val.into_int_value();
                    if int_val.get_type().get_bit_width() < target_ty.get_bit_width() {
                        self.builder
                            .build_int_z_extend(int_val, target_ty, "zext")
                            .unwrap()
                            .into()
                    } else {
                        arg_val
                    }
                } else {
                    arg_val
                })
            }
            "s2i" | "i2l" | "l2i16" => {
                let arg_val = self.visit(arg);
                let target_ty = match op {
                    "s2i" => self.context.i32_type(),
                    "i2l" => self.context.i64_type(),
                    _ => self.context.i128_type(),
                };
                Some(if arg_val.is_int_value() {
                    let int_val = arg_val.into_int_value();
                    if int_val.get_type().get_bit_width() < target_ty.get_bit_width() {
                        self.builder
                            .build_int_s_extend(int_val, target_ty, "sext")
                            .unwrap()
                            .into()
                    } else {
                        arg_val
                    }
                } else {
                    arg_val
                })
            }
            _ => None,
        }
    }

    fn lower_binary_logical_op(
        &mut self,
        op: &str,
        lhs_expr: &'ast Expr<'ast>,
        rhs_expr: &'ast Expr<'ast>,
    ) -> BasicValueEnum<'ctx> {
        let lhs = self.visit(lhs_expr).into_int_value();
        let rhs = self.visit(rhs_expr).into_int_value();
        let (lhs, rhs) = self.reconcile_int_types(lhs, rhs);
        match op {
            "and" => self.builder.build_and(lhs, rhs, "andtmp").unwrap().into(),
            "or" => self.builder.build_or(lhs, rhs, "ortmp").unwrap().into(),
            _ => unreachable!("invalid logical op: {}", op),
        }
    }

    fn lower_binary_cmp_op(
        &mut self,
        op: &str,
        lhs_expr: &'ast Expr<'ast>,
        rhs_expr: &'ast Expr<'ast>,
    ) -> BasicValueEnum<'ctx> {
        let lhs = self.visit(lhs_expr);
        let rhs = self.visit(rhs_expr);
        let (lhs, rhs) = self.reconcile_types(lhs, rhs);
        if lhs.is_float_value() {
            let left_val = lhs.into_float_value();
            let right_val = rhs.into_float_value();
            let pred = match op {
                "==" | "===" | "~" => inkwell::FloatPredicate::OEQ,
                "!=" | "!==" => inkwell::FloatPredicate::ONE,
                "<" => inkwell::FloatPredicate::OLT,
                "<=" => inkwell::FloatPredicate::OLE,
                ">" => inkwell::FloatPredicate::OGT,
                ">=" => inkwell::FloatPredicate::OGE,
                _ => unreachable!("invalid cmp op: {}", op),
            };
            self.builder
                .build_float_compare(pred, left_val, right_val, "fcmptmp")
                .unwrap()
                .into()
        } else {
            let left_val = lhs.into_int_value();
            let right_val = rhs.into_int_value();
            let pred = match op {
                "==" | "===" | "~" => inkwell::IntPredicate::EQ,
                "!=" | "!==" => inkwell::IntPredicate::NE,
                "<" => inkwell::IntPredicate::SLT,
                "<=" => inkwell::IntPredicate::SLE,
                ">" => inkwell::IntPredicate::SGT,
                ">=" => inkwell::IntPredicate::SGE,
                _ => unreachable!("invalid cmp op: {}", op),
            };
            self.builder
                .build_int_compare(pred, left_val, right_val, "icmptmp")
                .unwrap()
                .into()
        }
    }

    fn lower_binary_arith_op(
        &mut self,
        op: &str,
        lhs_expr: &'ast Expr<'ast>,
        rhs_expr: &'ast Expr<'ast>,
    ) -> BasicValueEnum<'ctx> {
        if matches!(lhs_expr, Expr::Record(_) | Expr::Tuple(_) | Expr::Array(_))
            || matches!(rhs_expr, Expr::Record(_) | Expr::Tuple(_) | Expr::Array(_))
        {
            panic!(
                "Attempted arithmetic operator {} on non-primitive pointer",
                op
            );
        }
        let lhs = self.visit(lhs_expr);
        let rhs = self.visit(rhs_expr);
        let (lhs, rhs) = self.reconcile_types(lhs, rhs);

        if lhs.is_float_value() {
            let left_val = lhs.into_float_value();
            let right_val = rhs.into_float_value();
            let res = match op {
                "+" => self
                    .builder
                    .build_float_add(left_val, right_val, "faddtmp")
                    .unwrap(),
                "-" => self
                    .builder
                    .build_float_sub(left_val, right_val, "fsubtmp")
                    .unwrap(),
                "*" => self
                    .builder
                    .build_float_mul(left_val, right_val, "fmultmp")
                    .unwrap(),
                "/" => self
                    .builder
                    .build_float_div(left_val, right_val, "fdivtmp")
                    .unwrap(),
                _ => unreachable!("invalid float arith op: {}", op),
            };
            res.into()
        } else {
            let left_val = lhs.into_int_value();
            let right_val = rhs.into_int_value();
            let res = match op {
                "+" => self
                    .builder
                    .build_int_add(left_val, right_val, "addtmp")
                    .unwrap(),
                "-" => self
                    .builder
                    .build_int_sub(left_val, right_val, "subtmp")
                    .unwrap(),
                "*" => self
                    .builder
                    .build_int_mul(left_val, right_val, "multmp")
                    .unwrap(),
                "/" => self
                    .builder
                    .build_int_signed_div(left_val, right_val, "sdivtmp")
                    .unwrap(),
                "%" => self
                    .builder
                    .build_int_signed_rem(left_val, right_val, "sremtmp")
                    .unwrap(),
                _ => unreachable!("invalid int arith op: {}", op),
            };
            res.into()
        }
    }

    fn lower_closure_call(
        &mut self,
        closure: &'ast Expr<'ast>,
        mut flat_args: Vec<&'ast Expr<'ast>>,
    ) -> BasicValueEnum<'ctx> {
        let mut current_closure = closure;
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
                unimplemented!(
                    "Full application lowering requires environment packing (too many args)"
                );
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

        res
    }
}

impl<'a, 'ctx, 'ast> ExprVisitor<'ast, BasicValueEnum<'ctx>> for LoweringContext<'a, 'ctx, 'ast> {
    fn visit_literal(&mut self, lit: &Literal<'ast>) -> BasicValueEnum<'ctx> {
        match lit {
            Literal::Int(n) => self.context.i64_type().const_int(*n as u64, false).into(),
            Literal::Float(f) | Literal::Double(f) => {
                self.context.f64_type().const_float(*f).into()
            }
            Literal::Bool(b) => self
                .context
                .bool_type()
                .const_int(if *b { 1 } else { 0 }, false)
                .into(),
            Literal::Char(c) => self.context.i32_type().const_int(*c as u64, false).into(),
            Literal::Unit => self.context.i8_type().const_int(0, false).into(),
            _ => unimplemented!("literal lowering for LLVM"),
        }
    }

    fn visit_var(&mut self, name: &'ast str) -> BasicValueEnum<'ctx> {
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
        pat: &Pattern<'ast>,
        def: &'ast Expr<'ast>,
        body: &'ast Expr<'ast>,
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
        f: &'ast Expr<'ast>,
        args: &'ast [&'ast Expr<'ast>],
    ) -> BasicValueEnum<'ctx> {
        let (mut curr_f, mut flat_args) = self.unroll_call_chain(f, args);

        if let Expr::Var(op) = curr_f {
            if flat_args.len() == 1 {
                if let Some(res) = self.lower_unary_op(op, flat_args[0]) {
                    return res;
                }
            }

            if flat_args.len() == 2 && matches!(*op, "and" | "or") {
                return self.lower_binary_logical_op(op, flat_args[0], flat_args[1]);
            }

            if flat_args.len() == 2
                && matches!(
                    *op,
                    "==" | "!=" | "===" | "!==" | "<" | "<=" | ">" | ">=" | "~"
                )
            {
                return self.lower_binary_cmp_op(op, flat_args[0], flat_args[1]);
            }

            if flat_args.len() == 2 && matches!(*op, "+" | "-" | "*" | "/" | "%") {
                return self.lower_binary_arith_op(op, flat_args[0], flat_args[1]);
            }

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
            return self.lower_closure_call(curr_f, flat_args);
        }

        unimplemented!("Full application lowering requires environment packing")
    }

    fn visit_tuple(&mut self, exprs: &'ast [&'ast Expr<'ast>]) -> BasicValueEnum<'ctx> {
        let vals: Vec<_> = exprs.iter().map(|e| self.visit(e)).collect();
        self.allocate_and_store_elements(&vals)
    }

    fn visit_record(
        &mut self,
        fields: &'ast [(&'ast str, &'ast Expr<'ast>)],
    ) -> BasicValueEnum<'ctx> {
        let vals: Vec<_> = fields.iter().map(|(_, e)| self.visit(e)).collect();
        self.allocate_and_store_elements(&vals)
    }

    fn visit_array(&mut self, exprs: &'ast [&'ast Expr<'ast>]) -> BasicValueEnum<'ctx> {
        self.visit_tuple(exprs)
    }

    fn visit_fn(&mut self, pat: &Pattern<'ast>, body: &'ast Expr<'ast>) -> BasicValueEnum<'ctx> {
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
        cond: &'ast Expr<'ast>,
        then_e: &'ast Expr<'ast>,
        else_e: &'ast Expr<'ast>,
    ) -> BasicValueEnum<'ctx> {
        let cond_val = self.visit(cond).into_int_value();
        let zero = cond_val.get_type().const_zero();
        let cmp = self
            .builder
            .build_int_compare(inkwell::IntPredicate::NE, cond_val, zero, "ifcond")
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
        let mut else_val = self.visit(else_e);
        if then_val.is_int_value() && else_val.is_int_value() {
            let tv = then_val.into_int_value();
            let ev = else_val.into_int_value();
            if tv.get_type().get_bit_width() != ev.get_type().get_bit_width() {
                if ev.get_type().get_bit_width() < tv.get_type().get_bit_width() {
                    else_val = self
                        .builder
                        .build_int_s_extend(ev, tv.get_type(), "sext")
                        .unwrap()
                        .into();
                } else {
                    else_val = self
                        .builder
                        .build_int_truncate(ev, tv.get_type(), "trunc")
                        .unwrap()
                        .into();
                }
            }
        }
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
        _expr: &'ast Expr<'ast>,
        _field: &'ast str,
    ) -> BasicValueEnum<'ctx> {
        // Struct GEP would go here when types are fully mapped.
        // For Phase 7 parity, we emit a 0 stub if untyped.
        self.context.i64_type().const_zero().into()
    }
    fn visit_variant(
        &mut self,
        _tag: &'ast str,
        payload: &'ast Expr<'ast>,
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
        expr: &'ast Expr<'ast>,
        branches: &'ast [(Pattern<'ast>, &'ast Expr<'ast>)],
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
        _arr: &'ast Expr<'ast>,
        _idx: &'ast Expr<'ast>,
    ) -> BasicValueEnum<'ctx> {
        self.context.i64_type().const_zero().into()
    }
    fn visit_annotate(
        &mut self,
        expr: &'ast Expr<'ast>,
        ty: &'ast MonoType<'ast>,
    ) -> BasicValueEnum<'ctx> {
        let val = self.visit(expr);
        match ty.chase() {
            MonoType::Prim(Prim::Double | Prim::Float) => {
                if val.is_int_value() {
                    let target_float_ty = if matches!(ty.chase(), MonoType::Prim(Prim::Float)) {
                        self.context.f32_type()
                    } else {
                        self.context.f64_type()
                    };
                    self.builder
                        .build_signed_int_to_float(val.into_int_value(), target_float_ty, "sitofp")
                        .unwrap()
                        .into()
                } else if val.is_float_value() {
                    let fv = val.into_float_value();
                    if matches!(ty.chase(), MonoType::Prim(Prim::Double))
                        && fv.get_type() == self.context.f32_type()
                    {
                        self.builder
                            .build_float_ext(fv, self.context.f64_type(), "fpext")
                            .unwrap()
                            .into()
                    } else if matches!(ty.chase(), MonoType::Prim(Prim::Float))
                        && fv.get_type() == self.context.f64_type()
                    {
                        self.builder
                            .build_float_trunc(fv, self.context.f32_type(), "fptrunc")
                            .unwrap()
                            .into()
                    } else {
                        val
                    }
                } else {
                    val
                }
            }
            MonoType::Prim(Prim::Int | Prim::Long | Prim::Short | Prim::Byte) => {
                if val.is_float_value() {
                    let target_int_ty = match ty.chase() {
                        MonoType::Prim(Prim::Byte) => self.context.i8_type(),
                        MonoType::Prim(Prim::Short) => self.context.i16_type(),
                        MonoType::Prim(Prim::Int) => self.context.i32_type(),
                        MonoType::Prim(Prim::Long) => self.context.i64_type(),
                        _ => unreachable!(),
                    };
                    self.builder
                        .build_float_to_signed_int(val.into_float_value(), target_int_ty, "fptosi")
                        .unwrap()
                        .into()
                } else if val.is_int_value() {
                    let iv = val.into_int_value();
                    let target_bits = match ty.chase() {
                        MonoType::Prim(Prim::Byte) => 8,
                        MonoType::Prim(Prim::Short) => 16,
                        MonoType::Prim(Prim::Int) => 32,
                        MonoType::Prim(Prim::Long) => 64,
                        _ => unreachable!(),
                    };
                    let cur_bits = iv.get_type().get_bit_width();
                    let target_int_ty = match target_bits {
                        8 => self.context.i8_type(),
                        16 => self.context.i16_type(),
                        32 => self.context.i32_type(),
                        _ => self.context.i64_type(),
                    };
                    if cur_bits < target_bits {
                        self.builder
                            .build_int_s_extend(iv, target_int_ty, "sext")
                            .unwrap()
                            .into()
                    } else if cur_bits > target_bits {
                        self.builder
                            .build_int_truncate(iv, target_int_ty, "trunc")
                            .unwrap()
                            .into()
                    } else {
                        val
                    }
                } else {
                    val
                }
            }
            _ => val,
        }
    }
}

impl<'a, 'ctx, 'ast> LoweringContext<'a, 'ctx, 'ast> {
    fn compile_pattern_check(
        &mut self,
        pat: &Pattern<'ast>,
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
                let (val_int, const_val) =
                    self.reconcile_int_types(val.into_int_value(), const_val);
                let cmp = self
                    .builder
                    .build_int_compare(inkwell::IntPredicate::EQ, val_int, const_val, "patcmp")
                    .unwrap();
                self.builder
                    .build_conditional_branch(cmp, match_bb, fail_bb)
                    .unwrap();
            }
            Pattern::Literal(Literal::Char(c)) => {
                let const_val = self.context.i32_type().const_int(*c as u64, false);
                let (val_int, const_val) =
                    self.reconcile_int_types(val.into_int_value(), const_val);
                let cmp = self
                    .builder
                    .build_int_compare(inkwell::IntPredicate::EQ, val_int, const_val, "patchar")
                    .unwrap();
                self.builder
                    .build_conditional_branch(cmp, match_bb, fail_bb)
                    .unwrap();
            }
            Pattern::Literal(Literal::Bool(b)) => {
                let const_val = self
                    .context
                    .bool_type()
                    .const_int(if *b { 1 } else { 0 }, false);
                let (val_int, const_val) =
                    self.reconcile_int_types(val.into_int_value(), const_val);
                let cmp = self
                    .builder
                    .build_int_compare(inkwell::IntPredicate::EQ, val_int, const_val, "patbool")
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
