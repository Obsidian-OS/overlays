use libc::{
    __errno_location, FILE, c_char, c_int, c_uint, gid_t, mode_t, off_t, size_t, ssize_t, uid_t,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::{CStr, CString};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
thread_local! {
    static DIRENT_BUFFER: RefCell<libc::dirent> = RefCell::new(unsafe { std::mem::zeroed() });
}

thread_local! {
    static DIRENT64_BUFFER: RefCell<libc::dirent64> = RefCell::new(unsafe { std::mem::zeroed() });
}

thread_local! {
    static OVERLAY_DISABLED: RefCell<bool> = RefCell::new(false);
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct DirPointer(*mut libc::DIR);
unsafe impl Send for DirPointer {}
unsafe impl Sync for DirPointer {}
struct OverlayDir {
    original_dir_ptr: *mut libc::DIR,
    overlay_dir_ptrs: Vec<*mut libc::DIR>,
    seen_original_entries: HashSet<String>,
}

unsafe impl Send for OverlayDir {}
unsafe impl Sync for OverlayDir {}
static OVERLAY_DIR_MAP: OnceLock<Mutex<HashMap<DirPointer, Box<OverlayDir>>>> = OnceLock::new();
fn get_overlay_dir_map() -> &'static Mutex<HashMap<DirPointer, Box<OverlayDir>>> {
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] get_overlay_dir_map: initializing or getting map...");
    }
    let map = OVERLAY_DIR_MAP.get_or_init(|| Mutex::new(HashMap::new()));
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] get_overlay_dir_map: map initialized/retrieved.");
    }
    map
}
static VERBOSE_MODE: OnceLock<bool> = OnceLock::new();
fn is_verbose_mode_enabled() -> bool {
    *VERBOSE_MODE
        .get_or_init(|| env::var("OBSIDIANOS_OVERLAYS_VERBOSE").map_or(false, |v| v == "1"))
}
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
    execve:
        unsafe extern "C" fn(*const c_char, *const *const c_char, *const *const c_char) -> c_int,
    execvp: unsafe extern "C" fn(*const c_char, *const *const c_char) -> c_int,
    execv: unsafe extern "C" fn(*const c_char, *const *const c_char) -> c_int,
    unlink: unsafe extern "C" fn(*const c_char) -> c_int,
    unlinkat: unsafe extern "C" fn(c_int, *const c_char, c_int) -> c_int,
    rmdir: unsafe extern "C" fn(*const c_char) -> c_int,
    mkdir: unsafe extern "C" fn(*const c_char, mode_t) -> c_int,
    mkdirat: unsafe extern "C" fn(c_int, *const c_char, mode_t) -> c_int,
    rename: unsafe extern "C" fn(*const c_char, *const c_char) -> c_int,
    renameat: unsafe extern "C" fn(c_int, *const c_char, c_int, *const c_char) -> c_int,
    creat: unsafe extern "C" fn(*const c_char, mode_t) -> c_int,
    creat64: unsafe extern "C" fn(*const c_char, mode_t) -> c_int,
    chdir: unsafe extern "C" fn(*const c_char) -> c_int,
    chmod: unsafe extern "C" fn(*const c_char, mode_t) -> c_int,
    fchmodat: unsafe extern "C" fn(c_int, *const c_char, mode_t, c_int) -> c_int,
    chown: unsafe extern "C" fn(*const c_char, uid_t, gid_t) -> c_int,
    fchownat: unsafe extern "C" fn(c_int, *const c_char, uid_t, gid_t, c_int) -> c_int,
    lchown: unsafe extern "C" fn(*const c_char, uid_t, gid_t) -> c_int,
    link: unsafe extern "C" fn(*const c_char, *const c_char) -> c_int,
    linkat: unsafe extern "C" fn(c_int, *const c_char, c_int, *const c_char, c_int) -> c_int,
    symlink: unsafe extern "C" fn(*const c_char, *const c_char) -> c_int,
    symlinkat: unsafe extern "C" fn(*const c_char, c_int, *const c_char) -> c_int,
    truncate: unsafe extern "C" fn(*const c_char, off_t) -> c_int,
    readdir: unsafe extern "C" fn(*mut libc::DIR) -> *mut libc::dirent,
    readdir64: unsafe extern "C" fn(*mut libc::DIR) -> *mut libc::dirent64,
    opendir: unsafe extern "C" fn(*const c_char) -> *mut libc::DIR,
    closedir: unsafe extern "C" fn(*mut libc::DIR) -> c_int,
    statx: Option<
        unsafe extern "C" fn(c_int, *const c_char, c_int, c_uint, *mut libc::statx) -> c_int,
    >,
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
    if let Some(config_mutex) = CONFIG.get() {
        if is_verbose_mode_enabled() {
            eprintln!("[DEBUG] get_overlays: CONFIG already initialized, acquiring lock...");
        }
        let config_lock = config_mutex.lock().unwrap();
        if is_verbose_mode_enabled() {
            eprintln!("[DEBUG] get_overlays: CONFIG lock acquired.");
        }
        let overlays = config_lock.clone();
        if is_verbose_mode_enabled() {
            eprintln!("[DEBUG] get_overlays: CONFIG lock released.");
        }
        return overlays;
    }

    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] get_overlays: CONFIG not initialized, loading config...");
    }
    let loaded_config = load_config();
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] get_overlays: acquiring lock for initialization...");
    }
    let config_mutex = CONFIG.get_or_init(|| Mutex::new(Vec::new()));
    let mut config_lock = config_mutex.lock().unwrap();
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] get_overlays: lock acquired for initialization.");
    }
    *config_lock = loaded_config;
    let overlays = config_lock.clone();
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] get_overlays: lock released after initialization.");
    }
    overlays
}

