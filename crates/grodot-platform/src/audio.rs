//! Audio: WAV decoding, a small software mixer and an ALSA output thread.
//!
//! ALSA (`libasound.so.2`) is loaded at runtime. PipeWire and PulseAudio
//! desktops provide ALSA compatibility, so the "default" device works there too.
//! If no device is available, [`Audio::new`] returns an error and the engine
//! keeps running silently.

use crate::dl::Library;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub const SAMPLE_RATE: u32 = 44100;

/// Decoded audio: interleaved stereo f32 at [`SAMPLE_RATE`].
#[derive(Debug, Clone)]
pub struct Clip {
    pub samples: Arc<Vec<f32>>,
}

impl Clip {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }
    pub fn duration(&self) -> f32 {
        self.frames() as f32 / SAMPLE_RATE as f32
    }
}

/// Decode a PCM WAV file (8/16/24/32-bit integer or 32-bit float, mono or stereo).
pub fn decode_wav(bytes: &[u8]) -> Result<Clip, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file (expected RIFF/WAVE header)".into());
    }
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
        if id == b"fmt " && body.len() >= 16 {
            let format = u16::from_le_bytes([body[0], body[1]]);
            let channels = u16::from_le_bytes([body[2], body[3]]);
            let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            let bits = u16::from_le_bytes([body[14], body[15]]);
            let format =
                if format == 0xFFFE && body.len() >= 26 { u16::from_le_bytes([body[24], body[25]]) } else { format };
            fmt = Some((format, channels, rate, bits));
        } else if id == b"data" {
            data = Some(body);
        }
        pos += 8 + size + (size & 1);
    }
    let (format, channels, rate, bits) = fmt.ok_or("WAV file has no fmt chunk")?;
    let data = data.ok_or("WAV file has no data chunk")?;
    if channels == 0 || channels > 2 {
        return Err(format!("unsupported channel count {} (use mono or stereo)", channels));
    }
    if !(4000..=192_000).contains(&rate) {
        return Err(format!("unsupported sample rate {}", rate));
    }
    let bps = (bits / 8) as usize;
    let read = |s: &[u8]| -> f32 {
        match (format, bits) {
            (1, 8) => (s[0] as f32 - 128.0) / 128.0,
            (1, 16) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
            (1, 24) => (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0,
            (1, 32) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
            (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            _ => 0.0,
        }
    };
    if !matches!((format, bits), (1, 8) | (1, 16) | (1, 24) | (1, 32) | (3, 32)) {
        return Err(format!("unsupported WAV encoding (format {}, {} bits); use 16-bit PCM", format, bits));
    }
    let frame_bytes = bps * channels as usize;
    let frames = data.len() / frame_bytes;
    let mut src = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        let base = f * frame_bytes;
        let l = read(&data[base..base + bps]);
        let r = if channels == 2 { read(&data[base + bps..base + 2 * bps]) } else { l };
        src.push(l);
        src.push(r);
    }
    // Linear resample to the mixer rate.
    let samples = if rate == SAMPLE_RATE {
        src
    } else {
        let ratio = rate as f64 / SAMPLE_RATE as f64;
        let out_frames = (frames as f64 / ratio) as usize;
        let mut out = Vec::with_capacity(out_frames * 2);
        for i in 0..out_frames {
            let p = i as f64 * ratio;
            let i0 = p as usize;
            let i1 = (i0 + 1).min(frames.saturating_sub(1));
            let t = (p - i0 as f64) as f32;
            for c in 0..2 {
                let a = src[i0 * 2 + c];
                let b = src[i1 * 2 + c];
                out.push(a + (b - a) * t);
            }
        }
        out
    };
    Ok(Clip { samples: Arc::new(samples) })
}

struct Voice {
    clip: Clip,
    pos: usize,
    volume: f32,
    looping: bool,
    music: bool,
}

/// The mixer, independent of the output device (unit-testable).
pub struct Mixer {
    voices: Vec<Voice>,
    pub master: f32,
}

impl Default for Mixer {
    fn default() -> Self {
        Mixer { voices: Vec::new(), master: 1.0 }
    }
}

