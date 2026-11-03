//! An X11 window that displays a CPU framebuffer (works on X11 and on Wayland
//! desktops through XWayland).

use crate::dl::Library;
use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CString};

type Display = c_void;
type Window = c_ulong;
type Atom = c_ulong;
type Gc = *mut c_void;
type XImagePtr = *mut c_void;

#[repr(C, align(8))]
struct XEvent {
    data: [u8; 192],
}

impl XEvent {
    fn i32_at(&self, off: usize) -> i32 {
        i32::from_ne_bytes(self.data[off..off + 4].try_into().unwrap())
    }
    fn u32_at(&self, off: usize) -> u32 {
        u32::from_ne_bytes(self.data[off..off + 4].try_into().unwrap())
    }
    fn u64_at(&self, off: usize) -> u64 {
        u64::from_ne_bytes(self.data[off..off + 8].try_into().unwrap())
    }
    fn kind(&self) -> i32 {
        self.i32_at(0)
    }
}

const KEY_PRESS: i32 = 2;
const KEY_RELEASE: i32 = 3;
const BUTTON_PRESS: i32 = 4;
const BUTTON_RELEASE: i32 = 5;
const MOTION_NOTIFY: i32 = 6;
const FOCUS_OUT: i32 = 10;
const EXPOSE: i32 = 12;
const CONFIGURE_NOTIFY: i32 = 22;
const CLIENT_MESSAGE: i32 = 33;

const KEY_PRESS_MASK: c_long = 1 << 0;
const KEY_RELEASE_MASK: c_long = 1 << 1;
const BUTTON_PRESS_MASK: c_long = 1 << 2;
const BUTTON_RELEASE_MASK: c_long = 1 << 3;
const POINTER_MOTION_MASK: c_long = 1 << 6;
const EXPOSURE_MASK: c_long = 1 << 15;
const STRUCTURE_NOTIFY_MASK: c_long = 1 << 17;
const FOCUS_CHANGE_MASK: c_long = 1 << 21;
const Z_PIXMAP: c_int = 2;

struct X11 {
    _lib: Library,
    open_display: unsafe extern "C" fn(*const c_char) -> *mut Display,
    close_display: unsafe extern "C" fn(*mut Display) -> c_int,
    default_screen: unsafe extern "C" fn(*mut Display) -> c_int,
    root_window: unsafe extern "C" fn(*mut Display, c_int) -> Window,
    default_visual: unsafe extern "C" fn(*mut Display, c_int) -> *mut c_void,
    default_depth: unsafe extern "C" fn(*mut Display, c_int) -> c_int,
    create_simple_window:
        unsafe extern "C" fn(*mut Display, Window, c_int, c_int, c_uint, c_uint, c_uint, c_ulong, c_ulong) -> Window,
    destroy_window: unsafe extern "C" fn(*mut Display, Window) -> c_int,
    select_input: unsafe extern "C" fn(*mut Display, Window, c_long) -> c_int,
    map_window: unsafe extern "C" fn(*mut Display, Window) -> c_int,
    store_name: unsafe extern "C" fn(*mut Display, Window, *const c_char) -> c_int,
    intern_atom: unsafe extern "C" fn(*mut Display, *const c_char, c_int) -> Atom,
    set_wm_protocols: unsafe extern "C" fn(*mut Display, Window, *mut Atom, c_int) -> c_int,
    pending: unsafe extern "C" fn(*mut Display) -> c_int,
    next_event: unsafe extern "C" fn(*mut Display, *mut XEvent) -> c_int,
    lookup_string: unsafe extern "C" fn(*mut XEvent, *mut c_char, c_int, *mut c_ulong, *mut c_void) -> c_int,
    lookup_keysym: unsafe extern "C" fn(*mut XEvent, c_int) -> c_ulong,
    create_gc: unsafe extern "C" fn(*mut Display, Window, c_ulong, *mut c_void) -> Gc,
    free_gc: unsafe extern "C" fn(*mut Display, Gc) -> c_int,
    create_image: unsafe extern "C" fn(
        *mut Display,
        *mut c_void,
        c_uint,
        c_int,
        c_int,
        *mut c_char,
        c_uint,
        c_uint,
        c_int,
        c_int,
    ) -> XImagePtr,
    put_image:
        unsafe extern "C" fn(*mut Display, Window, Gc, XImagePtr, c_int, c_int, c_int, c_int, c_uint, c_uint) -> c_int,
    free: unsafe extern "C" fn(*mut c_void) -> c_int,
    flush: unsafe extern "C" fn(*mut Display) -> c_int,
    sync: unsafe extern "C" fn(*mut Display, c_int) -> c_int,
    xkb_set_detectable_auto_repeat: Option<unsafe extern "C" fn(*mut Display, c_int, *mut c_int) -> c_int>,
}

