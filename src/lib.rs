use std::ffi::{CStr, CString};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use libc::{c_char, c_int, mode_t, size_t, ssize_t, FILE};
static CONFIG: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static ORIG_FUNCS: OnceLock<OriginalFunctions> = OnceLock::new();
static INIT_GUARD: AtomicBool = AtomicBool::new(false);
struct OriginalFunctions {
    open: unsafe extern "C" fn(*const c_char, c_int, ...) -> c_int,
    open64: unsafe extern "C" fn(*const c_char, c_int, ...) -> c_int,
    openat: unsafe extern "C" fn(c_int, *const c_char, c_int, ...) -> c_int,
    openat64: unsafe extern "C" fn(c_int, *const c_char, c_int, ...) -> c_int,
    fopen: unsafe extern "C" fn(*const c_char, *const c_char) -> *mut FILE,
    fopen64: unsafe extern "C" fn(*const c_char, *const c_char) -> *mut FILE,
    stat: unsafe extern "C" fn(*const c_char, *mut libc::stat) -> c_int,
    lstat: unsafe extern "C" fn(*const c_char, *mut libc::stat) -> c_int,
    stat64: unsafe extern "C" fn(*const c_char, *mut libc::stat64) -> c_int,
    lstat64: unsafe extern "C" fn(*const c_char, *mut libc::stat64) -> c_int,
    access: unsafe extern "C" fn(*const c_char, c_int) -> c_int,
    faccessat: unsafe extern "C" fn(c_int, *const c_char, c_int, c_int) -> c_int,
    readlink: unsafe extern "C" fn(*const c_char, *mut c_char, size_t) -> ssize_t,
    readlinkat: unsafe extern "C" fn(c_int, *const c_char, *mut c_char, size_t) -> ssize_t,
    execve: unsafe extern "C" fn(*const c_char, *const *const c_char, *const *const c_char) -> c_int,
    execvp: unsafe extern "C" fn(*const c_char, *const *const c_char) -> c_int,
    execv: unsafe extern "C" fn(*const c_char, *const *const c_char) -> c_int,
}

fn load_config() -> Vec<String> {
    if INIT_GUARD.load(Ordering::Relaxed) {
        return Vec::new();
    }
    INIT_GUARD.store(true, Ordering::Relaxed);
    let result = match fs::read_to_string("/etc/obsidianos-overlays.conf") {
        Ok(content) => content
            .lines()
            .map(|line| {
                line.split_once('#')
                    .map_or(line, |(before_comment, _)| before_comment)
                    .trim()
            })
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    };
    
    INIT_GUARD.store(false, Ordering::Relaxed);
    result
}

fn get_overlays() -> Vec<String> {
    CONFIG
        .get_or_init(|| Mutex::new(load_config()))
        .lock()
        .unwrap()
        .clone()
}

fn get_original_functions() -> &'static OriginalFunctions {
    ORIG_FUNCS.get_or_init(|| unsafe {
        let dlsym = |name: &str| -> *mut libc::c_void {
            let name_c = CString::new(name).unwrap();
            libc::dlsym(libc::RTLD_NEXT, name_c.as_ptr())
        };

        OriginalFunctions {
            open: std::mem::transmute(dlsym("open")),
            open64: std::mem::transmute(dlsym("open64")),
            openat: std::mem::transmute(dlsym("openat")),
            openat64: std::mem::transmute(dlsym("openat64")),
            fopen: std::mem::transmute(dlsym("fopen")),
            fopen64: std::mem::transmute(dlsym("fopen64")),
            stat: std::mem::transmute(dlsym("stat")),
            lstat: std::mem::transmute(dlsym("lstat")),
            stat64: std::mem::transmute(dlsym("stat64")),
            lstat64: std::mem::transmute(dlsym("lstat64")),
            access: std::mem::transmute(dlsym("access")),
            faccessat: std::mem::transmute(dlsym("faccessat")),
            readlink: std::mem::transmute(dlsym("readlink")),
            readlinkat: std::mem::transmute(dlsym("readlinkat")),
            execve: std::mem::transmute(dlsym("execve")),
            execvp: std::mem::transmute(dlsym("execvp")),
            execv: std::mem::transmute(dlsym("execv")),
        }
    })
}

fn find_overlay_path(path: &str) -> Option<String> {
    if INIT_GUARD.load(Ordering::Relaxed) || path.starts_with("/etc/obsidianos-overlays.conf") {
        return None;
    }
    
    let overlays = get_overlays();
    for overlay in overlays {
        let overlay_path = format!("{}{}", overlay, path);
        if Path::new(&overlay_path).exists() {
            return Some(overlay_path);
        }
    }
    None
}

