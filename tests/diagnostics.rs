use std::path::Path;
use std::process::Command;

fn run_case(name: &str) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let bin = env!("CARGO_BIN_EXE_lexora");
    let lx = format!("tests/diagnostics/{}.lx", name);
    let golden_path = Path::new(manifest).join(format!("tests/diagnostics/{}.stderr", name));

    let output = Command::new(bin)
        .arg(&lx)
        .current_dir(manifest)
        .output()
        .expect("lexora calistirilamadi");

    assert_eq!(
        output.status.code(),
        Some(1),
        "{}: tanilama cikti kodu 1 olmali",
        name
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let golden = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|_| panic!("golden yok: {} (once `make diag-bless`)", name));

    assert_eq!(
        stderr.trim_end(),
        golden.trim_end(),
        "{}: stderr golden ile eslesmiyor (`make diag-bless` ile guncelle)",
        name
    );
}

macro_rules! diag_tests {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() { run_case(stringify!($name)); }
        )*
    };
}

diag_tests!(
    undefined_var,
    undefined_fn,
    type_mismatch,
    invalid_cast,
    multiline_span,
    unknown_field,
    already_defined,
    parse_recovery,
    lex_recovery,
    multi_error_stmt,
    cond_recovery,
    deref_move,
    enum_field_type,
);
