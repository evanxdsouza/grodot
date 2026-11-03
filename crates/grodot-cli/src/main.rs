//! `grodot` - the command-line entry point for the Grodot engine.

mod package;

use grodot_core::config::{Project, ProjectConfig};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
Grodot - a Linux-first 3D game engine with the Groscript language

USAGE:
    grodot <command> [options]

COMMANDS:
    new <dir> [--name \"My Game\"]   Create a new project from the default template
    open [dir]                      Open the editor (default: current directory)
    run [dir]                       Play the game in a window
        --headless --frames N         Run without a window (CI smoke test)
        --screenshot out.png          With --headless, save the last frame
    check [dir | file.gro]          Validate a project (config, scenes, scripts, assets)
                                    or check one script's syntax
    script <file.gro>               Run a standalone Groscript file
    repl                            Interactive Groscript prompt
    test [dir]                      Run Groscript tests (*.gro, compared with *.out)
    package [dir] [--out dist]      Build a Linux .tar.gz with the game and runtime
    version                         Print the version

EDITOR SCREENSHOTS:
    grodot open <dir> --screenshot out.png [--select Name] [--play-frames N] [--size 1440x900]
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {}", e);
            ExitCode::from(1)
        }
    }
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// First positional argument after the command (skipping `--flag value` pairs).
fn positional(args: &[String]) -> Option<String> {
    let mut i = 0;
    let takes_value = ["--name", "--frames", "--screenshot", "--select", "--play-frames", "--size", "--out"];
    while i < args.len() {
        if takes_value.contains(&args[i].as_str()) {
            i += 2;
            continue;
        }
        if args[i].starts_with("--") {
            i += 1;
            continue;
        }
        return Some(args[i].clone());
    }
    None
}