unsafe fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr).to_str().ok().map(String::from) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn open(pathname: *const c_char, flags: c_int, mode: mode_t) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().open)(overlay_cstr.as_ptr(), flags, mode) };
        }
    }
    unsafe { (get_original_functions().open)(pathname, flags, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn open64(pathname: *const c_char, flags: c_int, mode: mode_t) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().open64)(overlay_cstr.as_ptr(), flags, mode) };
        }
    }
    unsafe { (get_original_functions().open64)(pathname, flags, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn openat(dirfd: c_int, pathname: *const c_char, flags: c_int, mode: mode_t) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe { (get_original_functions().openat)(dirfd, overlay_cstr.as_ptr(), flags, mode) };
            }
        }
    }
    unsafe { (get_original_functions().openat)(dirfd, pathname, flags, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn openat64(dirfd: c_int, pathname: *const c_char, flags: c_int, mode: mode_t) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe { (get_original_functions().openat64)(dirfd, overlay_cstr.as_ptr(), flags, mode) };
            }
        }
    }
    unsafe { (get_original_functions().openat64)(dirfd, pathname, flags, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fopen(pathname: *const c_char, mode: *const c_char) -> *mut FILE {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().fopen)(overlay_cstr.as_ptr(), mode) };
        }
    }
    unsafe { (get_original_functions().fopen)(pathname, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fopen64(pathname: *const c_char, mode: *const c_char) -> *mut FILE {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().fopen64)(overlay_cstr.as_ptr(), mode) };
        }
    }
    unsafe { (get_original_functions().fopen64)(pathname, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn stat(pathname: *const c_char, statbuf: *mut libc::stat) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().stat)(overlay_cstr.as_ptr(), statbuf) };
        }
    }
    unsafe { (get_original_functions().stat)(pathname, statbuf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lstat(pathname: *const c_char, statbuf: *mut libc::stat) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().lstat)(overlay_cstr.as_ptr(), statbuf) };
        }
    }
    unsafe { (get_original_functions().lstat)(pathname, statbuf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn stat64(pathname: *const c_char, statbuf: *mut libc::stat64) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().stat64)(overlay_cstr.as_ptr(), statbuf) };
        }
    }
    unsafe { (get_original_functions().stat64)(pathname, statbuf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lstat64(pathname: *const c_char, statbuf: *mut libc::stat64) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().lstat64)(overlay_cstr.as_ptr(), statbuf) };
        }
    }
    unsafe { (get_original_functions().lstat64)(pathname, statbuf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn access(pathname: *const c_char, mode: c_int) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().access)(overlay_cstr.as_ptr(), mode) };
        }
    }
    unsafe { (get_original_functions().access)(pathname, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn faccessat(dirfd: c_int, pathname: *const c_char, mode: c_int, flags: c_int) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe { (get_original_functions().faccessat)(dirfd, overlay_cstr.as_ptr(), mode, flags) };
            }
        }
    }
    unsafe { (get_original_functions().faccessat)(dirfd, pathname, mode, flags) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readlink(pathname: *const c_char, buf: *mut c_char, bufsiz: size_t) -> ssize_t {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().readlink)(overlay_cstr.as_ptr(), buf, bufsiz) };
        }
    }
    unsafe { (get_original_functions().readlink)(pathname, buf, bufsiz) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readlinkat(dirfd: c_int, pathname: *const c_char, buf: *mut c_char, bufsiz: size_t) -> ssize_t {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe { (get_original_functions().readlinkat)(dirfd, overlay_cstr.as_ptr(), buf, bufsiz) };
            }
        }
    }
    unsafe { (get_original_functions().readlinkat)(dirfd, pathname, buf, bufsiz) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execve(pathname: *const c_char, argv: *const *const c_char, envp: *const *const c_char) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().execve)(overlay_cstr.as_ptr(), argv, envp) };
        }
    }
    unsafe { (get_original_functions().execve)(pathname, argv, envp) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(file) } {
        if path_str.starts_with('/') {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe { (get_original_functions().execvp)(overlay_cstr.as_ptr(), argv) };
            }
        }
    }
    unsafe { (get_original_functions().execvp)(file, argv) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execv(pathname: *const c_char, argv: *const *const c_char) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().execv)(overlay_cstr.as_ptr(), argv) };
        }
    }
    unsafe { (get_original_functions().execv)(pathname, argv) }
}
