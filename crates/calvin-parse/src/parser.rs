use crate::ast::*;
use crate::lexer::Token;
use calvin_core::context::TypeContext;
use calvin_core::lang::expr::{Expr, Literal, Pattern};
use calvin_core::lang::types::Prim;
use chumsky::prelude::*;

pub type Span = std::ops::Range<usize>;
pub type ParseError<'a> = extra::Err<Rich<'a, Token<'a>, Span>>;

pub fn pattern_parser<'a, 'ctx, I>(
    ctx: &'ctx TypeContext,
) -> impl Parser<'a, I, Pattern<'ctx>, ParseError<'a>> + Clone
where
    'a: 'ctx,
    I: chumsky::input::ValueInput<'a, Token = Token<'a>, Span = Span>,
{
    recursive(|pat| {
        let literal = select! {
            Token::Int(n) => Literal::Int(n),
            Token::Float(n) => Literal::Float(n),
            Token::Double(n) => Literal::Double(n),
            Token::Byte(n) => Literal::Int(n as i64),
            Token::Short(n) => Literal::Int(n as i64),
            Token::Long(n) => Literal::Int(n),
            Token::Int128(n) => Literal::Int(n as i64),
            Token::Timespan(n) => Literal::Int(n),
            Token::Char(c) => Literal::Char(c),
            Token::String(s) => Literal::String(s),
        }
        .map(Pattern::Literal);

        let wildcard = just(Token::Underscore).to(Pattern::Any);
        let var = select! { Token::Ident(name) => Pattern::Var(name) };

        let tuple_or_paren = pat
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(move |pats| {
                if pats.len() == 1 {
                    pats.into_iter().next().unwrap()
                } else if pats.is_empty() {
                    Pattern::Literal(Literal::Unit)
                } else {
                    Pattern::Tuple(ctx.alloc_slice_clone(&pats))
                }
            });

        let record_field = select! { Token::Ident(name) => name }
            .then_ignore(just(Token::Colon).or(just(Token::Eq)))
            .then(pat.clone());

        let record = record_field
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map(move |fields| Pattern::Record(ctx.alloc_slice_clone(&fields)));

        let variant_tag = choice((
            select! { Token::Ident(name) => name },
            select! { Token::Int(0) => "0", Token::Int(1) => "1", Token::Int(2) => "2" },
        ));

        let variant_payload = variant_tag
            .then_ignore(just(Token::Colon).or(just(Token::Eq)))
            .then(pat.clone())
            .map(move |(tag, payload)| Pattern::Variant(tag, &*ctx.alloc(payload)));

        let variant_unit = variant_tag
            .map(move |tag| {
                let unit = Pattern::Literal(Literal::Unit);
                Pattern::Variant(tag, &*ctx.alloc(unit))
            });

        let variant = just(Token::Pipe)
            .ignore_then(choice((variant_payload, variant_unit)))
            .then_ignore(just(Token::Pipe));

        choice((literal, wildcard, var, tuple_or_paren, record, variant))
    })
}