fn get_original_functions() -> &'static OriginalFunctions {
    ORIG_FUNCS.get_or_init(|| unsafe {
        if is_verbose_mode_enabled() {
            eprintln!("[DEBUG] Initializing original functions...");
        }
        let dlsym = |name: &str| -> *mut libc::c_void {
            let name_c = CString::new(name).unwrap();
            let ptr = libc::dlsym(libc::RTLD_NEXT, name_c.as_ptr());
            if ptr.is_null() {
                eprintln!("[ERROR] Failed to load symbol: {}", name);
            }
            ptr
        };

        let open_ptr = dlsym("open");
        assert!(!open_ptr.is_null(), "Failed to load critical symbol: open");
        let stat_ptr = dlsym("stat");
        assert!(!stat_ptr.is_null(), "Failed to load critical symbol: stat");
        let statx_ptr = dlsym("statx");
        OriginalFunctions {
            open: std::mem::transmute(open_ptr),
            open64: std::mem::transmute(dlsym("open64")),
            openat: std::mem::transmute(dlsym("openat")),
            openat64: std::mem::transmute(dlsym("openat64")),
            fopen: std::mem::transmute(dlsym("fopen")),
            fopen64: std::mem::transmute(dlsym("fopen64")),
            stat: std::mem::transmute(stat_ptr),
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
            unlink: std::mem::transmute(dlsym("unlink")),
            unlinkat: std::mem::transmute(dlsym("unlinkat")),
            rmdir: std::mem::transmute(dlsym("rmdir")),
            mkdir: std::mem::transmute(dlsym("mkdir")),
            mkdirat: std::mem::transmute(dlsym("mkdirat")),
            rename: std::mem::transmute(dlsym("rename")),
            renameat: std::mem::transmute(dlsym("renameat")),
            creat: std::mem::transmute(dlsym("creat")),
            creat64: std::mem::transmute(dlsym("creat64")),
            chdir: std::mem::transmute(dlsym("chdir")),
            chmod: std::mem::transmute(dlsym("chmod")),
            fchmodat: std::mem::transmute(dlsym("fchmodat")),
            chown: std::mem::transmute(dlsym("chown")),
            fchownat: std::mem::transmute(dlsym("fchownat")),
            lchown: std::mem::transmute(dlsym("lchown")),
            link: std::mem::transmute(dlsym("link")),
            linkat: std::mem::transmute(dlsym("linkat")),
            symlink: std::mem::transmute(dlsym("symlink")),
            symlinkat: std::mem::transmute(dlsym("symlinkat")),
            truncate: std::mem::transmute(dlsym("truncate")),
            readdir: std::mem::transmute(dlsym("readdir")),
            readdir64: std::mem::transmute(dlsym("readdir64")),
            opendir: std::mem::transmute(dlsym("opendir")),
            closedir: std::mem::transmute(dlsym("closedir")),
            statx: if statx_ptr.is_null() {
                None
            } else {
                Some(std::mem::transmute(statx_ptr))
            },
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn statx(
    dirfd: c_int,
    pathname: *const c_char,
    flags: c_int,
    mask: c_uint,
    statxbuf: *mut libc::statx,
) -> c_int {
    if OVERLAY_DISABLED.with(|disabled| *disabled.borrow()) {
        let original_functions = get_original_functions();
        if let Some(original_statx) = original_functions.statx {
            return unsafe { original_statx(dirfd, pathname, flags, mask, statxbuf) };
        } else {
            unsafe {
                *__errno_location() = libc::ENOSYS;
            }
            return -1;
        }
    }

    let original_functions = get_original_functions();
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str).or_else(|| find_overlay_dir(&path_str)) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            if let Some(original_statx) = original_functions.statx {
                return unsafe {
                    original_statx(dirfd, overlay_cstr.as_ptr(), flags, mask, statxbuf)
                };
            } else {
                unsafe {
                    *__errno_location() = libc::ENOSYS;
                }
                return -1;
            }
        }
    }

    if let Some(original_statx) = original_functions.statx {
        unsafe { original_statx(dirfd, pathname, flags, mask, statxbuf) }
    } else {
        unsafe {
            *__errno_location() = libc::ENOSYS;
        }
        -1
    }
}