impl Mixer {
    pub fn play(&mut self, clip: Clip, volume: f32, looping: bool, music: bool) {
        if music {
            self.voices.retain(|v| !v.music);
        }
        if self.voices.len() >= 32 {
            self.voices.remove(0);
        }
        self.voices.push(Voice { clip, pos: 0, volume, looping, music });
    }

    pub fn stop_music(&mut self) {
        self.voices.retain(|v| !v.music);
    }

    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }

    /// Mix `out.len() / 2` stereo frames.
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        for v in self.voices.iter_mut() {
            let data = &v.clip.samples;
            let mut i = 0;
            while i < out.len() {
                if v.pos >= data.len() {
                    if v.looping && !data.is_empty() {
                        v.pos = 0;
                    } else {
                        break;
                    }
                }
                out[i] += data[v.pos] * v.volume;
                out[i + 1] += data[v.pos + 1] * v.volume;
                v.pos += 2;
                i += 2;
            }
        }
        self.voices.retain(|v| v.looping || v.pos < v.clip.samples.len());
        for s in out.iter_mut() {
            // soft clip
            let x = *s * self.master;
            *s = x / (1.0 + x.abs() * 0.25);
        }
    }
}

enum Cmd {
    Play(Clip, f32, bool, bool),
    StopMusic,
    Volume(f32),
}

type PcmPtr = *mut c_void;

