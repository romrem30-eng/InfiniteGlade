use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

static REAL_WINMM: AtomicUsize = AtomicUsize::new(0);

unsafe fn get_real_proc(name: &[u8]) -> usize {
    let mut handle = REAL_WINMM.load(Ordering::Relaxed);
    if handle == 0 {
        let path = b"C:\\Windows\\System32\\winmm.dll\0";
        unsafe {
            handle = LoadLibraryA(path.as_ptr()) as usize;
        }
        REAL_WINMM.store(handle, Ordering::Relaxed);
    }
    unsafe {
        match GetProcAddress(handle as _, name.as_ptr()) {
            Some(f) => f as usize,
            None => 0,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn timeGetTime() -> u32 {
    type FnType = unsafe extern "system" fn() -> u32;
    static ADDR: AtomicUsize = AtomicUsize::new(0);
    let mut p = ADDR.load(Ordering::Relaxed);
    if p == 0 {
        unsafe {
            p = get_real_proc(b"timeGetTime\0");
        }
        ADDR.store(p, Ordering::Relaxed);
    }
    if p != 0 {
        unsafe {
            let f: FnType = std::mem::transmute(p);
            f()
        }
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn timeBeginPeriod(u_period: u32) -> u32 {
    type FnType = unsafe extern "system" fn(u32) -> u32;
    static ADDR: AtomicUsize = AtomicUsize::new(0);
    let mut p = ADDR.load(Ordering::Relaxed);
    if p == 0 {
        unsafe {
            p = get_real_proc(b"timeBeginPeriod\0");
        }
        ADDR.store(p, Ordering::Relaxed);
    }
    if p != 0 {
        unsafe {
            let f: FnType = std::mem::transmute(p);
            f(u_period)
        }
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn timeEndPeriod(u_period: u32) -> u32 {
    type FnType = unsafe extern "system" fn(u32) -> u32;
    static ADDR: AtomicUsize = AtomicUsize::new(0);
    let mut p = ADDR.load(Ordering::Relaxed);
    if p == 0 {
        unsafe {
            p = get_real_proc(b"timeEndPeriod\0");
        }
        ADDR.store(p, Ordering::Relaxed);
    }
    if p != 0 {
        unsafe {
            let f: FnType = std::mem::transmute(p);
            f(u_period)
        }
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn timeGetDevCaps(ptc: *mut c_void, cbtc: u32) -> u32 {
    type FnType = unsafe extern "system" fn(*mut c_void, u32) -> u32;
    static ADDR: AtomicUsize = AtomicUsize::new(0);
    let mut p = ADDR.load(Ordering::Relaxed);
    if p == 0 {
        unsafe {
            p = get_real_proc(b"timeGetDevCaps\0");
        }
        ADDR.store(p, Ordering::Relaxed);
    }
    if p != 0 {
        unsafe {
            let f: FnType = std::mem::transmute(p);
            f(ptc, cbtc)
        }
    } else {
        0
    }
}