fn find_overlay_path(path: &str) -> Option<String> {
    if path.starts_with("/etc/obsidianos-overlays.conf") {
        return None;
    }

    let overlays = get_overlays();
    for overlay in overlays {
        let overlay_path = format!("{}{}", overlay, path);
        let is_file = OVERLAY_DISABLED.with(|disabled| {
            *disabled.borrow_mut() = true;
            let is_file = Path::new(&overlay_path).is_file();
            *disabled.borrow_mut() = false;
            is_file
        });

        if is_verbose_mode_enabled() {
            eprintln!(
                "[DEBUG] Checking overlay_path: {} is_file: {}",
                overlay_path, is_file
            );
        }
        if is_file {
            if is_verbose_mode_enabled() {
                eprintln!("[*] ObsidianOS Overlays: {} -> {}", path, overlay_path);
            }
            return Some(overlay_path);
        }
    }
    None
}

fn find_overlay_dir(path: &str) -> Option<String> {
    if path.starts_with("/etc/obsidianos-overlays.conf") {
        return None;
    }

    let overlays = get_overlays();
    for overlay in overlays {
        let overlay_path = format!("{}{}", overlay, path);
        let is_dir = OVERLAY_DISABLED.with(|disabled| {
            *disabled.borrow_mut() = true;
            let is_dir = Path::new(&overlay_path).is_dir();
            *disabled.borrow_mut() = false;
            is_dir
        });

        if is_verbose_mode_enabled() {
            eprintln!(
                "[DEBUG] Checking overlay_dir: {} is_dir: {}",
                overlay_path, is_dir
            );
        }
        if is_dir {
            if is_verbose_mode_enabled() {
                eprintln!("[*] ObsidianOS Overlays: dir {} -> {}", path, overlay_path);
            }
            return Some(overlay_path);
        }
    }
    None
}

