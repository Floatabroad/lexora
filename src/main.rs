use bumpalo::Bump;
use lexora::lexer::Lexer;
use lexora::parser::Parser;
use lexora::typechecker::TypeChecker;
use lexora::ast::Program;
use lexora::symbol::Interner;
use lexora::error::LexoraError;
use lexora::span::Span;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut file: Option<String> = None;
    let mut backend = String::from("string-ir");
    let mut opt: u8 = 0;
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
            "-O0" => opt = 0,
            "-O1" => opt = 1,
            "-O2" => opt = 2,
            "-O3" => opt = 3,
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

    let arena = Bump::new();
    let (program, interner) = match load_program(&path, &arena) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let mut checker = TypeChecker::new(&interner);
    if let Err(e) = checker.check_program(&program) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    println!("Ok: tip kontrolu basarili.");

    match backend.as_str() {
        "string-ir" => run_string_ir(&interner, &program),
        "inkwell" => run_inkwell(&interner, &program, opt),
        other => {
            eprintln!("bilinmeyen backend: '{}' (string-ir veya inkwell)", other);
            std::process::exit(1);
        }
    }
}
fn load_program<'arena> (
    entry: &str,
    arena: &'arena Bump,
) -> Result<(Program<'arena>, Interner), LexoraError> {
    let entry_path = PathBuf::from(entry);
    let entry_src = read_source(&entry_path)?;
    let mut parser = Parser::new(Lexer::new(&entry_src), arena)?;
    let mut program = parser.parse_program()?;
    let mut interner = parser.interner;

    let mut visited: HashSet<PathBuf> = HashSet::new();
    visited.insert(canonical(&entry_path));

    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    enqueue_imports(&mut queue, &entry_path, &program.imports);

    while let Some(path) = queue.pop_front() {
        if !visited.insert(canonical(&path)){
            continue;
        }
        let src = read_source(&path)?;
        let mut sub = Parser::with_interner(Lexer::new(&src), arena, interner)?;
        let sub_program = sub.parse_program()?;
        interner = sub.interner;

        enqueue_imports(&mut queue, &path, &sub_program.imports);
        program.functions.extend(sub_program.functions);
        program.structs.extend(sub_program.structs);
    }
    Ok((program, interner))
}

fn read_source(path: &Path) -> Result<String, LexoraError> {
    fs::read_to_string(path).map_err(|e| LexoraError::Custom{
        message: format!("dosya okunamadi: '{}': {}", path.display(), e),
        span: Span::default(),
    })
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn enqueue_imports(queue: &mut VecDeque<PathBuf>, importer: &Path, imports: &[&str]) {
    let base = importer.parent().unwrap_or_else(|| Path::new(""));
    for imp in imports.iter().copied(){
        queue.push_back(base.join(imp));
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
fn run_inkwell(interner: &Interner, program: &Program, opt: u8) {
    use inkwell::context::Context;
    use lexora::backend::inkwell::codegen::CodeGen;

    let context = Context::create();
    let mut codegen = CodeGen::new(&context, interner, "lexora_module", opt);
    if let Err(e) = codegen.compile(program) {
        eprintln!("{}", LexoraError::Codegen { message: e.to_string() });
        std::process::exit(1);
    }
    if let Err(e) = codegen.verify() {
        eprintln!("{}", LexoraError::Codegen { message: format!("module.verify: {}", e)
        });
        std::process::exit(1);
    }
    if let Err(e) = codegen.optimize() {
        eprintln!("{}", LexoraError::Codegen { message: format!("optimize: {}", e) });
        std::process::exit(1);
    }
    if let Err(e) = codegen.write_ir(Path::new("output.ll")) {
        eprintln!("{}", LexoraError::Codegen { message: format!("ir dump: {}", e) });
        std::process::exit(1);
    }
    if let Err(e) = codegen.emit_object(Path::new("output.o")) {
        eprintln!("{}", LexoraError::Codegen { message: format!("object emit: {}", e)
        });
        std::process::exit(1);
    }
    let status = std::process::Command::new("cc")
        .arg("output.o")
        .arg("-o")
        .arg("output")
        .status();
    match status {
        Ok(s) if s.success() => println!("output (binary) uretildi."),
        Ok(s) => {
            eprintln!("link basarisiz: cc exit {}", s);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("cc calistirilamadi: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(not(feature = "inkwell"))]
fn run_inkwell(_interner: &Interner, _program: &Program, _opt: u8) {
    eprintln!("inkwell backend bu binary'de derlenmemis; `cargo run --features inkwell` ile derle");
    std::process::exit(1);
}