pub fn expr_parser<'a, 'ctx, I>(
    ctx: &'ctx TypeContext,
) -> impl Parser<'a, I, &'ctx Expr<'ctx>, ParseError<'a>> + Clone
where
    'a: 'ctx,
    'ctx: 'a,
    I: chumsky::input::ValueInput<'a, Token = Token<'a>, Span = Span>,
{
    recursive(|expr| {
        let literal = select! {
            Token::Int(n) => Literal::Int(n),
            Token::Float(n) => Literal::Float(n),
            Token::Double(n) => Literal::Double(n),
            Token::Byte(n) => Literal::Int(n as i64),
            Token::Short(n) => Literal::Int(n as i64),
            Token::Long(n) => Literal::Int(n),
            Token::Int128(n) => Literal::Int(n as i64),
            Token::Timespan(n) => Literal::Int(n),
            Token::Char(c) => Literal::Char(c),
            Token::String(s) => Literal::String(s),
        };

        let atom_literal = literal.map(move |l| &*ctx.alloc(Expr::Literal(l)));
        let atom_var = select! { Token::Ident(name) => &*ctx.alloc(Expr::Var(name)) };

        let tuple_or_paren = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(move |exprs| {
                if exprs.len() == 1 {
                    exprs[0]
                } else if exprs.is_empty() {
                    &*ctx.alloc(Expr::Literal(Literal::Unit))
                } else {
                    &*ctx.alloc(Expr::Tuple(ctx.alloc_slice_clone(&exprs)))
                }
            });

        let array = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(move |exprs| &*ctx.alloc(Expr::Array(ctx.alloc_slice_clone(&exprs))));

        let record_field = select! { Token::Ident(name) => name }
            .then_ignore(just(Token::Colon).or(just(Token::Eq)))
            .then(expr.clone());

        let record = record_field
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map(move |fields| &*ctx.alloc(Expr::Record(ctx.alloc_slice_clone(&fields))));

        let variant_tag = choice((
            select! { Token::Ident(name) => name },
            select! { Token::Int(0) => "0", Token::Int(1) => "1", Token::Int(2) => "2" },
        ));

        let variant_payload = variant_tag
            .then_ignore(just(Token::Colon).or(just(Token::Eq)))
            .then(expr.clone())
            .map(move |(tag, payload)| &*ctx.alloc(Expr::Variant(tag, payload)));

        let variant_unit = variant_tag
            .map(move |tag| {
                let unit = &*ctx.alloc(Expr::Literal(Literal::Unit));
                &*ctx.alloc(Expr::Variant(tag, unit))
            });

        let variant = just(Token::Pipe)
            .ignore_then(choice((variant_payload, variant_unit)))
            .then_ignore(just(Token::Pipe));

        let atom = choice((
            atom_literal,
            atom_var,
            array,
            record,
            variant,
            tuple_or_paren.clone(),
        ));

        enum Postfix<'a, 'ctx> {
            Field(&'a str),
            Index(&'ctx Expr<'ctx>),
            Slice(&'ctx Expr<'ctx>, Option<&'ctx Expr<'ctx>>),
        }

        let slice_parser = just(Token::LBracket)
            .ignore_then(expr.clone())
            .then(just(Token::Colon).ignore_then(expr.clone().or_not()).or_not())
            .then_ignore(just(Token::RBracket))
            .map(|(start, slice_opt)| match slice_opt {
                Some(end_opt) => Postfix::Slice(start, end_opt),
                None => Postfix::Index(start),
            });

        let field_name = choice((
            select! { Token::Ident(name) => name },
            select! { Token::Int(0) => "0", Token::Int(1) => "1", Token::Int(2) => "2" },
        ));
        let field_parser = just(Token::Dot)
            .ignore_then(field_name)
            .map(Postfix::Field);

        let atom_postfix = atom.foldl(
            choice((field_parser, slice_parser)).repeated(),
            move |base, post| match post {
                Postfix::Field(f) => &*ctx.alloc(Expr::FieldAccess(base, f)),
                Postfix::Index(idx) => &*ctx.alloc(Expr::ArrayIndex(base, idx)),
                Postfix::Slice(start, end_opt) => {
                    let slice_var = &*ctx.alloc(Expr::Var("slice"));
                    if let Some(end) = end_opt {
                        &*ctx.alloc(Expr::App(slice_var, ctx.alloc_slice_clone(&[base, start, end])))
                    } else {
                        &*ctx.alloc(Expr::App(slice_var, ctx.alloc_slice_clone(&[base, start])))
                    }
                }
            },
        ).boxed();

        let atom_annotated = atom_postfix
            .then(just(Token::DoubleColon).ignore_then(qual_type_parser()).or_not())
            .map(move |(e, ty_opt)| match ty_opt {
                Some(qte) => &*ctx.alloc(Expr::Annotate(e, lower_qual_type(ctx, &qte))),
                None => e,
            });

        let app_arg = choice((atom_literal, tuple_or_paren.clone()));

        let app = atom_annotated
            .clone()
            .then(app_arg.repeated().collect::<Vec<_>>())
            .map(move |(f, args)| {
                if args.is_empty() {
                    f
                } else {
                    let args_slice = ctx.alloc_slice_clone(&args);
                    &*ctx.alloc(Expr::App(f, args_slice))
                }
            })
            .boxed();

        let op_mul = app.clone().foldl(
            choice((
                just(Token::Star).to("*"),
                just(Token::Slash).to("/"),
                just(Token::Percent).to("%"),
            ))
            .then(app.clone())
            .repeated(),
            move |lhs, (op_name, rhs)| {
                &*ctx.alloc(Expr::App(
                    &*ctx.alloc(Expr::Var(op_name)),
                    ctx.alloc_slice_clone(&[lhs, rhs]),
                ))
            },
        ).boxed();

        let op_add = op_mul.clone().foldl(
            choice((
                just(Token::Plus).to("+"),
                just(Token::Minus).to("-"),
                just(Token::PlusPlus).to("++"),
            ))
            .then(op_mul.clone())
            .repeated(),
            move |lhs, (op_name, rhs)| {
                &*ctx.alloc(Expr::App(
                    &*ctx.alloc(Expr::Var(op_name)),
                    ctx.alloc_slice_clone(&[lhs, rhs]),
                ))
            },
        ).boxed();

        let op_rel = op_add.clone().foldl(
            choice((
                just(Token::Lte).to("<="),
                just(Token::Gte).to(">="),
                just(Token::Lt).to("<"),
                just(Token::Gt).to(">"),
                just(Token::DoubleEq).to("=="),
                just(Token::NotEq).to("!="),
                just(Token::TripleEq).to("==="),
                just(Token::ExclDoubleEq).to("!=="),
                just(Token::Tilde).to("~"),
            ))
            .then(op_add.clone())
            .repeated(),
            move |lhs, (op_name, rhs)| {
                &*ctx.alloc(Expr::App(
                    &*ctx.alloc(Expr::Var(op_name)),
                    ctx.alloc_slice_clone(&[lhs, rhs]),
                ))
            },
        ).boxed();

        let op_and = op_rel.clone().foldl(
            just(Token::And)
                .to("and")
                .then(op_rel.clone())
                .repeated(),
            move |lhs, (op_name, rhs)| {
                &*ctx.alloc(Expr::App(
                    &*ctx.alloc(Expr::Var(op_name)),
                    ctx.alloc_slice_clone(&[lhs, rhs]),
                ))
            },
        ).boxed();

        let op_or = op_and.clone().foldl(
            just(Token::Or)
                .to("or")
                .then(op_and.clone())
                .repeated(),
            move |lhs, (op_name, rhs)| {
                &*ctx.alloc(Expr::App(
                    &*ctx.alloc(Expr::Var(op_name)),
                    ctx.alloc_slice_clone(&[lhs, rhs]),
                ))
            },
        ).boxed();

        let pat = pattern_parser(ctx);

        let let_bind = just(Token::Let)
            .ignore_then(pat.clone())
            .then_ignore(just(Token::Eq))
            .then(expr.clone())
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .map(move |((p, def), body)| &*ctx.alloc(Expr::Let(p, def, body)));

        let lambda = just(Token::Lambda)
            .or(just(Token::Fn))
            .ignore_then(pat.clone().repeated().at_least(1).collect::<Vec<_>>())
            .then(just(Token::Arrow).to(false).or(just(Token::Dot).to(true)))
            .then(expr.clone())
            .map(move |((pats, is_dot), body)| {
                if pats.len() == 1 {
                    &*ctx.alloc(Expr::Fn(pats[0].clone(), body))
                } else if is_dot {
                    let tup_pat = Pattern::Tuple(ctx.alloc_slice_clone(&pats));
                    &*ctx.alloc(Expr::Fn(tup_pat, body))
                } else {
                    pats.into_iter()
                        .rfold(body, |acc, p| &*ctx.alloc(Expr::Fn(p, acc)))
                }
            });

        let case_branches = pat
            .clone()
            .then_ignore(just(Token::Arrow))
            .then(expr.clone())
            .separated_by(just(Token::Pipe))
            .collect::<Vec<_>>();

        let match_expr = just(Token::Match)
            .ignore_then(expr.clone())
            .then_ignore(just(Token::With))
            .then_ignore(just(Token::Pipe).or_not())
            .then(case_branches)
            .map(move |(scrutinee, branches)| {
                &*ctx.alloc(Expr::Case(scrutinee, ctx.alloc_slice_clone(&branches)))
            });

        let if_expr = just(Token::If)
            .ignore_then(expr.clone())
            .then_ignore(just(Token::Then))
            .then(expr.clone())
            .then_ignore(just(Token::Else))
            .then(expr.clone())
            .map(move |((cond, then_e), else_e)| {
                &*ctx.alloc(Expr::If(cond, then_e, else_e))
            });

        let do_stmt = choice((
            expr.clone()
                .then(choice((just(Token::LeftArrow), just(Token::Eq))))
                .then(expr.clone())
                .map(|((_lhs, _), rhs)| (Some(Pattern::Any), rhs)),
            just(Token::Return)
                .ignore_then(expr.clone())
                .map(|e| (None, e)),
            expr.clone().map(|e| (Some(Pattern::Any), e)),
        ));

        let do_expr = just(Token::Do)
            .ignore_then(
                do_stmt
                    .separated_by(just(Token::Semi))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map(move |stmts| {
                let unit = &*ctx.alloc(Expr::Literal(Literal::Unit));
                stmts.into_iter().rfold(unit, |acc, (pat_opt, e)| {
                    if let Some(p) = pat_opt {
                        &*ctx.alloc(Expr::Let(p, e, acc))
                    } else {
                        e
                    }
                })
            });

        let unpack_expr = just(Token::Unpack)
            .ignore_then(select! { Token::Ident(name) => name })
            .then_ignore(just(Token::Eq))
            .then(expr.clone())
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .map(move |((name, def), body)| {
                &*ctx.alloc(Expr::Let(Pattern::Var(name), def, body))
            });

        choice((if_expr, let_bind, lambda, match_expr, do_expr, unpack_expr, op_or))
            .then(just(Token::DoubleColon).ignore_then(qual_type_parser()).or_not())
            .map(move |(e, ty_opt)| match ty_opt {
                Some(qte) => &*ctx.alloc(Expr::Annotate(e, lower_qual_type(ctx, &qte))),
                None => e,
            })
    })
}

fn lower_type_expr<'a, 'ctx>(
    ctx: &'ctx TypeContext,
    te: &TypeExpr<'a>,
) -> &'ctx calvin_core::lang::types::MonoType<'ctx> {
    match te {
        TypeExpr::Prim(p) => ctx.alloc(calvin_core::lang::types::MonoType::Prim(*p)),
        TypeExpr::Var(_) => ctx.alloc(calvin_core::lang::types::MonoType::TVar(
            ctx.fresh_tvar_id(),
            std::cell::Cell::new(None),
        )),
        TypeExpr::Tuple(ts) => {
            let lowered: Vec<&'ctx calvin_core::lang::types::MonoType<'ctx>> =
                ts.iter().map(|t| lower_type_expr(ctx, t)).collect();
            ctx.alloc(calvin_core::lang::types::MonoType::Tuple(
                ctx.arena().alloc_slice_clone(&lowered),
            ))
        }
        TypeExpr::Fn(dom, codom) => {
            let d = lower_type_expr(ctx, dom);
            let c = lower_type_expr(ctx, codom);
            ctx.alloc(calvin_core::lang::types::MonoType::Fn(d, c))
        }
        TypeExpr::Array(inner) => {
            let elem = lower_type_expr(ctx, inner);
            ctx.alloc(calvin_core::lang::types::MonoType::Array(elem))
        }
        TypeExpr::App(head, args) => {
            let h = lower_type_expr(ctx, head);
            let lowered_args: Vec<&'ctx calvin_core::lang::types::MonoType<'ctx>> =
                args.iter().map(|a| lower_type_expr(ctx, a)).collect();
            ctx.alloc(calvin_core::lang::types::MonoType::App(
                h,
                ctx.arena().alloc_slice_clone(&lowered_args),
            ))
        }
    }
}

fn lower_qual_type<'a, 'ctx>(
    ctx: &'ctx TypeContext,
    qte: &QualTypeExpr<'a>,
) -> &'ctx calvin_core::lang::types::MonoType<'ctx> {
    lower_type_expr(ctx, &qte.ty)
}

pub fn type_expr_parser<'a, I>() -> impl Parser<'a, I, TypeExpr<'a>, ParseError<'a>> + Clone
where
    I: chumsky::input::ValueInput<'a, Token = Token<'a>, Span = Span>,
{
    recursive(|ty| {
        let prim = select! {
            Token::Ident("unit") => Prim::Unit,
            Token::Ident("bool") => Prim::Bool,
            Token::Ident("char") => Prim::Char,
            Token::Ident("byte") => Prim::Byte,
            Token::Ident("short") => Prim::Short,
            Token::Ident("int") => Prim::Int,
            Token::Ident("long") => Prim::Long,
            Token::Ident("int128") => Prim::Int128,
            Token::Ident("float") => Prim::Float,
            Token::Ident("double") => Prim::Double,
            Token::Ident("time") => Prim::Time,
            Token::Ident("timespan") => Prim::TimeSpan,
            Token::Ident("datetime") => Prim::DateTime,
        }
        .map(TypeExpr::Prim);

        let var = select! {
            Token::Ident(name) => TypeExpr::Var(name),
            Token::String(s) => TypeExpr::Var(s),
        };

        let fixed_array = just(Token::LBracket)
            .ignore_then(just(Token::Colon))
            .ignore_then(ty.clone())
            .then_ignore(just(Token::Pipe))
            .then(choice((
                select! { Token::Ident(name) => name },
                select! { Token::Int(_) => "1" },
            )))
            .then_ignore(just(Token::Colon))
            .then_ignore(just(Token::RBracket))
            .map(|(elem_ty, _)| TypeExpr::Array(Box::new(elem_ty)));

        let opaque_type = just(Token::Lt)
            .ignore_then(
                select! { Token::Ident(name) => name }
                    .separated_by(just(Token::Dot))
                    .at_least(1)
                    .collect::<Vec<_>>(),
            )
            .then_ignore(just(Token::Gt))
            .map(|_parts| TypeExpr::Prim(Prim::Char));

        let exists_type = just(Token::Exists)
            .then(select! { Token::Ident(name) => name })
            .then_ignore(just(Token::Dot))
            .then(ty.clone())
            .map(|(_, inner)| inner);

        let elem_ty = choice((
            ty.clone().repeated().at_least(2).collect::<Vec<_>>().map(|mut tys| {
                let head = tys.remove(0);
                TypeExpr::App(Box::new(head), tys)
            }),
            ty.clone(),
        ));

        let tuple_sep = choice((just(Token::Comma), just(Token::Star), just(Token::Plus)));
        let tuple_or_paren = elem_ty
            .separated_by(tuple_sep)
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(|tys| {
                if tys.len() == 1 {
                    tys.into_iter().next().unwrap()
                } else {
                    TypeExpr::Tuple(tys)
                }
            });

        let array = just(Token::LBracket)
            .ignore_then(ty.clone())
            .then_ignore(just(Token::RBracket))
            .map(|t| TypeExpr::Array(Box::new(t)));

        let record_field = choice((
            select! { Token::Ident(name) => name }
                .then_ignore(just(Token::Colon).or(just(Token::Eq)))
                .then(ty.clone())
                .map(|(_, t)| t),
            ty.clone(),
        ));
        let record_type = record_field
            .separated_by(choice((just(Token::Comma), just(Token::Star))))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map(|_| TypeExpr::Tuple(vec![]));

        let raw_atom = choice((prim, fixed_array, opaque_type, exists_type, array, tuple_or_paren, record_type, var));

        let atom = raw_atom
            .then(just(Token::At).then(choice((select! { Token::Ident(name) => name }, just(Token::Question).to("?")))).or_not())
            .map(|(base, _)| base);

        atom.clone()
            .then(just(Token::Arrow).ignore_then(ty.clone()).or_not())
            .map(|(lhs, rhs)| match rhs {
                Some(r) => TypeExpr::Fn(Box::new(lhs), Box::new(r)),
                None => lhs,
            })
    })
}

pub fn constraint_parser<'a, I>() -> impl Parser<'a, I, TypeConstraint<'a>, ParseError<'a>> + Clone
where
    I: chumsky::input::ValueInput<'a, Token = Token<'a>, Span = Span>,
{
    let type_expr = type_expr_parser();

    let not_eq = type_expr
        .clone()
        .then_ignore(just(Token::NotEq))
        .then(type_expr.clone())
        .map(|(t1, t2)| TypeConstraint::NotEq(t1, t2));

    let eq_cst = type_expr
        .clone()
        .then_ignore(just(Token::Eq))
        .then(type_expr.clone())
        .map(|(t1, t2)| TypeConstraint::Eq(t1, t2));

    let lookup_cst = select! { Token::Ident(name) => name }
        .then_ignore(just(Token::Slash))
        .then(select! { Token::Ident(lbl) => lbl })
        .then_ignore(just(Token::DoubleColon))
        .then(type_expr.clone())
        .map(|((s, lbl), ty)| TypeConstraint::FieldLookup(s, lbl, ty));

    let class_cst = select! { Token::Ident(name) => name }
        .then(type_expr.clone().repeated().at_least(1).collect::<Vec<_>>())
        .map(|(name, args)| TypeConstraint::Class(name, args));

    choice((class_cst, lookup_cst, not_eq, eq_cst))
}

pub fn qual_type_parser<'a, I>() -> impl Parser<'a, I, QualTypeExpr<'a>, ParseError<'a>> + Clone
where
    I: chumsky::input::ValueInput<'a, Token = Token<'a>, Span = Span>,
{
    let constraint = constraint_parser();
    let type_expr = type_expr_parser();

    let context = constraint
        .separated_by(just(Token::Comma))
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen))
        .then_ignore(just(Token::FatArrow));

    context
        .or_not()
        .then(type_expr)
        .map(|(ctx_opt, ty)| QualTypeExpr {
            context: ctx_opt.unwrap_or_default(),
            ty,
        })
}

