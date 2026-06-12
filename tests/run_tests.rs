use bumpalo::Bump;
use lexora::lexer::Lexer;
use lexora::parser::Parser;
use lexora::typechecker::TypeChecker;

fn check(source: &str) -> Result<(), String> {
    let arena = Bump::new();
    let lexer = Lexer::new(source, 0);
    let mut parser = Parser::new(lexer, &arena).map_err(|e| e.to_string())?;
    let program = parser.parse_program().map_err(|e| e.to_string())?;
    let interner = &parser.interner;
    let mut checker = TypeChecker::new(interner);
    checker.check_program(&program).map_err(|e| e.to_string())
}

#[test]
fn test_arithmetic() {
    assert!(check(include_str!("cases/arithmetic.lx")).is_ok());
}

#[test]
fn test_structs() {
    assert!(check(include_str!("cases/structs.lx")).is_ok());
}

#[test]
fn test_loops() {
    assert!(check(include_str!("cases/loops.lx")).is_ok());
}

#[test]
fn test_type_error() {
    let result = check(include_str!("cases/type_error.lx"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("beklenen"));
}