fn project_dir(args: &[String]) -> PathBuf {
    positional(args).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn run(args: Vec<String>) -> Result<ExitCode, String> {
    let Some(cmd) = args.first().cloned() else {
        print!("{}", HELP);
        return Ok(ExitCode::SUCCESS);
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "help" | "--help" | "-h" => {
            print!("{}", HELP);
            Ok(ExitCode::SUCCESS)
        }
        "version" | "--version" | "-V" => {
            println!("grodot {}", grodot_core::VERSION);
            Ok(ExitCode::SUCCESS)
        }
        "new" => {
            let dir = positional(rest).ok_or("usage: grodot new <dir> [--name \"My Game\"]")?;
            let dir = PathBuf::from(dir);
            let name = opt(rest, "--name").unwrap_or_else(|| {
                dir.file_name()
                    .map(|n| n.to_string_lossy().replace(['-', '_'], " "))
                    .unwrap_or_else(|| "My Game".into())
            });
            new_project(&dir, &name)?;
            println!("Created project '{}' in {}", name, dir.display());
            println!("Next: grodot open {}", dir.display());
            Ok(ExitCode::SUCCESS)
        }
        "open" | "edit" => {
            let dir = project_dir(rest);
            if let Some(out) = opt(rest, "--screenshot") {
                let (w, h) = parse_size(opt(rest, "--size").as_deref().unwrap_or("1440x900"))?;
                let frames = opt(rest, "--play-frames").map(|s| s.parse().unwrap_or(0)).unwrap_or(0);
                grodot_editor::editor_screenshot(
                    &dir,
                    Path::new(&out),
                    w,
                    h,
                    opt(rest, "--select").as_deref(),
                    frames,
                )?;
                println!("Saved {}", out);
                return Ok(ExitCode::SUCCESS);
            }
            grodot_editor::run_editor(&dir)?;
            Ok(ExitCode::SUCCESS)
        }
        "run" | "play" => {
            let dir = project_dir(rest);
            let project = Project::open(&dir)?;
            if flag(rest, "--headless") {
                let frames = opt(rest, "--frames")
                    .map(|s| s.parse().map_err(|_| "--frames needs a number"))
                    .transpose()?
                    .unwrap_or(300);
                let shot = opt(rest, "--screenshot").map(PathBuf::from);
                let errors =
                    grodot_editor::player::run_headless(&project, frames, shot.as_deref(), flag(rest, "--verbose"))?;
                if errors > 0 {
                    eprintln!("{} script error(s) while running {} frames", errors, frames);
                    return Ok(ExitCode::from(1));
                }
                println!("Ran {} frames headless without errors.", frames);
                return Ok(ExitCode::SUCCESS);
            }
            grodot_editor::player::run_game(&project)?;
            Ok(ExitCode::SUCCESS)
        }
        "check" => {
            let dir = project_dir(rest);
            if dir.extension().map(|e| e == "gro").unwrap_or(false) {
                let src = std::fs::read_to_string(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
                let errors = groscript::check(&src);
                for mut e in errors.clone() {
                    e.file = Some(dir.display().to_string().as_str().into());
                    e.source = Some(src.as_str().into());
                    eprint!("{}", e.report());
                }
                if errors.is_empty() {
                    println!("OK: {} has no syntax errors.", dir.display());
                    return Ok(ExitCode::SUCCESS);
                }
                return Ok(ExitCode::from(1));
            }
            let problems = check_project(&dir)?;
            if problems == 0 {
                println!("OK: no problems found.");
                Ok(ExitCode::SUCCESS)
            } else {
                eprintln!("{} problem(s) found.", problems);
                Ok(ExitCode::from(1))
            }
        }
        "script" => {
            let file = positional(rest).ok_or("usage: grodot script <file.gro>")?;
            let mut it = groscript::Interpreter::new();
            match it.run_file(Path::new(&file)) {
                Ok(_) => Ok(ExitCode::SUCCESS),
                Err(e) => {
                    eprint!("{}", e.report());
                    Ok(ExitCode::from(1))
                }
            }
        }
        "repl" => {
            repl();
            Ok(ExitCode::SUCCESS)
        }
        "test" => {
            let dir = positional(rest).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("tests"));
            let failed = run_tests(&dir)?;
            Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        "package" => {
            let dir = project_dir(rest);
            let out = opt(rest, "--out").map(PathBuf::from).unwrap_or_else(|| dir.join("builds"));
            let problems = check_project(&dir)?;
            if problems > 0 {
                return Err(format!("fix the {} problem(s) above before packaging", problems));
            }
            let archive = package::package(&dir, &out)?;
            println!("Packaged {}", archive.display());
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command '{}'. Run 'grodot help'.", other)),
    }
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or("size must look like 1440x900")?;
    Ok((w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?))
}

pub const TEMPLATE: &[(&str, &str)] = &[
    ("grodot.toml", include_str!("../../../templates/default-project/grodot.toml")),
    ("README.md", include_str!("../../../templates/default-project/README.md")),
    ("scenes/main.gscene", include_str!("../../../templates/default-project/scenes/main.gscene")),
    ("scripts/player.gro", include_str!("../../../templates/default-project/scripts/player.gro")),
    ("assets/audio/.gitkeep", ""),
    ("assets/models/.gitkeep", ""),
    ("prefabs/.gitkeep", ""),
    ("builds/.gitkeep", ""),
];

fn new_project(dir: &Path, name: &str) -> Result<(), String> {
    if dir.join("grodot.toml").exists() {
        return Err(format!("{} already contains a Grodot project", dir.display()));
    }
    if name.contains('"') || name.contains('\n') {
        return Err("project names cannot contain quotes or newlines".into());
    }
    for (rel, text) in TEMPLATE {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&p, text.replace("{{NAME}}", name)).map_err(|e| format!("{}: {}", p.display(), e))?;
    }
    // Validate what we wrote.
    ProjectConfig::parse(&std::fs::read_to_string(dir.join("grodot.toml")).unwrap())?;
    Ok(())
}

/// Validate a project. Prints problems and returns how many were found.
fn check_project(dir: &Path) -> Result<usize, String> {
    let project = Project::open(dir)?;
    println!("Checking '{}' in {}", project.config.name, project.root.display());
    let problems = std::cell::Cell::new(0usize);
    let report = |msg: String| {
        eprintln!("  problem: {}", msg);
        problems.set(problems.get() + 1);
    };
    // Scripts: syntax check every .gro file.
    let mut scripts = Vec::new();
    collect_files(&project.root, "gro", &mut scripts);
    for s in &scripts {
        let rel = s.strip_prefix(&project.root).unwrap_or(s).display().to_string();
        match std::fs::read_to_string(s) {
            Ok(src) => {
                for mut e in groscript::check(&src) {
                    e.file = Some(rel.as_str().into());
                    e.source = Some(src.as_str().into());
                    eprint!("{}", e.report());
                    problems.set(problems.get() + 1);
                }
            }
            Err(e) => report(format!("{}: {}", rel, e)),
        }
    }
    // Scenes and prefabs.
    let mut scenes = Vec::new();
    collect_files(&project.root, "gscene", &mut scenes);
    collect_files(&project.root, "gprefab", &mut scenes);
    let main = project.resolve(&project.config.main_scene)?;
    if !main.is_file() {
        report(format!("main scene '{}' does not exist", project.config.main_scene));
    }
    for sp in &scenes {
        let rel = sp.strip_prefix(&project.root).unwrap_or(sp).display().to_string();
        let scene = match std::fs::read_to_string(sp)
            .map_err(|e| e.to_string())
            .and_then(|s| grodot_core::Scene::from_json_str(&s))
        {
            Ok(s) => s,
            Err(e) => {
                report(format!("{}: {}", rel, e));
                continue;
            }
        };
        let mut cameras = 0;
        for e in &scene.entities {
            if e.camera().map(|c| c.active).unwrap_or(false) {
                cameras += 1;
            }
            let need = |what: &str, path: &str| {
                if !path.is_empty() && grodot_core::mesh::Mesh::primitive(path).is_none() {
                    match project.resolve(path) {
                        Ok(p) if p.is_file() => {}
                        _ => report(format!("{}: entity '{}' uses missing {} '{}'", rel, e.name, what, path)),
                    }
                }
            };
            if let Some(s) = e.script() {
                need("script", &s.path);
            }
            if let Some(m) = e.mesh() {
                need("mesh", &m.mesh);
            }
            if let Some(grodot_core::Component::AudioSource(a)) = e.get("AudioSource") {
                need("audio clip", &a.clip);
            }
        }
        if sp.extension().map(|e| e == "gscene").unwrap_or(false) && cameras == 0 {
            report(format!("{}: no active Camera (the game would show nothing)", rel));
        }
        println!("  {}: {} entities", rel, scene.entities.len());
    }
    // Load every scene's scripts in a runtime to catch class/definition errors.
    if main.is_file() {
        if let Ok(mut rt) = grodot_runtime::Runtime::from_project(&project) {
            rt.start();
            let w = rt.world.borrow();
            for e in w.log.entries.iter().filter(|e| e.level == grodot_runtime::LogLevel::Error) {
                eprintln!("{}", e.message);
                problems.set(problems.get() + 1);
            }
        }
    }
    println!("  {} script(s), {} scene/prefab file(s)", scripts.len(), scenes.len());
    Ok(problems.get())
}

fn collect_files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') || name == "builds" || name == "target" {
            continue;
        }
        if p.is_dir() {
            collect_files(&p, ext, out);
        } else if p.extension().map(|e| e == ext).unwrap_or(false) {
            out.push(p);
        }
    }
}