pub fn module_parser<'a, 'ctx, I>(
    ctx: &'ctx TypeContext,
) -> impl Parser<'a, I, Module<'ctx>, ParseError<'a>> + Clone
where
    'a: 'ctx,
    'ctx: 'a,
    I: chumsky::input::ValueInput<'a, Token = Token<'a>, Span = Span>,
{
    let type_expr = type_expr_parser();
    let expr = expr_parser(ctx);

    let op_sym = choice((
        just(Token::Plus).to("+"),
        just(Token::Minus).to("-"),
        just(Token::Star).to("*"),
        just(Token::Slash).to("/"),
        just(Token::Percent).to("%"),
        just(Token::DoubleEq).to("=="),
        just(Token::NotEq).to("!="),
        just(Token::Lte).to("<="),
        just(Token::Gte).to(">="),
        just(Token::Lt).to("<"),
        just(Token::Gt).to(">"),
        just(Token::TripleEq).to("==="),
        just(Token::ExclDoubleEq).to("!=="),
        just(Token::Tilde).to("~"),
        just(Token::PlusPlus).to("++"),
        just(Token::LeftArrow).to("<-"),
        just(Token::And).to("and"),
        just(Token::Or).to("or"),
        just(Token::In).to("in"),
    ));

    let paren_op = op_sym.clone().delimited_by(just(Token::LParen), just(Token::RParen));
    let name_sym = select! { Token::Ident(name) => name }.or(paren_op);

    let constraint = constraint_parser();

    let inst_context = constraint
        .clone()
        .separated_by(just(Token::Comma))
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen))
        .then_ignore(just(Token::FatArrow));

    let qual_type = qual_type_parser();

    let fundep = select! { Token::Ident(name) => name }
        .repeated()
        .at_least(1)
        .collect::<Vec<_>>()
        .then_ignore(just(Token::Arrow))
        .then(
            select! { Token::Ident(name) => name }
                .repeated()
                .at_least(1)
                .collect::<Vec<_>>(),
        );

    let fundeps = just(Token::Pipe).ignore_then(
        fundep
            .separated_by(just(Token::Comma))
            .collect::<Vec<_>>(),
    );

    let cmember = name_sym
        .clone()
        .then_ignore(just(Token::DoubleColon))
        .then(qual_type)
        .map(|(name, ty)| VarTypeDef { name, ty });

    let class_def = just(Token::Class)
        .ignore_then(inst_context.clone().or_not())
        .then(select! { Token::Ident(name) => name })
        .then(select! { Token::Ident(name) => name }.repeated().collect::<Vec<_>>())
        .then(fundeps.or_not())
        .then_ignore(just(Token::Where))
        .then(cmember.clone().repeated().collect::<Vec<_>>())
        .map(|((((ctx_opt, name), params), fds), members)| {
            ModuleDef::Class(ClassDef {
                context: ctx_opt.unwrap_or_default(),
                name,
                params,
                fundeps: fds.unwrap_or_default(),
                members,
            })
        });

    let data_def = just(Token::Data)
        .ignore_then(select! { Token::Ident(name) => name })
        .then(select! { Token::Ident(name) => name }.repeated().collect::<Vec<_>>())
        .then_ignore(just(Token::Eq))
        .then(type_expr.clone())
        .map(|((name, params), ty)| {
            ModuleDef::Data(DataDef { name, params, ty })
        });

    let op_member = select! { Token::Ident(lhs) => lhs }
        .then(op_sym)
        .then(select! { Token::Ident(rhs) => rhs })
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .map(|(((lhs, op), rhs), body)| VarDef {
            name: op,
            args: vec![lhs, rhs],
            body,
        });

    let fn_arg = choice((
        select! { Token::Ident(arg) => arg },
        just(Token::Underscore).to("_"),
    ));
    let fn_member = name_sym
        .then(fn_arg.repeated().collect::<Vec<_>>())
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .map(|((name, args), body)| VarDef { name, args, body });

    let imember = choice((op_member.clone(), fn_member.clone()));

    let instance_def = just(Token::Instance)
        .ignore_then(inst_context.or_not())
        .then(select! { Token::Ident(name) => name })
        .then(type_expr.clone().repeated().collect::<Vec<_>>())
        .then_ignore(just(Token::Where))
        .then(imember.repeated().collect::<Vec<_>>())
        .map(|(((ctx_opt, class_name), types), members)| {
            ModuleDef::Instance(InstanceDef {
                context: ctx_opt.unwrap_or_default(),
                class_name,
                types,
                members,
            })
        });

    let top_var_type = cmember.map(ModuleDef::VarType);
    let top_var_def = choice((op_member, fn_member)).map(ModuleDef::VarDef);

    let module_def = choice((class_def, instance_def, data_def, top_var_type, top_var_def));

    let module_header = just(Token::Module)
        .ignore_then(select! { Token::Ident(name) => name })
        .then_ignore(just(Token::Where));

    module_header
        .or_not()
        .then(module_def.repeated().collect::<Vec<_>>())
        .map(|(name, defs)| Module { name, defs })
}

