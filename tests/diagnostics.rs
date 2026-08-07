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
    lex_nonascii,
    multi_error_stmt,
    cond_recovery,
    deref_move,
    enum_field_type,
    generic_arity,
    generic_infer,
    generic_field,
    try_bad,
    string_bad,
    string_move,
    string_concat_bad,
    string_concat_move,
    string_cmp_bad,
    string_cmp_move,
    def_dup,
    print_gate,
    builtin_shadow,
    recursive_struct,
    flow_gate,
    entry_missing,
    int_literal_overflow,
    match_unreachable,
    expr_too_deep,
    owning_partial,
    place_temp,
    struct_fields,
    deep_expr_forms,
    dup_binding,
    undefined_type,
    reserved_runtime,
    recovery_enum,
    recovery_in_block,
    array_size,
    broken_def,
    cast_chain,
    struct_drop_cycle,
    float_gate,
    fn_generic_bad,
    fn_generic_move,
    wildcard_read,
    turbofish_nongeneric,
    generic_inst_gate,
    method_bad,
    method_call_bad,
    method_move,
    deref_temp,
    fn_type_name,
    undefined_fn_hints,
    owning_assign_bad,
);
