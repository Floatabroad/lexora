#![cfg(feature = "inkwell")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LEXORA: &str = env!("CARGO_BIN_EXE_lexora");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn compile(backend: &str, case: &Path) {
    let status = Command::new(LEXORA)
        .current_dir(root())
        .args(["--backend", backend])
        .arg(case)
        .status()
        .expect("lexora calistirilamadi");
    assert!(status.success(), "{backend} derleme basarisiz: {}", case.display());
}

fn run_bin(bin: &Path) -> (String, i32) {
    let out = Command::new(bin).output().expect("binary calistirilamadi");
    let stdout = String::from_utf8(out.stdout).expect("stdout utf8 degil");
    (stdout, out.status.code().unwrap_or(-1))
}

fn string_ir(case: &Path, tmp: &Path) -> (String, i32) {
    compile("string-ir", case);
    let asm = tmp.join("out.s");
    let bin = tmp.join("sir");
    assert!(
        Command::new("llc")
            .current_dir(root())
            .args(["output.ll", "-o"])
            .arg(&asm)
            .status()
            .expect("llc")
            .success(),
        "llc basarisiz"
    );
    assert!(
        Command::new("clang")
            .arg("-no-pie")
            .arg(&asm)
            .arg("-o")
            .arg(&bin)
            .status()
            .expect("clang")
            .success(),
        "clang basarisiz"
    );
    run_bin(&bin)
}

fn inkwell(case: &Path) -> (String, i32) {
    compile("inkwell", case);
    run_bin(&root().join("output"))
}

#[test]
fn backend_equivalence() {
    let cases_dir = root().join("tests/equiv");
    let tmp = root().join("target/equiv_tmp");
    fs::create_dir_all(&tmp).unwrap();

    let mut cases: Vec<PathBuf> = fs::read_dir(&cases_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "lx"))
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "equiv korpusu bos");

    for case in &cases {
        let name = case.file_stem().unwrap().to_string_lossy().into_owned();

        let (sir_out, sir_code) = string_ir(case, &tmp);
        let (ink_out, ink_code) = inkwell(case);

        assert_eq!(sir_out, ink_out, "[{name}] stdout farkli (string-ir vs inkwell)");
        assert_eq!(sir_code, ink_code, "[{name}] exit code farkli");

        let golden = cases_dir.join(format!("{name}.out"));
        let expected = fs::read_to_string(&golden)
            .unwrap_or_else(|_| panic!("[{name}] golden yok (once `make equiv-bless`)"));
        assert_eq!(
            sir_out.trim_end(),
            expected.trim_end(),
            "[{name}] golden ile uyusmuyor"
        );
    }
}
