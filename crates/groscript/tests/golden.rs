//! Golden tests: every `tests/language/*.gro` file is run and its output is
//! compared with the matching `.out` file. Files ending in `_error.gro` must
//! fail, and the `.out` file holds a substring of the expected error report.
//!
//! Set `BLESS=1` to (re)write the expected output files.

use std::path::PathBuf;

fn run(path: &PathBuf) -> (String, Option<String>) {
    let mut it = groscript::Interpreter::new();
    it.seed_random(42);
    let out = it.capture_output();
    it.import_root = path.parent().map(|p| p.to_path_buf());
    let src = std::fs::read_to_string(path).unwrap();
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    let res = it.run_source(&name, &src);
    let text = out.borrow().clone();
    (text, res.err().map(|e| e.report()))
}

#[test]
fn golden_language_tests() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/language");
    let bless = std::env::var("BLESS").is_ok();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("tests/language exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "gro").unwrap_or(false))
        .collect();
    entries.sort();
    assert!(!entries.is_empty());
    let mut failures = Vec::new();
    for path in &entries {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if name.starts_with('_') {
            continue; // helper modules
        }
        let expect_error = name.ends_with("_error");
        let (out, err) = run(path);
        let out_path = path.with_extension("out");
        let actual = if expect_error {
            match &err {
                Some(e) => e.clone(),
                None => {
                    failures.push(format!("{}: expected an error but it succeeded", name));
                    continue;
                }
            }
        } else {
            if let Some(e) = &err {
                failures.push(format!("{}: unexpected error\n{}", name, e));
                continue;
            }
            out
        };
        if bless {
            std::fs::write(&out_path, &actual).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&out_path).unwrap_or_default();
        let ok = if expect_error {
            expected.lines().all(|l| actual.contains(l))
        } else {
            expected == actual
        };
        if !ok {
            failures.push(format!("{}:\n--- expected\n{}\n--- actual\n{}", name, expected, actual));
        }
    }
    if !failures.is_empty() {
        panic!("{} golden test(s) failed:\n\n{}", failures.len(), failures.join("\n\n"));
    }
}
