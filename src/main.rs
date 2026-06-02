use bumpalo::Bump;
use lexora::lexer::Lexer;
use lexora::parser::Parser;
use lexora::typechecker::TypeChecker;
use lexora::ast::Program;
use lexora::symbol::Interner;
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut file: Option<String> = None;
    let mut backend = String::from("string-ir");
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--backend" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--backend bir deger bekliyor (string-ir veya inkwell)");
                    std::process::exit(1);
                }
                backend = args[i].clone();
            }
            other => file = Some(other.to_string()),
        }
        i += 1;
    }
    let path = match file {
        Some(p) => p,
        None => {
            eprintln!("kullanım: lexora [--backend string-ir|inkwell] <dosya.lx>");
            std::process::exit(1);
        }
    };

    let source = fs::read_to_string(&path).unwrap_or_else(|e| {
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
    let program = match parser.parse_program() {
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

    match backend.as_str() {
        "string-ir" => run_string_ir(interner, &program),
        "inkwell" => run_inkwell(interner, &program),
        other => {
            eprintln!("bilinmeyen backend: '{}' (string-ir veya inkwell)", other);
            std::process::exit(1);
        }
    }
}

fn run_string_ir(interner: &Interner, program: &Program) {
    use lexora::backend::string_ir::builder::IrBuilder;
    use lexora::backend::string_ir::codegen::CodeGen;
    let ir_builder = IrBuilder::new(interner);
    let mut codegen = CodeGen::new(ir_builder);
    let ir = match codegen.gen_program(program) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    fs::write("output.ll", &ir).unwrap_or_else(|e| {
        eprintln!("output.ll yazılamadı: {}", e);
        std::process::exit(1);
    });
    println!("output.ll üretildi.");
}

#[cfg(feature = "inkwell")]
fn run_inkwell(interner: &Interner, program: &Program) {
    use inkwell::context::Context;
    use lexora::backend::inkwell::codegen::CodeGen;
    use lexora::error::LexoraError;
    let context = Context::create();
    let mut codegen = CodeGen::new(&context, interner, "lexora_module");
    if let Err(e) = codegen.compile(program) {
        eprintln!("{}", LexoraError::Codegen { message: e.to_string() });
        std::process::exit(1);
    }
    codegen.print_ir();
}

#[cfg(not(feature = "inkwell"))]
fn run_inkwell(_interner: &Interner, _program: &Program) {
    eprintln!("inkwell backend bu binary'de derlenmemis; `cargo run --features inkwell` ile derle");
    std::process::exit(1);
}