pub fn parse_expr<'ctx>(ctx: &'ctx TypeContext, src: &'ctx str) -> Result<&'ctx Expr<'ctx>, String> {
    use logos::Logos;
    let lex = Token::lexer(src);
    let token_iter = lex.spanned().map(|(tok, span)| match tok {
        Ok(t) => Ok((t, span)),
        Err(e) => Err((e, span)),
    });

    let mut tokens = Vec::new();
    for t in token_iter {
        tokens.push(t.map_err(|(e, span)| format!("Lex error at {:?}: {:?}", span, e))?);
    }

    let eof = src.len()..src.len();
    let token_stream = chumsky::input::Stream::from_iter(tokens).map(eof, |(t, s)| (t, s));

    let parser = expr_parser(ctx);
    match parser.parse(token_stream).into_result() {
        Ok(e) => Ok(e),
        Err(errs) => Err(format!("Parse error: {:?}", errs)),
    }
}

pub fn parse_module<'ctx>(ctx: &'ctx TypeContext, src: &'ctx str) -> Result<Module<'ctx>, String> {
    use logos::Logos;
    let lex = Token::lexer(src);
    let token_iter = lex.spanned().map(|(tok, span)| match tok {
        Ok(t) => Ok((t, span)),
        Err(e) => Err((e, span)),
    });

    let mut tokens = Vec::new();
    for t in token_iter {
        tokens.push(t.map_err(|(e, span)| format!("Lex error at {:?}: {:?}", span, e))?);
    }

    let eof = src.len()..src.len();
    let token_stream = chumsky::input::Stream::from_iter(tokens).map(eof, |(t, s)| (t, s));

    let parser = module_parser(ctx);
    match parser.parse(token_stream).into_result() {
        Ok(m) => Ok(m),
        Err(errs) => Err(format!("Parse error: {:?}", errs)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_to_lower() {
        let ctx = TypeContext::new();
        let src = r#"
toLower :: char -> char
toLower c = if (c >= 'A' and c <= 'Z') then ((c - 'A') + 'a') else c
"#;
        let module = parse_module(&ctx, src).expect("failed to parse toLower module");
        assert_eq!(module.defs.len(), 2);
    }

    #[test]
    fn test_parse_lcase_sig() {
        let ctx = TypeContext::new();
        let src = r#"
toLower :: char -> char
toLower c = if (c >= 'A' and c <= 'Z') then ((c - 'A') + 'a') else c

lcase :: (Array cs char) => cs -> [char]
lcase cs = map(toLower, cs[0:])
"#;
        let module = parse_module(&ctx, src).expect("failed to parse lcase");
        assert_eq!(module.defs.len(), 4);
    }

    #[test]
    fn test_parse_strings_hob() {
        let ctx = TypeContext::new();
        let src = include_str!("../../calvin-boot/boot/strings.hob");
        let module = parse_module(&ctx, src).expect("failed to parse strings.hob");
        assert_eq!(module.defs.len(), 9);
    }

    #[test]
    fn test_parse_amapping_hob() {
        let ctx = TypeContext::new();
        let src = include_str!("../../calvin-boot/boot/amapping.hob");
        let res = parse_module(&ctx, src);
        if let Err(e) = res {
            panic!("parse error on amapping.hob: {}", e);
        }
    }

    #[test]
    fn test_parse_convert_hob() {
        let ctx = TypeContext::new();
        let src = include_str!("../../calvin-boot/boot/convert.hob");
        let res = parse_module(&ctx, src);
        if let Err(e) = res {
            panic!("parse error on convert.hob: {}", e);
        }
    }
}