fn run_tests(dir: &Path) -> Result<usize, String> {
    let mut files = Vec::new();
    collect_files(dir, "gro", &mut files);
    files.retain(|f| !f.file_name().unwrap().to_string_lossy().starts_with('_'));
    if files.is_empty() {
        return Err(format!("no .gro test files found in {}", dir.display()));
    }
    let (mut passed, mut failed) = (0, 0);
    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        let mut it = groscript::Interpreter::new();
        it.seed_random(42);
        it.import_root = f.parent().map(|p| p.to_path_buf());
        let out = it.capture_output();
        let src = std::fs::read_to_string(f).map_err(|e| e.to_string())?;
        let res = it.run_source(&f.file_name().unwrap().to_string_lossy(), &src);
        let expected = std::fs::read_to_string(f.with_extension("out")).ok();
        let ok = match (&res, name.ends_with("_error"), &expected) {
            (Err(e), true, Some(exp)) => {
                let rep = e.report();
                exp.lines().all(|l| rep.contains(l))
            }
            (Err(_), true, None) => true,
            (Ok(_), false, Some(exp)) => *out.borrow() == *exp,
            (Ok(_), false, None) => true,
            _ => false,
        };
        if ok {
            passed += 1;
            println!("  pass  {}", name);
        } else {
            failed += 1;
            println!("  FAIL  {}", name);
            match res {
                Err(e) => print!("{}", indent(&e.report())),
                Ok(_) => {
                    println!("    expected:\n{}", indent(expected.as_deref().unwrap_or("")));
                    println!("    actual:\n{}", indent(&out.borrow()));
                }
            }
        }
    }
    println!("{} passed, {} failed", passed, failed);
    Ok(failed)
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("      {}\n", l)).collect()
}

fn repl() {
    use std::io::{BufRead, Write};
    println!(
        "Groscript {} - type code and press Enter. Blocks continue until 'end'. Ctrl+D to exit.",
        grodot_core::VERSION
    );
    let mut it = groscript::Interpreter::new();
    let stdin = std::io::stdin();
    let mut buf = String::new();
    loop {
        print!("{}", if buf.is_empty() { ">> " } else { ".. " });
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            println!();
            break;
        }
        buf.push_str(&line);
        // Keep reading while a block is still open.
        let errs = groscript::check(&buf);
        if errs
            .iter()
            .any(|e| e.message.starts_with("missing 'end'") || e.message.contains("unterminated interpolation"))
        {
            continue;
        }
        let src = std::mem::take(&mut buf);
        match it.eval_repl(&src) {
            Ok(v) => {
                if !matches!(v, groscript::Value::Nil) {
                    println!("=> {}", v.inspect());
                }
            }
            Err(e) => eprint!("{}", e.report()),
        }
    }
}
