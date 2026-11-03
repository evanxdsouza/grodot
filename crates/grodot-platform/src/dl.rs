//! Minimal `dlopen` wrapper so Grodot links against no system library at build
//! time: X11 and ALSA are loaded when the program starts.

use std::ffi::{c_char, c_int, c_void, CStr, CString};

extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

const RTLD_NOW: c_int = 2;

pub struct Library {
    handle: *mut c_void,
    pub name: String,
}

impl Library {
    /// Try each candidate name in turn.
    pub fn open(names: &[&str]) -> Result<Library, String> {
        let mut last = String::new();
        for n in names {
            let c = CString::new(*n).unwrap();
            let h = unsafe { dlopen(c.as_ptr(), RTLD_NOW) };
            if !h.is_null() {
                return Ok(Library { handle: h, name: n.to_string() });
            }
            last = unsafe {
                let e = dlerror();
                if e.is_null() {
                    "unknown error".into()
                } else {
                    CStr::from_ptr(e).to_string_lossy().into_owned()
                }
            };
        }
        Err(format!("could not load {} ({})", names.join(" / "), last))
    }

    /// Look up a symbol and cast it to a function pointer type `T`.
    ///
    /// # Safety
    /// `T` must be the correct `extern "C" fn` signature for the symbol.
    pub unsafe fn sym<T: Copy>(&self, name: &str) -> Result<T, String> {
        let c = CString::new(name).unwrap();
        let p = dlsym(self.handle, c.as_ptr());
        if p.is_null() {
            return Err(format!("symbol {} not found in {}", name, self.name));
        }
        Ok(std::mem::transmute_copy::<*mut c_void, T>(&p))
    }
}

// Libraries stay loaded for the lifetime of the process.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}
