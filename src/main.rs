use bumpalo::Bump;
use lexora::ast::{ExprId, Program, Type};
use lexora::error::LexoraError;
use lexora::lexer::{Lexer, Token};
use lexora::parser::Parser;
use lexora::source_map::SourceMap;
use lexora::span::Span;
use lexora::symbol::{Interner, Symbol};
use lexora::typechecker::TypeChecker;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
fn main() {
    let args: Vec<String> = env::args().collect();
    let mut file: Option<String> = None;
    let mut backend = String::from("string-ir");
    let mut opt: u8 = 0;
    let mut debug = false;
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
            "-g" => debug = true,
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

    let color = std::io::stderr().is_terminal();

    let arena = Bump::new();
    let mut sources = SourceMap::new();
    let (program, interner, errors) = match load_program(&path, &arena, &mut sources) {
        Ok(v) => v,
        Err(e) => report(&sources, e, color),
    };

    if !errors.is_empty() {
        report(&sources, errors, color);
    }

    let mut checker = TypeChecker::new(&interner);
    if let Err(type_errors) = checker.check_program(&program) {
        report(&sources, type_errors, color);
    }
    let (moves, move_errors) =
        lexora::move_check::MoveChecker::new(&checker.types, &interner).check(&program);
    if !move_errors.is_empty() {
        report(&sources, move_errors, color);
    }
    println!("Ok: tip kontrolu basarili.");

    match backend.as_str() {
        "string-ir" => run_string_ir(
            &interner,
            &program,
            &checker.types,
            &moves,
            &checker.instances,
            &checker.fn_instances,
            &checker.call_targs,
        ),
        "inkwell" => run_inkwell(
            &interner,
            &program,
            &checker.types,
            &sources,
            opt,
            debug,
            &moves,
            &checker.instances,
            &checker.fn_instances,
            &checker.call_targs,
        ),
        other => {
            eprintln!("bilinmeyen backend: '{}' (string-ir veya inkwell)", other);
            std::process::exit(1);
        }
    }
}
const ERROR_LIMIT: usize = 100;
fn report(sources: &SourceMap, mut errors: Vec<LexoraError>, color: bool) -> ! {
    errors.sort_by_key(|e| e.span().map(|s| s.start).unwrap_or(u32::MAX));
    let total = errors.len();
    for e in errors.iter().take(ERROR_LIMIT) {
        eprint!(
            "{}",
            lexora::diagnostic::render(sources, &lexora::diagnostic::to_diagnostic(e), color)
        );
    }
    eprint!(
        "{}",
        lexora::diagnostic::render_summary(total, ERROR_LIMIT, color)
    );
    std::process::exit(1);
}
fn load_program<'arena>(
    entry: &str,
    arena: &'arena Bump,
    sources: &mut SourceMap<'arena>,
) -> Result<(Program<'arena>, Interner, Vec<LexoraError>), Vec<LexoraError>> {
    let entry_path = PathBuf::from(entry);
    let entry_src = arena.alloc_str(&read_source(&entry_path).map_err(|e| vec![e])?);
    let base = sources.add(entry_path.display().to_string(), entry_src);
    let mut pre_interner = Interner::new();
    let enum_names = collect_enum_names(&entry_path, &mut pre_interner);
    let mut parser = Parser::with_interner(Lexer::new(entry_src, base), arena, pre_interner, 0);
    parser.seed_enum_names(enum_names.clone());
    let mut program = parser.parse_program();
    let mut errors: Vec<LexoraError> = parser.take_errors();
    let mut interner = parser.interner;
    let mut next_id = parser.next_expr_id;

    let mut visited: HashSet<PathBuf> = HashSet::new();
    visited.insert(canonical(&entry_path));

    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    enqueue_imports(&mut queue, &entry_path, &program.imports);

    while let Some(path) = queue.pop_front() {
        if !visited.insert(canonical(&path)) {
            continue;
        }
        let src = match read_source(&path) {
            Ok(s) => arena.alloc_str(&s),
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let base = sources.add(path.display().to_string(), src);
        let mut sub = Parser::with_interner(Lexer::new(src, base), arena, interner, next_id);
        sub.seed_enum_names(enum_names.clone());
        let sub_program = sub.parse_program();
        errors.extend(sub.take_errors());
        enqueue_imports(&mut queue, &path, &sub_program.imports);
        program.functions.extend(sub_program.functions);
        program.structs.extend(sub_program.structs);
        program.enums.extend(sub_program.enums);
        program.impls.extend(sub_program.impls);
        interner = sub.interner;
        next_id = sub.next_expr_id;
    }

    Ok((program, interner, errors))
}

fn read_source(path: &Path) -> Result<String, LexoraError> {
    fs::read_to_string(path).map_err(|e| LexoraError::Custom {
        message: format!("dosya okunamadi: '{}': {}", path.display(), e),
        span: Span::default(),
    })
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn enqueue_imports(queue: &mut VecDeque<PathBuf>, importer: &Path, imports: &[&str]) {
    let base = importer.parent().unwrap_or_else(|| Path::new(""));
    for imp in imports.iter().copied() {
        queue.push_back(base.join(imp));
    }
}
fn collect_enum_names(entry: &Path, interner: &mut Interner) -> HashSet<Symbol> {
    let mut names: HashSet<Symbol> = HashSet::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(entry.to_path_buf());

    while let Some(path) = queue.pop_front() {
        if !visited.insert(canonical(&path)) {
            continue;
        }
        let src = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut lexer = Lexer::new(&src, 0);
        let mut after_enum = false;
        let mut after_import = false;
        loop {
            let t = lexer.next_token();
            match t.token {
                Token::Eof => break,
                Token::Enum => {
                    after_enum = true;
                    after_import = false;
                }
                Token::Import => {
                    after_import = true;
                    after_enum = false;
                }
                Token::Identifier(s) => {
                    if after_enum {
                        names.insert(interner.intern(s));
                    }
                    after_enum = false;
                    after_import = false;
                }
                Token::StringLiteral(s) => {
                    if after_import {
                        let base = path.parent().unwrap_or_else(|| Path::new(""));
                        queue.push_back(base.join(s));
                    }
                    after_enum = false;
                    after_import = false;
                }
                _ => {
                    after_enum = false;
                    after_import = false;
                }
            }
        }
    }
    names
}
fn run_string_ir(
    interner: &Interner,
    program: &Program,
    types: &HashMap<ExprId, Type>,
    moves:  &HashMap<ExprId, Vec<u32>>,
    instances: &HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
    fn_instances: &HashMap<(Symbol, Vec<Type>), (Vec<Type>, Type)>,
    call_targs: &HashMap<ExprId, Vec<Type>>,
) {
    use lexora::backend::string_ir::builder::IrBuilder;
    use lexora::backend::string_ir::codegen::CodeGen;
    let ir_builder = IrBuilder::new(interner);
    let mut codegen = CodeGen::new(
        ir_builder,
        types,
        moves,
        instances,
        fn_instances,
        call_targs,
    );
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
fn run_inkwell(
    interner: &Interner,
    program: &Program,
    types: &HashMap<ExprId, Type>,
    sources: &SourceMap,
    opt: u8,
    debug: bool,
    moves:  &HashMap<ExprId, Vec<u32>>,
    instances: &HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
    fn_instances: &HashMap<(Symbol, Vec<Type>), (Vec<Type>, Type)>,
    call_targs: &HashMap<ExprId, Vec<Type>>,
) {
    use inkwell::context::Context;
    use lexora::backend::inkwell::codegen::CodeGen;

    let context = Context::create();
    let mut codegen = CodeGen::new(
        &context,
        interner,
        "lexora_module",
        types,
        sources,
        opt,
        debug,
        moves,
        instances,
        fn_instances,
        call_targs,
    );
    if let Err(e) = codegen.compile(program) {
        eprintln!(
            "{}",
            LexoraError::Codegen {
                message: e.to_string()
            }
        );
        std::process::exit(1);
    }
    if let Err(e) = codegen.verify() {
        eprintln!(
            "{}",
            LexoraError::Codegen {
                message: format!("module.verify: {}", e)
            }
        );
        std::process::exit(1);
    }
    if let Err(e) = codegen.optimize() {
        eprintln!(
            "{}",
            LexoraError::Codegen {
                message: format!("optimize: {}", e)
            }
        );
        std::process::exit(1);
    }
    if let Err(e) = codegen.write_ir(Path::new("output.ll")) {
        eprintln!(
            "{}",
            LexoraError::Codegen {
                message: format!("ir dump: {}", e)
            }
        );
        std::process::exit(1);
    }
    if let Err(e) = codegen.emit_object(Path::new("output.o")) {
        eprintln!(
            "{}",
            LexoraError::Codegen {
                message: format!("object emit: {}", e)
            }
        );
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
fn run_inkwell(
    _interner: &Interner,
    _program: &Program,
    _types: &HashMap<ExprId, Type>,
    _sources: &SourceMap,
    _opt: u8,
    _debug: bool,
    _moves: &HashMap<ExprId, Vec<u32>>,
    _instances: &HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
    _fn_instances: &HashMap<(Symbol, Vec<Type>), (Vec<Type>, Type)>,
    _call_targs: &HashMap<ExprId, Vec<Type>>,
) {
    eprintln!("inkwell backend bu binary'de derlenmemis; `cargo run --features inkwell` ile derle");
    std::process::exit(1);
}

