use ariadne::{Color, Label, Report, ReportKind, Source};
use std::ops::Range;

/// Print a parser error nicely using ariadne
pub fn report_error(filename: &str, src: &str, span: Range<usize>, msg: &str) {
    Report::build(ReportKind::Error, filename, span.start)
        .with_message(msg)
        .with_label(
            Label::new((filename, span))
                .with_message("Here")
                .with_color(Color::Red),
        )
        .finish()
        .eprint((filename, Source::from(src)))
        .unwrap();
}