impl X11 {
    fn load() -> Result<X11, String> {
        let lib = Library::open(&["libX11.so.6", "libX11.so"])?;
        unsafe {
            Ok(X11 {
                open_display: lib.sym("XOpenDisplay")?,
                close_display: lib.sym("XCloseDisplay")?,
                default_screen: lib.sym("XDefaultScreen")?,
                root_window: lib.sym("XRootWindow")?,
                default_visual: lib.sym("XDefaultVisual")?,
                default_depth: lib.sym("XDefaultDepth")?,
                create_simple_window: lib.sym("XCreateSimpleWindow")?,
                destroy_window: lib.sym("XDestroyWindow")?,
                select_input: lib.sym("XSelectInput")?,
                map_window: lib.sym("XMapWindow")?,
                store_name: lib.sym("XStoreName")?,
                intern_atom: lib.sym("XInternAtom")?,
                set_wm_protocols: lib.sym("XSetWMProtocols")?,
                pending: lib.sym("XPending")?,
                next_event: lib.sym("XNextEvent")?,
                lookup_string: lib.sym("XLookupString")?,
                lookup_keysym: lib.sym("XLookupKeysym")?,
                create_gc: lib.sym("XCreateGC")?,
                free_gc: lib.sym("XFreeGC")?,
                create_image: lib.sym("XCreateImage")?,
                put_image: lib.sym("XPutImage")?,
                free: lib.sym("XFree")?,
                flush: lib.sym("XFlush")?,
                sync: lib.sym("XSync")?,
                xkb_set_detectable_auto_repeat: lib.sym("XkbSetDetectableAutoRepeat").ok(),
                _lib: lib,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Close,
    Resize(u32, u32),
    /// `key` is the engine key name (e.g. "W", "Space"); `text` is the typed character.
    KeyDown {
        key: Option<String>,
        text: Option<String>,
        repeat: bool,
        ctrl: bool,
        shift: bool,
    },
    KeyUp {
        key: Option<String>,
    },
    MouseMove(f32, f32),
    MouseDown(MouseButton),
    MouseUp(MouseButton),
    Wheel(f32),
    FocusLost,
}

pub struct PlatformWindow {
    x: X11,
    dpy: *mut Display,
    win: Window,
    gc: Gc,
    visual: *mut c_void,
    depth: c_int,
    wm_delete: Atom,
    pub width: u32,
    pub height: u32,
    keys_down: std::collections::HashSet<u32>,
}

fn key_name(keysym: c_ulong) -> Option<String> {
    let k = keysym as u32;
    Some(match k {
        0x61..=0x7a => (((k - 0x61) as u8 + b'A') as char).to_string(),
        0x41..=0x5a => (k as u8 as char).to_string(),
        0x30..=0x39 => (k as u8 as char).to_string(),
        0x20 => "Space".into(),
        0xff0d | 0xff8d => "Enter".into(),
        0xff1b => "Escape".into(),
        0xff09 => "Tab".into(),
        0xffe1 | 0xffe2 => "Shift".into(),
        0xffe3 | 0xffe4 => "Ctrl".into(),
        0xffe9 | 0xffea => "Alt".into(),
        0xff08 => "Backspace".into(),
        0xffff => "Delete".into(),
        0xff51 => "Left".into(),
        0xff52 => "Up".into(),
        0xff53 => "Right".into(),
        0xff54 => "Down".into(),
        0xff50 => "Home".into(),
        0xff57 => "End".into(),
        0xffbe..=0xffc9 => format!("F{}", k - 0xffbe + 1),
        _ => return None,
    })
}

impl PlatformWindow {
    pub fn open(title: &str, width: u32, height: u32) -> Result<PlatformWindow, String> {
        let x = X11::load().map_err(|e| format!("X11 is not available: {}", e))?;
        unsafe {
            let dpy = (x.open_display)(std::ptr::null());
            if dpy.is_null() {
                return Err("cannot connect to an X display (is DISPLAY set? On Wayland, XWayland is required)".into());
            }
            let screen = (x.default_screen)(dpy);
            let depth = (x.default_depth)(dpy, screen);
            if depth != 24 && depth != 32 {
                (x.close_display)(dpy);
                return Err(format!("unsupported display depth {} (Grodot needs 24-bit colour)", depth));
            }
            let root = (x.root_window)(dpy, screen);
            let win = (x.create_simple_window)(dpy, root, 0, 0, width, height, 0, 0, 0x202225);
            (x.select_input)(
                dpy,
                win,
                KEY_PRESS_MASK
                    | KEY_RELEASE_MASK
                    | BUTTON_PRESS_MASK
                    | BUTTON_RELEASE_MASK
                    | POINTER_MOTION_MASK
                    | EXPOSURE_MASK
                    | STRUCTURE_NOTIFY_MASK
                    | FOCUS_CHANGE_MASK,
            );
            let t = CString::new(title).unwrap_or_default();
            (x.store_name)(dpy, win, t.as_ptr());
            let name = CString::new("WM_DELETE_WINDOW").unwrap();
            let mut wm_delete = (x.intern_atom)(dpy, name.as_ptr(), 0);
            (x.set_wm_protocols)(dpy, win, &mut wm_delete, 1);
            if let Some(f) = x.xkb_set_detectable_auto_repeat {
                f(dpy, 1, std::ptr::null_mut());
            }
            (x.map_window)(dpy, win);
            let gc = (x.create_gc)(dpy, win, 0, std::ptr::null_mut());
            let visual = (x.default_visual)(dpy, screen);
            (x.flush)(dpy);
            Ok(PlatformWindow {
                x,
                dpy,
                win,
                gc,
                visual,
                depth,
                wm_delete,
                width,
                height,
                keys_down: Default::default(),
            })
        }
    }

    pub fn set_title(&mut self, title: &str) {
        let t = CString::new(title).unwrap_or_default();
        unsafe {
            (self.x.store_name)(self.dpy, self.win, t.as_ptr());
        }
    }

    pub fn poll_events(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        unsafe {
            while (self.x.pending)(self.dpy) > 0 {
                let mut ev = XEvent { data: [0; 192] };
                (self.x.next_event)(self.dpy, &mut ev);
                match ev.kind() {
                    KEY_PRESS | KEY_RELEASE => {
                        let keycode = ev.u32_at(84);
                        let state = ev.u32_at(80);
                        let keysym = (self.x.lookup_keysym)(&mut ev, 0);
                        let key = key_name(keysym);
                        if ev.kind() == KEY_PRESS {
                            let mut buf = [0 as c_char; 16];
                            let mut sym: c_ulong = 0;
                            let n =
                                (self.x.lookup_string)(&mut ev, buf.as_mut_ptr(), 15, &mut sym, std::ptr::null_mut());
                            let text = if n > 0 {
                                let bytes: Vec<u8> = buf[..n as usize].iter().map(|&b| b as u8).collect();
                                let s = String::from_utf8_lossy(&bytes).to_string();
                                if s.chars().all(|c| !c.is_control()) {
                                    Some(s)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let repeat = !self.keys_down.insert(keycode);
                            out.push(Event::KeyDown { key, text, repeat, shift: state & 1 != 0, ctrl: state & 4 != 0 });
                        } else {
                            self.keys_down.remove(&keycode);
                            out.push(Event::KeyUp { key });
                        }
                    }
                    BUTTON_PRESS | BUTTON_RELEASE => {
                        let b = ev.u32_at(84);
                        let pressed = ev.kind() == BUTTON_PRESS;
                        let btn = match b {
                            1 => Some(MouseButton::Left),
                            2 => Some(MouseButton::Middle),
                            3 => Some(MouseButton::Right),
                            4 if pressed => {
                                out.push(Event::Wheel(1.0));
                                None
                            }
                            5 if pressed => {
                                out.push(Event::Wheel(-1.0));
                                None
                            }
                            _ => None,
                        };
                        if let Some(btn) = btn {
                            out.push(if pressed { Event::MouseDown(btn) } else { Event::MouseUp(btn) });
                        }
                    }
                    MOTION_NOTIFY => out.push(Event::MouseMove(ev.i32_at(64) as f32, ev.i32_at(68) as f32)),
                    FOCUS_OUT => {
                        self.keys_down.clear();
                        out.push(Event::FocusLost);
                    }
                    CONFIGURE_NOTIFY => {
                        let (w, h) = (ev.i32_at(56).max(1) as u32, ev.i32_at(60).max(1) as u32);
                        if w != self.width || h != self.height {
                            self.width = w;
                            self.height = h;
                            out.push(Event::Resize(w, h));
                        }
                    }
                    CLIENT_MESSAGE => {
                        if ev.u64_at(56) == self.wm_delete {
                            out.push(Event::Close);
                        }
                    }
                    EXPOSE => {}
                    _ => {}
                }
            }
        }
        out
    }

    /// Copy a 0x00RRGGBB framebuffer to the window.
    pub fn present(&mut self, pixels: &[u32], width: usize, height: usize) {
        if pixels.len() < width * height || width == 0 || height == 0 {
            return;
        }
        unsafe {
            let img = (self.x.create_image)(
                self.dpy,
                self.visual,
                self.depth as c_uint,
                Z_PIXMAP,
                0,
                pixels.as_ptr() as *mut c_char,
                width as c_uint,
                height as c_uint,
                32,
                (width * 4) as c_int,
            );
            if img.is_null() {
                return;
            }
            (self.x.put_image)(self.dpy, self.win, self.gc, img, 0, 0, 0, 0, width as c_uint, height as c_uint);
            // Detach our buffer before freeing the XImage header (XImage.data is at offset 16).
            *(img as *mut u8).add(16).cast::<*mut c_char>() = std::ptr::null_mut();
            (self.x.free)(img);
            (self.x.sync)(self.dpy, 0);
        }
    }
}

impl Drop for PlatformWindow {
    fn drop(&mut self) {
        unsafe {
            (self.x.free_gc)(self.dpy, self.gc);
            (self.x.destroy_window)(self.dpy, self.win);
            (self.x.close_display)(self.dpy);
        }
    }
}
