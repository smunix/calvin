use calvin_core::context::TypeContext;
use clap::Parser as ClapParser;
use colored::*;
use reedline::{DefaultPrompt, Reedline, Signal};
use std::io::IsTerminal;

mod compiler;
use compiler::{BackendChoice, Compiler};

#[derive(ClapParser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "cranelift")]
    backend: String,
}

fn main() {
    let args = Args::parse();

    let backend = match args.backend.to_lowercase().as_str() {
        "cranelift" => BackendChoice::Cranelift,
        "llvm" => BackendChoice::Llvm,
        _ => {
            eprintln!("{}", "Error: backend must be 'cranelift' or 'llvm'".red());
            std::process::exit(1);
        }
    };

    println!(
        "{} v0.1.0 (Backend: {})",
        "Calvin Interactive".green().bold(),
        args.backend.cyan()
    );
    println!("Type ':h' for help, or ':q' to quit.");

    let ctx = TypeContext::new();
    let mut compiler = Compiler::new(&ctx, backend);

    if std::io::stdin().is_terminal() {
        let mut line_editor = Reedline::create();
        let prompt = DefaultPrompt::default();

        loop {
            let sig = line_editor.read_line(&prompt);
            match sig {
                Ok(Signal::Success(buffer)) => {
                    if !process_line(&buffer, &mut compiler) {
                        break;
                    }
                }
                Ok(Signal::CtrlD) | Ok(Signal::CtrlC) => break,
                _ => {}
            }
        }
    } else {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(buffer) => {
                    if !process_line(&buffer, &mut compiler) {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }
}

fn process_line(input: &str, compiler: &mut Compiler) -> bool {
    let input = input.trim();
    if input.is_empty() {
        return true;
    }

    if input.starts_with(':') {
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = if parts.len() > 1 { parts[1] } else { "" };

        match cmd {
            ":q" => return false,
            ":h" => {
                println!("Commands:");
                println!("  :q            Quit");
                println!("  :h            Help");
                println!("  :t <expr>     Print the inferred type of <expr>");
                println!("  :u <expr>     Print the unsweetened AST of <expr>");
                println!("  :a <expr>     Dump the Cranelift/Llvm IR for <expr>");
                println!(
                    "  :x <expr>     Disassemble the JIT-compiled native machine code for <expr>"
                );
                println!("  :l <path>     Load a script or fregion structured data file");
            }
            ":t" => match compiler.type_of(arg) {
                Ok(t) => println!("{}", t.cyan()),
                Err(e) => eprintln!("{}", e.red()),
            },
            ":u" => match compiler.parse(arg) {
                Ok(ast) => println!("{}", ast.blue()),
                Err(e) => eprintln!("{}", e.red()),
            },
            ":a" => match compiler.dump_ir(arg) {
                Ok(ir) => println!("{}", ir.magenta()),
                Err(e) => eprintln!("{}", e.red()),
            },
            ":x" => match compiler.disassemble(arg) {
                Ok(asm) => println!("{}", asm.yellow()),
                Err(e) => eprintln!("{}", e.red()),
            },
            ":l" => {
                if arg.is_empty() {
                    eprintln!("{}", "Error: missing file path".red());
                } else if arg.ends_with(".log") || arg.ends_with(".fregion") {
                    match calvin_storage::fregion::FRegion::open(arg) {
                        Ok(f) => {
                            println!("{}", format!("Loaded {} successfully", arg).green());
                            let header = f.header();
                            println!(
                                "FRegion: magic={:#010x}, version={}, capacity={} bytes",
                                header.magic, header.version, header.size
                            );
                        }
                        Err(e) => eprintln!("{}", e.to_string().red()),
                    }
                } else {
                    eprintln!(
                        "{}",
                        "Script loading not yet fully integrated; only .log supported currently."
                            .yellow()
                    );
                }
            }
            _ => println!("{}", format!("Unknown command: {}", cmd).yellow()),
        }
        return true;
    }

    match compiler.eval_dynamic(input) {
        Ok(val) => println!("{}", val.yellow().bold()),
        Err(e) => eprintln!("{}", e.red()),
    }
    true
}
