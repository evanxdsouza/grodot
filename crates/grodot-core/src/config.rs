//! Project configuration (`grodot.toml`). Supports the TOML subset Grodot uses:
//! `[sections]`, `key = value` with strings, numbers and booleans, and comments.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum TomlValue {
    Str(String),
    Num(f64),
    Bool(bool),
}

pub type TomlDoc = BTreeMap<String, BTreeMap<String, TomlValue>>;

pub fn parse_toml(src: &str) -> Result<TomlDoc, String> {
    let mut doc: TomlDoc = BTreeMap::new();
    let mut section = String::new();
    doc.insert(section.clone(), BTreeMap::new());
    for (i, raw) in src.lines().enumerate() {
        let line_no = i + 1;
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') {
                return Err(format!("line {}: section header must end with ']'", line_no));
            }
            section = line[1..line.len() - 1].trim().to_string();
            doc.entry(section.clone()).or_default();
            continue;
        }
        let (k, v) = line.split_once('=').ok_or_else(|| format!("line {}: expected 'key = value'", line_no))?;
        let key = k.trim().to_string();
        let v = v.trim();
        let value = if v.starts_with('"') {
            if !v.ends_with('"') || v.len() < 2 {
                return Err(format!("line {}: unterminated string for '{}'", line_no, key));
            }
            TomlValue::Str(v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\"))
        } else if v == "true" || v == "false" {
            TomlValue::Bool(v == "true")
        } else {
            TomlValue::Num(
                v.replace('_', "")
                    .parse()
                    .map_err(|_| format!("line {}: invalid value for '{}': {}", line_no, key, v))?,
            )
        };
        doc.get_mut(&section).unwrap().insert(key, value);
    }
    Ok(doc)
}

fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Parsed `grodot.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectConfig {
    pub config_version: u32,
    pub name: String,
    pub version: String,
    pub main_scene: String,
    pub window_width: u32,
    pub window_height: u32,
    pub window_title: String,
    /// Action name -> list of key names, from the `[input]` section.
    pub input: BTreeMap<String, Vec<String>>,
}

impl ProjectConfig {
    pub fn new(name: &str) -> Self {
        ProjectConfig {
            config_version: 1,
            name: name.to_string(),
            version: "0.1.0".into(),
            main_scene: "scenes/main.gscene".into(),
            window_width: 1280,
            window_height: 720,
            window_title: name.to_string(),
            input: default_input(),
        }
    }

    pub fn parse(src: &str) -> Result<Self, String> {
        let doc = parse_toml(src)?;
        let get = |sec: &str, key: &str| doc.get(sec).and_then(|s| s.get(key));
        let req_str = |sec: &str, key: &str| -> Result<String, String> {
            match get(sec, key) {
                Some(TomlValue::Str(s)) => Ok(s.clone()),
                Some(_) => Err(format!("[{}] {} must be a string", sec, key)),
                None => Err(format!("missing required key [{}] {}", sec, key)),
            }
        };
        let opt_num = |sec: &str, key: &str, d: f64| -> Result<f64, String> {
            match get(sec, key) {
                Some(TomlValue::Num(n)) => Ok(*n),
                Some(_) => Err(format!("[{}] {} must be a number", sec, key)),
                None => Ok(d),
            }
        };
        let name = req_str("project", "name")?;
        let mut input = default_input();
        if let Some(sec) = doc.get("input") {
            for (k, v) in sec {
                match v {
                    TomlValue::Str(s) => {
                        input.insert(
                            k.clone(),
                            s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                        );
                    }
                    _ => return Err(format!("[input] {} must be a string like \"W, Up\"", k)),
                }
            }
        }
        let width = opt_num("window", "width", 1280.0)?;
        let height = opt_num("window", "height", 720.0)?;
        if !(64.0..=8192.0).contains(&width) || !(64.0..=8192.0).contains(&height) {
            return Err("[window] width and height must be between 64 and 8192".into());
        }
        Ok(ProjectConfig {
            config_version: opt_num("project", "config_version", 1.0)? as u32,
            version: req_str("project", "version").unwrap_or_else(|_| "0.1.0".into()),
            main_scene: req_str("game", "main_scene")?,
            window_width: width as u32,
            window_height: height as u32,
            window_title: match get("window", "title") {
                Some(TomlValue::Str(s)) => s.clone(),
                _ => name.clone(),
            },
            name,
            input,
        })
    }

    pub fn to_toml(&self) -> String {
        let mut s = format!(
            "[project]\nname = \"{}\"\nversion = \"{}\"\nengine = \"grodot\"\nconfig_version = {}\n\n[game]\nmain_scene = \"{}\"\n\n[window]\nwidth = {}\nheight = {}\ntitle = \"{}\"\n\n# Input actions: action = \"Key, OtherKey\". Key names: A-Z, 0-9, Space, Enter,\n# Escape, Tab, Shift, Ctrl, Up, Down, Left, Right, MouseLeft, MouseRight.\n[input]\n",
            self.name, self.version, self.config_version, self.main_scene, self.window_width, self.window_height, self.window_title
        );
        for (k, v) in &self.input {
            s.push_str(&format!("{} = \"{}\"\n", k, v.join(", ")));
        }
        s
    }
}

pub fn default_input() -> BTreeMap<String, Vec<String>> {
    let mut m = BTreeMap::new();
    let mut add = |a: &str, keys: &[&str]| {
        m.insert(a.to_string(), keys.iter().map(|k| k.to_string()).collect::<Vec<_>>());
    };
    add("move_forward", &["W", "Up"]);
    add("move_back", &["S", "Down"]);
    add("move_left", &["A", "Left"]);
    add("move_right", &["D", "Right"]);
    add("jump", &["Space"]);
    add("action", &["E", "Enter"]);
    add("restart", &["R"]);
    add("pause", &["Escape"]);
    m
}

/// A loaded project on disk.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: ProjectConfig,
}

impl Project {
    pub fn open(dir: &Path) -> Result<Project, String> {
        let cfg_path = dir.join("grodot.toml");
        let src = std::fs::read_to_string(&cfg_path)
            .map_err(|e| format!("cannot read {}: {} (is this a Grodot project folder?)", cfg_path.display(), e))?;
        let config = ProjectConfig::parse(&src).map_err(|e| format!("grodot.toml: {}", e))?;
        let root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        Ok(Project { root, config })
    }

    /// Resolve a project-relative path, rejecting attempts to escape the project.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, String> {
        resolve_in(&self.root, rel)
    }

    pub fn scripts_dir(&self) -> PathBuf {
        self.root.join("scripts")
    }
}

/// Resolve `rel` under `root`, rejecting absolute paths and `..` components.
pub fn resolve_in(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err(format!("'{}' must be a project-relative path", rel));
    }
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("'{}' may not leave the project folder", rel));
    }
    Ok(root.join(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_project_config() {
        let src = "[project]\nname = \"My First Game\" # comment\nversion = \"0.1.0\"\n\n[game]\nmain_scene = \"scenes/main.gscene\"\n\n[window]\nwidth = 800\nheight = 600\n\n[input]\njump = \"Space, W\"\n";
        let c = ProjectConfig::parse(src).unwrap();
        assert_eq!(c.name, "My First Game");
        assert_eq!(c.window_width, 800);
        assert_eq!(c.window_title, "My First Game");
        assert_eq!(c.input["jump"], vec!["Space", "W"]);
        assert!(c.input.contains_key("move_left"));
        let again = ProjectConfig::parse(&c.to_toml()).unwrap();
        assert_eq!(again, c);
    }

    #[test]
    fn missing_key_named() {
        let e = ProjectConfig::parse("[project]\nname = \"x\"\n").unwrap_err();
        assert!(e.contains("[game] main_scene"), "{}", e);
    }

    #[test]
    fn path_traversal_rejected() {
        assert!(resolve_in(Path::new("/tmp/p"), "../etc/passwd").is_err());
        assert!(resolve_in(Path::new("/tmp/p"), "/etc/passwd").is_err());
        assert!(resolve_in(Path::new("/tmp/p"), "assets/a b/ü.wav").is_ok());
    }
}
