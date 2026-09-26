#![no_main]
use libfuzzer_sys::fuzz_target;
use calvin_core::context::TypeContext;
use calvin_core::lang::typeinf::TypeInference;
use calvin_core::lang::expr::ExprVisitor;
use calvin_parse::lexer::Token;
use chumsky::Parser;
use chumsky::input::{Stream, Input};
use logos::Logos;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let ctx = TypeContext::new();
        let src: &str = ctx.arena().alloc_str(s);
        
        let token_iter = Token::lexer(src)
            .spanned()
            .map(|(tok, span)| match tok {
                Ok(t) => Ok((t, span)),
                Err(e) => Err((e, span)),
            });
            
        let mut tokens = Vec::new();
        for t in token_iter {
            if let Ok(t) = t { tokens.push(t); } else { return; }
        }
        
        let eof = src.len()..src.len();
        let token_stream = Stream::from_iter(tokens.into_iter()).map(eof, |(t, s)| (t, s));

        let res = calvin_parse::parser::expr_parser(&ctx).parse(token_stream).into_result();
        if let Ok(ast) = res {
            let mut type_inf = TypeInference::new(&ctx);
            let _ = type_inf.visit(ast);
        }
    }
});
