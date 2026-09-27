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

        let variant_payload = select! { Token::Ident(name) => name }
            .then_ignore(just(Token::Colon).or(just(Token::Eq)))
            .then(pat.clone())
            .map(move |(tag, payload)| Pattern::Variant(tag, &*ctx.alloc(payload)));

        let variant_unit = select! { Token::Ident(name) => name }
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

        let variant_payload = select! { Token::Ident(name) => name }
            .then_ignore(just(Token::Colon).or(just(Token::Eq)))
            .then(expr.clone())
            .map(move |(tag, payload)| &*ctx.alloc(Expr::Variant(tag, payload)));

        let variant_unit = select! { Token::Ident(name) => name }
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
            tuple_or_paren,
        ));

        let atom_annotated = atom
            .then(just(Token::DoubleColon).ignore_then(type_expr_parser()).or_not())
            .map(move |(e, ty_opt)| match ty_opt {
                Some(te) => &*ctx.alloc(Expr::Annotate(e, lower_type_expr(ctx, &te))),
                None => e,
            });

        let app = atom_annotated
            .clone()
            .then(atom_annotated.clone().repeated().collect::<Vec<_>>())
            .map(move |(f, args)| {
                if args.is_empty() {
                    f
                } else {
                    let args_slice = ctx.alloc_slice_clone(&args);
                    &*ctx.alloc(Expr::App(f, args_slice))
                }
            });

        let op_mul = app.clone().foldl(
            choice((
                just(Token::Star).to(Token::Star),
                just(Token::Slash).to(Token::Slash),
                just(Token::Percent).to(Token::Percent),
            ))
            .then(app.clone())
            .repeated(),
            move |lhs, (op, rhs)| {
                let op_name = match op {
                    Token::Star => "*",
                    Token::Slash => "/",
                    Token::Percent => "%",
                    _ => unreachable!(),
                };
                &*ctx.alloc(Expr::App(
                    &*ctx.alloc(Expr::Var(op_name)),
                    ctx.alloc_slice_clone(&[lhs, rhs]),
                ))
            },
        );

        let op_add = op_mul.clone().foldl(
            choice((
                just(Token::Plus).to(Token::Plus),
                just(Token::Minus).to(Token::Minus),
            ))
            .then(op_mul.clone())
            .repeated(),
            move |lhs, (op, rhs)| {
                let op_name = match op {
                    Token::Plus => "+",
                    Token::Minus => "-",
                    _ => unreachable!(),
                };
                &*ctx.alloc(Expr::App(
                    &*ctx.alloc(Expr::Var(op_name)),
                    ctx.alloc_slice_clone(&[lhs, rhs]),
                ))
            },
        );

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

        choice((let_bind, lambda, match_expr, op_add))
            .then(just(Token::DoubleColon).ignore_then(type_expr_parser()).or_not())
            .map(move |(e, ty_opt)| match ty_opt {
                Some(te) => &*ctx.alloc(Expr::Annotate(e, lower_type_expr(ctx, &te))),
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
    }
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

        let var = select! { Token::Ident(name) => TypeExpr::Var(name) };

        let tuple_or_paren = ty
            .clone()
            .separated_by(just(Token::Comma))
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

        let atom = choice((prim, var, tuple_or_paren));

        atom.clone()
            .then(just(Token::Arrow).ignore_then(ty.clone()).or_not())
            .map(|(lhs, rhs)| match rhs {
                Some(r) => TypeExpr::Fn(Box::new(lhs), Box::new(r)),
                None => lhs,
            })
    })
}

pub fn module_parser<'a, 'ctx, I>(
    ctx: &'ctx TypeContext,
) -> impl Parser<'a, I, Module<'ctx>, ParseError<'a>> + Clone
where
    'a: 'ctx,
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
    ));

    let paren_op = op_sym.clone().delimited_by(just(Token::LParen), just(Token::RParen));
    let name_sym = select! { Token::Ident(name) => name }.or(paren_op);

    let not_eq = type_expr
        .clone()
        .then_ignore(just(Token::NotEq))
        .then(type_expr.clone())
        .map(|(t1, t2)| TypeConstraint::NotEq(t1, t2));

    let class_cst = select! { Token::Ident(name) => name }
        .then(type_expr.clone().repeated().at_least(1).collect::<Vec<_>>())
        .map(|(name, args)| TypeConstraint::Class(name, args));

    let constraint = choice((not_eq, class_cst));

    let context = constraint
        .clone()
        .separated_by(just(Token::Comma))
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen))
        .then_ignore(just(Token::FatArrow));

    let qual_type = context
        .or_not()
        .then(type_expr.clone())
        .map(|(ctx_opt, ty)| QualTypeExpr {
            context: ctx_opt.unwrap_or_default(),
            ty,
        });

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
        .ignore_then(select! { Token::Ident(name) => name })
        .then(select! { Token::Ident(name) => name }.repeated().collect::<Vec<_>>())
        .then(fundeps.or_not())
        .then_ignore(just(Token::Where))
        .then(cmember.repeated().collect::<Vec<_>>())
        .map(|(((name, params), fds), members)| {
            ModuleDef::Class(ClassDef {
                name,
                params,
                fundeps: fds.unwrap_or_default(),
                members,
            })
        });

    let inst_context = constraint
        .clone()
        .separated_by(just(Token::Comma))
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen))
        .then_ignore(just(Token::FatArrow));

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

    let fn_member = name_sym
        .then(select! { Token::Ident(arg) => arg }.repeated().collect::<Vec<_>>())
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .map(|((name, args), body)| VarDef { name, args, body });

    let imember = choice((op_member, fn_member));

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

    let module_def = choice((class_def, instance_def));

    module_def.repeated().collect::<Vec<_>>().map(|defs| Module {
        name: None,
        defs,
    })
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