struct Alsa {
    _lib: Library,
    open: unsafe extern "C" fn(*mut PcmPtr, *const c_char, c_int, c_int) -> c_int,
    set_params: unsafe extern "C" fn(PcmPtr, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
    writei: unsafe extern "C" fn(PcmPtr, *const c_void, c_ulong) -> c_long,
    recover: unsafe extern "C" fn(PcmPtr, c_int, c_int) -> c_int,
    close: unsafe extern "C" fn(PcmPtr) -> c_int,
    strerror: unsafe extern "C" fn(c_int) -> *const c_char,
}

impl Alsa {
    fn load() -> Result<Alsa, String> {
        let lib = Library::open(&["libasound.so.2", "libasound.so"])?;
        unsafe {
            Ok(Alsa {
                open: lib.sym("snd_pcm_open")?,
                set_params: lib.sym("snd_pcm_set_params")?,
                writei: lib.sym("snd_pcm_writei")?,
                recover: lib.sym("snd_pcm_recover")?,
                close: lib.sym("snd_pcm_close")?,
                strerror: lib.sym("snd_strerror")?,
                _lib: lib,
            })
        }
    }
    fn err(&self, code: c_int) -> String {
        unsafe { CStr::from_ptr((self.strerror)(code)).to_string_lossy().into_owned() }
    }
}

struct SendPtr(PcmPtr);
unsafe impl Send for SendPtr {}

/// Audio output with a clip cache. Dropping it stops the output thread.
pub struct Audio {
    tx: mpsc::Sender<Cmd>,
    cache: HashMap<PathBuf, Result<Clip, String>>,
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub device: String,
}

impl Audio {
    /// Open the default ALSA device.
    pub fn new() -> Result<Audio, String> {
        let alsa = Alsa::load().map_err(|e| format!("ALSA not available: {}", e))?;
        let mut pcm: PcmPtr = std::ptr::null_mut();
        let dev = CString::new("default").unwrap();
        unsafe {
            let r = (alsa.open)(&mut pcm, dev.as_ptr(), 0, 0);
            if r < 0 {
                return Err(format!("cannot open audio device 'default': {}", alsa.err(r)));
            }
            // S16_LE = 2, RW_INTERLEAVED = 3, 2 channels, 60 ms latency.
            let r = (alsa.set_params)(pcm, 2, 3, 2, SAMPLE_RATE, 1, 60_000);
            if r < 0 {
                (alsa.close)(pcm);
                return Err(format!("cannot configure audio device: {}", alsa.err(r)));
            }
        }
        let (tx, rx) = mpsc::channel::<Cmd>();
        let running = Arc::new(AtomicBool::new(true));
        let run2 = running.clone();
        let pcm = SendPtr(pcm);
        let thread = std::thread::Builder::new()
            .name("grodot-audio".into())
            .spawn(move || {
                let pcm = pcm;
                let mut mixer = Mixer::default();
                let frames = 512;
                let mut fbuf = vec![0f32; frames * 2];
                let mut ibuf = vec![0i16; frames * 2];
                while run2.load(Ordering::Relaxed) {
                    while let Ok(cmd) = rx.try_recv() {
                        match cmd {
                            Cmd::Play(c, v, l, m) => mixer.play(c, v, l, m),
                            Cmd::StopMusic => mixer.stop_music(),
                            Cmd::Volume(v) => mixer.master = v,
                        }
                    }
                    mixer.mix(&mut fbuf);
                    for (o, s) in ibuf.iter_mut().zip(fbuf.iter()) {
                        *o = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                    unsafe {
                        let r = (alsa.writei)(pcm.0, ibuf.as_ptr() as *const c_void, frames as c_ulong);
                        if r < 0 {
                            (alsa.recover)(pcm.0, r as c_int, 1);
                        }
                    }
                }
                unsafe {
                    (alsa.close)(pcm.0);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Audio { tx, cache: HashMap::new(), running, thread: Some(thread), device: "default".into() })
    }

    /// Load (and cache) a clip from disk.
    pub fn load(&mut self, path: &Path) -> Result<Clip, String> {
        if let Some(c) = self.cache.get(path) {
            return c.clone();
        }
        let r = load_clip(path);
        self.cache.insert(path.to_path_buf(), r.clone());
        r
    }

    pub fn play(&mut self, path: &Path, volume: f32, looping: bool, music: bool) -> Result<(), String> {
        let clip = self.load(path)?;
        let _ = self.tx.send(Cmd::Play(clip, volume, looping, music));
        Ok(())
    }

    pub fn stop_music(&mut self) {
        let _ = self.tx.send(Cmd::StopMusic);
    }

    pub fn set_volume(&mut self, v: f32) {
        let _ = self.tx.send(Cmd::Volume(v));
    }
}

pub fn load_clip(path: &Path) -> Result<Clip, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    decode_wav(&bytes).map_err(|e| format!("{}: {}", path.display(), e))
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        v.extend_from_slice(&(channels * 2).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&data);
        v
    }

    #[test]
    fn decodes_and_resamples_mono() {
        let clip = decode_wav(&wav(22050, 1, &[0, 16384, 32767, -32768])).unwrap();
        assert_eq!(clip.frames(), 8);
        assert!((clip.samples[4] - 0.5).abs() < 0.01, "frame 2 = source frame 1");
        assert!((clip.samples[2] - 0.25).abs() < 0.01, "frame 1 is interpolated");
        assert_eq!(clip.samples[2], clip.samples[3], "mono is duplicated to both channels");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_wav(b"hello").is_err());
    }

    #[test]
    fn mixer_plays_and_finishes() {
        let clip = decode_wav(&wav(44100, 2, &[10000; 200])).unwrap();
        let mut m = Mixer::default();
        m.play(clip.clone(), 1.0, false, false);
        let mut buf = vec![0.0; 128];
        m.mix(&mut buf);
        assert!(buf[0] > 0.2);
        m.mix(&mut buf);
        assert_eq!(m.active_voices(), 0);
        m.play(clip.clone(), 1.0, true, true);
        m.mix(&mut buf);
        m.mix(&mut buf);
        assert_eq!(m.active_voices(), 1, "looping music keeps playing");
        m.stop_music();
        assert_eq!(m.active_voices(), 0);
    }

    #[test]
    fn sample_project_sounds_decode() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/orb-garden/assets/audio");
        for f in ["pickup.wav", "jump.wav", "music.wav"] {
            let c = load_clip(&dir.join(f)).unwrap();
            assert!(c.duration() > 0.05);
        }
    }

    #[test]
    fn missing_device_is_an_error_not_a_crash() {
        // In CI containers there is usually no sound card; either outcome is fine,
        // but it must not panic.
        let _ = Audio::new();
    }
}
