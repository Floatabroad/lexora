use bumpalo::Bump;
use lexora::lexer::Lexer;
use lexora::parser::Parser;
use lexora::typechecker::TypeChecker;
use std::env;
use std::fs;

use inkwell::context::Context;
use lexora::codegen::CodeGen;
use lexora::error::LexoraError;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("kullanım: lexora <dosya.lx>");
        std::process::exit(1);
    }
    let source = fs::read_to_string(&args[1])
        .unwrap_or_else(|e| {
            eprintln!("dosya okunamadı: {}", e);
            std::process::exit(1);
        });
    let arena = Bump::new();
    let lexer = Lexer::new(&source);

    let mut parser = match Parser::new(lexer, &arena) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let program = match parser.parse_program(){
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let interner = &parser.interner;
    let mut checker = TypeChecker::new(interner);
    if let Err(e) = checker.check_program(&program) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    println!("Ok: tip kontrolu basarili.");

    // ---- CodeGen (inkwell) ----
    let context = Context::create();
    let mut codegen = CodeGen::new(&context, interner, "lexora_module");
    if let Err(e) = codegen.compile(&program) {
        eprintln!("{}", LexoraError::Codegen { message: e.to_string() });
        std::process::exit(1);
    }
    codegen.print_ir();

}