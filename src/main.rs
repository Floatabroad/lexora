mod lexer;
mod ast;
mod parser;
mod typechecker;
mod codegen;

//use std::os::unix::raw::off_t;
use lexer::Lexer;
use parser::Parser;
use typechecker::TypeChecker;
use codegen::CodeGen;
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Kullanim: lexora <dosya.lx>");
        std::process::exit(1);
    }
    let source = fs::read_to_string(&args[1])
        .unwrap_or_else(|_| panic!("Dosya okunamadi: {}", args[1]));
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| {
        let lexer = Lexer::new(&source);
        let mut parser = Parser::new(lexer);
        let mut program = parser.parse_program();

        for import_path in &program.imports.clone() {
            let import_source = fs::read_to_string(import_path)
                .unwrap_or_else(|_| panic!("Import dosyasi okunamadi: {}", import_path));
            let import_lexer = Lexer::new(&import_source);
            let mut import_parser = Parser::new(import_lexer);
            let import_program = import_parser.parse_program();
            for func in import_program.functions {
                program.functions.push(func);
            }
        }

        let mut checker = TypeChecker::new();
        checker.check_program(&program);

        let mut codegen = CodeGen::new();
        codegen.generate(&program)
    });

    match result {
        Ok(ir) => {
            fs::write("output.ll", &ir).unwrap();
            println!("LLVM IR yazildi: output.ll");
            println!("---");
            println!("{}", ir);
        }
        Err(e) => {
            if let Some(msg) = e.downcast_ref::<String>() {
                eprintln!("{}", msg);
            } else if let Some(msg) = e.downcast_ref::<&str>() {
                eprintln!("{}", msg);
            } else {
                eprintln!("Bilinmeyen hata");
            }
            std::process::exit(1);
        }
    }
}