unsafe fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    match unsafe { CStr::from_ptr(ptr).to_str() } {
        Ok(s) => Some(s.to_string()),
        Err(_) => None,
    }
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
            return unsafe {
                (get_original_functions().open64)(overlay_cstr.as_ptr(), flags, mode)
            };
        }
    }
    unsafe { (get_original_functions().open64)(pathname, flags, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn openat(
    dirfd: c_int,
    pathname: *const c_char,
    flags: c_int,
    mode: mode_t,
) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe {
                    (get_original_functions().openat)(dirfd, overlay_cstr.as_ptr(), flags, mode)
                };
            }
        }
    }
    unsafe { (get_original_functions().openat)(dirfd, pathname, flags, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn openat64(
    dirfd: c_int,
    pathname: *const c_char,
    flags: c_int,
    mode: mode_t,
) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe {
                    (get_original_functions().openat64)(dirfd, overlay_cstr.as_ptr(), flags, mode)
                };
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
        if let Some(overlay_path) = find_overlay_path(&path_str).or_else(|| find_overlay_dir(&path_str)) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().stat)(overlay_cstr.as_ptr(), statbuf) };
        }
    }
    unsafe { (get_original_functions().stat)(pathname, statbuf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lstat(pathname: *const c_char, statbuf: *mut libc::stat) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str).or_else(|| find_overlay_dir(&path_str)) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().lstat)(overlay_cstr.as_ptr(), statbuf) };
        }
    }
    unsafe { (get_original_functions().lstat)(pathname, statbuf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn stat64(pathname: *const c_char, statbuf: *mut libc::stat64) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str).or_else(|| find_overlay_dir(&path_str)) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().stat64)(overlay_cstr.as_ptr(), statbuf) };
        }
    }
    unsafe { (get_original_functions().stat64)(pathname, statbuf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lstat64(pathname: *const c_char, statbuf: *mut libc::stat64) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str).or_else(|| find_overlay_dir(&path_str)) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().lstat64)(overlay_cstr.as_ptr(), statbuf) };
        }
    }
    unsafe { (get_original_functions().lstat64)(pathname, statbuf) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn access(pathname: *const c_char, mode: c_int) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str).or_else(|| find_overlay_dir(&path_str)) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().access)(overlay_cstr.as_ptr(), mode) };
        }
    }
    unsafe { (get_original_functions().access)(pathname, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn faccessat(
    dirfd: c_int,
    pathname: *const c_char,
    mode: c_int,
    flags: c_int,
) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe {
                    (get_original_functions().faccessat)(dirfd, overlay_cstr.as_ptr(), mode, flags)
                };
            }
        }
    }
    unsafe { (get_original_functions().faccessat)(dirfd, pathname, mode, flags) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readlink(
    pathname: *const c_char,
    buf: *mut c_char,
    bufsiz: size_t,
) -> ssize_t {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe {
                (get_original_functions().readlink)(overlay_cstr.as_ptr(), buf, bufsiz)
            };
        }
    }
    unsafe { (get_original_functions().readlink)(pathname, buf, bufsiz) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readlinkat(
    dirfd: c_int,
    pathname: *const c_char,
    buf: *mut c_char,
    bufsiz: size_t,
) -> ssize_t {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe {
                    (get_original_functions().readlinkat)(dirfd, overlay_cstr.as_ptr(), buf, bufsiz)
                };
            }
        }
    }
    unsafe { (get_original_functions().readlinkat)(dirfd, pathname, buf, bufsiz) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn execve(
    pathname: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn unlink(pathname: *const c_char) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().unlink)(overlay_cstr.as_ptr()) };
        }
    }
    unsafe { (get_original_functions().unlink)(pathname) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn unlinkat(dirfd: c_int, pathname: *const c_char, flags: c_int) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe {
                    (get_original_functions().unlinkat)(dirfd, overlay_cstr.as_ptr(), flags)
                };
            }
        }
    }
    unsafe { (get_original_functions().unlinkat)(dirfd, pathname, flags) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rmdir(pathname: *const c_char) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().rmdir)(overlay_cstr.as_ptr()) };
        }
    }
    unsafe { (get_original_functions().rmdir)(pathname) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mkdir(pathname: *const c_char, mode: mode_t) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().mkdir)(overlay_cstr.as_ptr(), mode) };
        }
    }
    unsafe { (get_original_functions().mkdir)(pathname, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mkdirat(dirfd: c_int, pathname: *const c_char, mode: mode_t) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe {
                    (get_original_functions().mkdirat)(dirfd, overlay_cstr.as_ptr(), mode)
                };
            }
        }
    }
    unsafe { (get_original_functions().mkdirat)(dirfd, pathname, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rename(oldpath: *const c_char, newpath: *const c_char) -> c_int {
    if let Some(oldpath_str) = unsafe { cstr_to_string(oldpath) } {
        if let Some(overlay_oldpath) = find_overlay_path(&oldpath_str) {
            let overlay_old_cstr = CString::new(overlay_oldpath).unwrap();
            if let Some(newpath_str) = unsafe { cstr_to_string(newpath) } {
                if let Some(overlay_newpath) = find_overlay_path(&newpath_str) {
                    let overlay_new_cstr = CString::new(overlay_newpath).unwrap();
                    return unsafe {
                        (get_original_functions().rename)(
                            overlay_old_cstr.as_ptr(),
                            overlay_new_cstr.as_ptr(),
                        )
                    };
                }
            }
        }
    }
    unsafe { (get_original_functions().rename)(oldpath, newpath) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn renameat(
    olddirfd: c_int,
    oldpath: *const c_char,
    newdirfd: c_int,
    newpath: *const c_char,
) -> c_int {
    if olddirfd == libc::AT_FDCWD && newdirfd == libc::AT_FDCWD {
        if let Some(oldpath_str) = unsafe { cstr_to_string(oldpath) } {
            if let Some(overlay_oldpath) = find_overlay_path(&oldpath_str) {
                let overlay_old_cstr = CString::new(overlay_oldpath).unwrap();
                if let Some(newpath_str) = unsafe { cstr_to_string(newpath) } {
                    if let Some(overlay_newpath) = find_overlay_path(&newpath_str) {
                        let overlay_new_cstr = CString::new(overlay_newpath).unwrap();
                        return unsafe {
                            (get_original_functions().renameat)(
                                olddirfd,
                                overlay_old_cstr.as_ptr(),
                                newdirfd,
                                overlay_new_cstr.as_ptr(),
                            )
                        };
                    }
                }
            }
        }
    }
    unsafe { (get_original_functions().renameat)(olddirfd, oldpath, newdirfd, newpath) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn creat(pathname: *const c_char, mode: mode_t) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().creat)(overlay_cstr.as_ptr(), mode) };
        }
    }
    unsafe { (get_original_functions().creat)(pathname, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn creat64(pathname: *const c_char, mode: mode_t) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().creat64)(overlay_cstr.as_ptr(), mode) };
        }
    }
    unsafe { (get_original_functions().creat64)(pathname, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chdir(path: *const c_char) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(path) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().chdir)(overlay_cstr.as_ptr()) };
        }
    }
    unsafe { (get_original_functions().chdir)(path) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chmod(pathname: *const c_char, mode: mode_t) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().chmod)(overlay_cstr.as_ptr(), mode) };
        }
    }
    unsafe { (get_original_functions().chmod)(pathname, mode) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fchmodat(
    dirfd: c_int,
    pathname: *const c_char,
    mode: mode_t,
    flags: c_int,
) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe {
                    (get_original_functions().fchmodat)(dirfd, overlay_cstr.as_ptr(), mode, flags)
                };
            }
        }
    }
    unsafe { (get_original_functions().fchmodat)(dirfd, pathname, mode, flags) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chown(pathname: *const c_char, owner: uid_t, group: gid_t) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe {
                (get_original_functions().chown)(overlay_cstr.as_ptr(), owner, group)
            };
        }
    }
    unsafe { (get_original_functions().chown)(pathname, owner, group) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fchownat(
    dirfd: c_int,
    pathname: *const c_char,
    owner: uid_t,
    group: gid_t,
    flags: c_int,
) -> c_int {
    if dirfd == libc::AT_FDCWD {
        if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
            if let Some(overlay_path) = find_overlay_path(&path_str) {
                let overlay_cstr = CString::new(overlay_path).unwrap();
                return unsafe {
                    (get_original_functions().fchownat)(
                        dirfd,
                        overlay_cstr.as_ptr(),
                        owner,
                        group,
                        flags,
                    )
                };
            }
        }
    }
    unsafe { (get_original_functions().fchownat)(dirfd, pathname, owner, group, flags) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lchown(pathname: *const c_char, owner: uid_t, group: gid_t) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(pathname) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe {
                (get_original_functions().lchown)(overlay_cstr.as_ptr(), owner, group)
            };
        }
    }
    unsafe { (get_original_functions().lchown)(pathname, owner, group) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn link(oldpath: *const c_char, newpath: *const c_char) -> c_int {
    if let Some(oldpath_str) = unsafe { cstr_to_string(oldpath) } {
        if let Some(overlay_oldpath) = find_overlay_path(&oldpath_str) {
            let overlay_old_cstr = CString::new(overlay_oldpath).unwrap();
            if let Some(newpath_str) = unsafe { cstr_to_string(newpath) } {
                if let Some(overlay_newpath) = find_overlay_path(&newpath_str) {
                    let overlay_new_cstr = CString::new(overlay_newpath).unwrap();
                    return unsafe {
                        (get_original_functions().link)(
                            overlay_old_cstr.as_ptr(),
                            overlay_new_cstr.as_ptr(),
                        )
                    };
                }
            }
        }
    }
    unsafe { (get_original_functions().link)(oldpath, newpath) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn linkat(
    olddirfd: c_int,
    oldpath: *const c_char,
    newdirfd: c_int,
    newpath: *const c_char,
    flags: c_int,
) -> c_int {
    if olddirfd == libc::AT_FDCWD && newdirfd == libc::AT_FDCWD {
        if let Some(oldpath_str) = unsafe { cstr_to_string(oldpath) } {
            if let Some(overlay_oldpath) = find_overlay_path(&oldpath_str) {
                let overlay_old_cstr = CString::new(overlay_oldpath).unwrap();
                if let Some(newpath_str) = unsafe { cstr_to_string(newpath) } {
                    if let Some(overlay_newpath) = find_overlay_path(&newpath_str) {
                        let overlay_new_cstr = CString::new(overlay_newpath).unwrap();
                        return unsafe {
                            (get_original_functions().linkat)(
                                olddirfd,
                                overlay_old_cstr.as_ptr(),
                                newdirfd,
                                overlay_new_cstr.as_ptr(),
                                flags,
                            )
                        };
                    }
                }
            }
        }
    }
    unsafe { (get_original_functions().linkat)(olddirfd, oldpath, newdirfd, newpath, flags) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn symlink(target: *const c_char, linkpath: *const c_char) -> c_int {
    if let Some(target_str) = unsafe { cstr_to_string(target) } {
        if let Some(overlay_target) = find_overlay_path(&target_str) {
            let overlay_target_cstr = CString::new(overlay_target).unwrap();
            if let Some(linkpath_str) = unsafe { cstr_to_string(linkpath) } {
                if let Some(overlay_linkpath) = find_overlay_path(&linkpath_str) {
                    let overlay_link_cstr = CString::new(overlay_linkpath).unwrap();
                    return unsafe {
                        (get_original_functions().symlink)(
                            overlay_target_cstr.as_ptr(),
                            overlay_link_cstr.as_ptr(),
                        )
                    };
                }
            }
        }
    }
    unsafe { (get_original_functions().symlink)(target, linkpath) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn symlinkat(
    target: *const c_char,
    newdirfd: c_int,
    linkpath: *const c_char,
) -> c_int {
    if newdirfd == libc::AT_FDCWD {
        if let Some(target_str) = unsafe { cstr_to_string(target) } {
            if let Some(overlay_target) = find_overlay_path(&target_str) {
                let overlay_target_cstr = CString::new(overlay_target).unwrap();
                if let Some(linkpath_str) = unsafe { cstr_to_string(linkpath) } {
                    if let Some(overlay_linkpath) = find_overlay_path(&linkpath_str) {
                        let overlay_link_cstr = CString::new(overlay_linkpath).unwrap();
                        return unsafe {
                            (get_original_functions().symlinkat)(
                                overlay_target_cstr.as_ptr(),
                                newdirfd,
                                overlay_link_cstr.as_ptr(),
                            )
                        };
                    }
                }
            }
        }
    }
    unsafe { (get_original_functions().symlinkat)(target, newdirfd, linkpath) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn truncate(path: *const c_char, length: off_t) -> c_int {
    if let Some(path_str) = unsafe { cstr_to_string(path) } {
        if let Some(overlay_path) = find_overlay_path(&path_str) {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            return unsafe { (get_original_functions().truncate)(overlay_cstr.as_ptr(), length) };
        }
    }
    unsafe { (get_original_functions().truncate)(path, length) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn opendir(name: *const c_char) -> *mut libc::DIR {
    let path_str = match unsafe { cstr_to_string(name) } {
        Some(s) => s,
        None => return unsafe { (get_original_functions().opendir)(name) },
    };

    let original_dir_ptr = unsafe { (get_original_functions().opendir)(name) };
    let mut overlay_dir_ptrs = Vec::new();
    let overlays = get_overlays();
    for overlay in overlays {
        let overlay_path = format!("{}{}", overlay, path_str);
        let is_dir = OVERLAY_DISABLED.with(|disabled| {
            *disabled.borrow_mut() = true;
            let is_dir = Path::new(&overlay_path).is_dir();
            *disabled.borrow_mut() = false;
            is_dir
        });
        if is_dir {
            let overlay_cstr = CString::new(overlay_path).unwrap();
            let ptr = unsafe { (get_original_functions().opendir)(overlay_cstr.as_ptr()) };
            if !ptr.is_null() {
                overlay_dir_ptrs.push(ptr);
            }
        }
    }

    if original_dir_ptr.is_null() && overlay_dir_ptrs.is_empty() {
        return std::ptr::null_mut();
    }

    let overlay_dir = OverlayDir {
        original_dir_ptr,
        overlay_dir_ptrs: overlay_dir_ptrs.clone(),
        seen_original_entries: HashSet::new(),
    };

    let overlay_dir_boxed = Box::new(overlay_dir);
    let overlay_dir_ptr_raw = Box::into_raw(overlay_dir_boxed);
    let returned_ptr = if original_dir_ptr.is_null() && !overlay_dir_ptrs.is_empty() {
        overlay_dir_ptrs[0]
    } else {
        original_dir_ptr
    };
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] opendir: acquiring OVERLAY_DIR_MAP lock...");
    }
    get_overlay_dir_map()
        .lock()
        .unwrap()
        .insert(DirPointer(returned_ptr), unsafe {
            Box::from_raw(overlay_dir_ptr_raw)
        });
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] opendir: OVERLAY_DIR_MAP lock acquired and released.");
    }

    returned_ptr
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readdir(dirp: *mut libc::DIR) -> *mut libc::dirent {
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] readdir: acquiring OVERLAY_DIR_MAP lock...");
    }
    let mut map = get_overlay_dir_map().lock().unwrap();
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] readdir: OVERLAY_DIR_MAP lock acquired.");
    }
    let overlay_dir_opt = map.get_mut(&DirPointer(dirp));
    if let Some(overlay_dir) = overlay_dir_opt {
        loop {
            for overlay_ptr in &overlay_dir.overlay_dir_ptrs {
                let overlay_dirent_ptr = unsafe { (get_original_functions().readdir)(*overlay_ptr) };
                if !overlay_dirent_ptr.is_null() {
                    let overlay_dirent = unsafe { *overlay_dirent_ptr };
                    let d_name_cstr = unsafe { CStr::from_ptr(overlay_dirent.d_name.as_ptr()) };
                    let d_name_str = d_name_cstr.to_string_lossy().into_owned();
                    overlay_dir.seen_original_entries.insert(d_name_str.clone());
                    return DIRENT_BUFFER.with(|cell| {
                        let mut dirent_buffer = cell.borrow_mut();
                        *dirent_buffer = overlay_dirent;
                        &mut *dirent_buffer as *mut libc::dirent
                    });
                }
            }

            if overlay_dir.original_dir_ptr.is_null() {
                return std::ptr::null_mut();
            }
            let original_dirent_ptr =
                unsafe { (get_original_functions().readdir)(overlay_dir.original_dir_ptr) };
            if !original_dirent_ptr.is_null() {
                let original_dirent = unsafe { *original_dirent_ptr };
                let d_name_cstr = unsafe { CStr::from_ptr(original_dirent.d_name.as_ptr()) };
                let d_name_str = d_name_cstr.to_string_lossy().into_owned();
                if d_name_str != "."
                    && d_name_str != ".."
                    && !overlay_dir.seen_original_entries.contains(&d_name_str)
                {
                    return DIRENT_BUFFER.with(|cell| {
                        let mut dirent_buffer = cell.borrow_mut();
                        *dirent_buffer = original_dirent;
                        &mut *dirent_buffer as *mut libc::dirent
                    });
                }
            } else {
                return std::ptr::null_mut();
            }
        }
    }
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] readdir: OVERLAY_DIR_MAP lock released (implicit).");
    }
    unsafe { (get_original_functions().readdir)(dirp) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn readdir64(dirp: *mut libc::DIR) -> *mut libc::dirent64 {
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] readdir64: acquiring OVERLAY_DIR_MAP lock...");
    }
    let mut map = get_overlay_dir_map().lock().unwrap();
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] readdir64: OVERLAY_DIR_MAP lock acquired.");
    }
    let overlay_dir_opt = map.get_mut(&DirPointer(dirp));
    if let Some(overlay_dir) = overlay_dir_opt {
        loop {
            for overlay_ptr in &overlay_dir.overlay_dir_ptrs {
                let overlay_dirent64_ptr =
                    unsafe { (get_original_functions().readdir64)(*overlay_ptr) };
                if !overlay_dirent64_ptr.is_null() {
                    let overlay_dirent64 = unsafe { *overlay_dirent64_ptr };
                    let d_name_cstr = unsafe { CStr::from_ptr(overlay_dirent64.d_name.as_ptr()) };
                    let d_name_str = d_name_cstr.to_string_lossy().into_owned();
                    overlay_dir.seen_original_entries.insert(d_name_str.clone());
                    return DIRENT64_BUFFER.with(|cell| {
                        let mut dirent64_buffer = cell.borrow_mut();
                        *dirent64_buffer = overlay_dirent64;
                        &mut *dirent64_buffer as *mut libc::dirent64
                    });
                }
            }

            if overlay_dir.original_dir_ptr.is_null() {
                return std::ptr::null_mut();
            }
            let original_dirent64_ptr =
                unsafe { (get_original_functions().readdir64)(overlay_dir.original_dir_ptr) };
            if !original_dirent64_ptr.is_null() {
                let original_dirent64 = unsafe { *original_dirent64_ptr };
                let d_name_cstr = unsafe { CStr::from_ptr(original_dirent64.d_name.as_ptr()) };
                let d_name_str = d_name_cstr.to_string_lossy().into_owned();
                if d_name_str != "."
                    && d_name_str != ".."
                    && !overlay_dir.seen_original_entries.contains(&d_name_str)
                {
                    return DIRENT64_BUFFER.with(|cell| {
                        let mut dirent64_buffer = cell.borrow_mut();
                        *dirent64_buffer = original_dirent64;
                        &mut *dirent64_buffer as *mut libc::dirent64
                    });
                }
            } else {
                return std::ptr::null_mut();
            }
        }
    }
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] readdir64: OVERLAY_DIR_MAP lock released (implicit).");
    }
    unsafe { (get_original_functions().readdir64)(dirp) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn closedir(dirp: *mut libc::DIR) -> c_int {
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] closedir: acquiring OVERLAY_DIR_MAP lock...");
    }
    let mut map = get_overlay_dir_map().lock().unwrap();
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] closedir: OVERLAY_DIR_MAP lock acquired.");
    }
    if let Some(overlay_dir) = map.remove(&DirPointer(dirp)) {
        let original_res = if !overlay_dir.original_dir_ptr.is_null() {
            unsafe { (get_original_functions().closedir)(overlay_dir.original_dir_ptr) }
        } else {
            0
        };
        let mut overlay_res = 0;
        for ptr in overlay_dir.overlay_dir_ptrs {
            if !ptr.is_null() {
                let res = unsafe { (get_original_functions().closedir)(ptr) };
                if res != 0 {
                    overlay_res = res;
                }
            }
        }
        if original_res == 0 && overlay_res == 0 {
            if is_verbose_mode_enabled() {
                eprintln!("[DEBUG] closedir: OVERLAY_DIR_MAP lock released (implicit).");
            }
            return 0;
        } else {
            if is_verbose_mode_enabled() {
                eprintln!("[DEBUG] closedir: OVERLAY_DIR_MAP lock released (implicit).");
            }
            return -1;
        }
    }
    if is_verbose_mode_enabled() {
        eprintln!("[DEBUG] closedir: OVERLAY_DIR_MAP lock released (implicit).");
    }
    unsafe { (get_original_functions().closedir)(dirp) }
}
