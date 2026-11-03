//! `grodot package`: build a self-contained Linux `.tar.gz` of a game.
//!
//! The archive contains the `grodot` executable, the project files and a
//! `run.sh` launcher. Tar and gzip are written directly (no external tools).

use std::path::{Path, PathBuf};

struct Entry {
    path: String,
    data: Vec<u8>,
    mode: u32,
}

fn slug(name: &str) -> String {
    let s: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    let mut out = String::new();
    for c in s.chars() {
        if !(c == '-' && out.ends_with('-')) {
            out.push(c);
        }
    }
    if out.is_empty() {
        "game".into()
    } else {
        out
    }
}

fn collect(root: &Path, dir: &Path, prefix: &str, out: &mut Vec<Entry>) -> Result<(), String> {
    let mut items: Vec<_> =
        std::fs::read_dir(dir).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    items.sort();
    for p in items {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with('.') || name == "builds" || name == "target" || name == "dist" {
            continue;
        }
        if p.is_dir() {
            collect(root, &p, prefix, out)?;
        } else {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push(Entry {
                path: format!("{}/{}", prefix, rel),
                data: std::fs::read(&p).map_err(|e| e.to_string())?,
                mode: 0o644,
            });
        }
    }
    Ok(())
}

pub fn package(project_dir: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let project = grodot_core::Project::open(project_dir)?;
    let base = format!("{}-{}-linux-x86_64", slug(&project.config.name), project.config.version);
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate the grodot executable: {}", e))?;
    let mut entries = vec![Entry {
        path: format!("{}/grodot", base),
        data: std::fs::read(&exe).map_err(|e| e.to_string())?,
        mode: 0o755,
    }];
    entries.push(Entry {
        path: format!("{}/run.sh", base),
        data: b"#!/bin/sh\n# Launch the game. Requires an X11 or XWayland desktop session.\ncd \"$(dirname \"$0\")\" && exec ./grodot run game \"$@\"\n".to_vec(),
        mode: 0o755,
    });
    entries.push(Entry {
        path: format!("{}/README.txt", base),
        data: format!(
            "{name} {ver}\n\nRun: ./run.sh\n\nRequirements: 64-bit Linux (x86_64, glibc) with an X11 or Wayland (XWayland)\ndesktop. Sound uses ALSA (libasound.so.2), which PipeWire and PulseAudio\nprovide; the game runs silently without it.\n\nBuilt with Grodot {gv}.\n",
            name = project.config.name,
            ver = project.config.version,
            gv = grodot_core::VERSION
        )
        .into_bytes(),
        mode: 0o644,
    });
    collect(&project.root, &project.root, &format!("{}/game", base), &mut entries)?;
    let tar = write_tar(&entries)?;
    let gz = gzip_stored(&tar);
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let out = out_dir.join(format!("{}.tar.gz", base));
    std::fs::write(&out, gz).map_err(|e| format!("{}: {}", out.display(), e))?;
    Ok(out)
}

fn octal(field: &mut [u8], v: u64) {
    let s = format!("{:0width$o}", v, width = field.len() - 1);
    field[..s.len()].copy_from_slice(s.as_bytes());
    field[field.len() - 1] = 0;
}

fn write_tar(entries: &[Entry]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mtime = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    for e in entries {
        let mut h = [0u8; 512];
        let (prefix, name) = if e.path.len() > 100 {
            let split = e.path[..e.path.len().min(155)].rfind('/').ok_or("path too long for tar")?;
            (&e.path[..split], &e.path[split + 1..])
        } else {
            ("", e.path.as_str())
        };
        if name.len() > 100 || prefix.len() > 155 {
            return Err(format!("path too long for the archive: {}", e.path));
        }
        h[..name.len()].copy_from_slice(name.as_bytes());
        octal(&mut h[100..108], e.mode as u64);
        octal(&mut h[108..116], 0);
        octal(&mut h[116..124], 0);
        octal(&mut h[124..136], e.data.len() as u64);
        octal(&mut h[136..148], mtime);
        h[148..156].copy_from_slice(b"        ");
        h[156] = b'0';
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        let s = format!("{:06o}\0 ", sum);
        h[148..156].copy_from_slice(s.as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(&e.data);
        let pad = (512 - e.data.len() % 512) % 512;
        out.extend(std::iter::repeat_n(0, pad));
    }
    out.extend(std::iter::repeat_n(0, 1024));
    Ok(out)
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// gzip container with uncompressed (stored) deflate blocks.
fn gzip_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
    let mut chunks = data.chunks(65535).peekable();
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(c) = chunks.next() {
        out.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = c.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Orb Garden!"), "orb-garden");
        assert_eq!(slug("???"), "game");
    }

    #[test]
    fn crc() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
