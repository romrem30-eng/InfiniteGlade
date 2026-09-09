use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

#[repr(C)]
struct STARTUPINFOW {
    cb: u32,
    lp_reserved: *mut u16,
    lp_desktop: *mut u16,
    lp_title: *mut u16,
    dw_x: u32,
    dw_y: u32,
    dw_x_size: u32,
    dw_y_size: u32,
    dw_x_count_chars: u32,
    dw_y_count_chars: u32,
    dw_fill_attribute: u32,
    dw_flags: u32,
    w_show_window: u16,
    cb_reserved2: u16,
    lp_reserved2: *mut u8,
    h_std_input: *mut c_void,
    h_std_output: *mut c_void,
    h_std_error: *mut c_void,
}

#[repr(C)]
struct PROCESS_INFORMATION {
    h_process: *mut c_void,
    h_thread: *mut c_void,
    dw_process_id: u32,
    dw_thread_id: u32,
}

const CREATE_SUSPENDED: u32 = 0x00000004;
const DETACHED_PROCESS: u32 = 0x00000008;
const MEM_COMMIT: u32 = 0x00001000;
const MEM_RESERVE: u32 = 0x00002000;
const PAGE_READWRITE: u32 = 0x04;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateProcessW(
        lpApplicationName: *const u16,
        lpCommandLine: *mut u16,
        lpProcessAttributes: *const c_void,
        lpThreadAttributes: *const c_void,
        bInheritHandles: i32,
        dwCreationFlags: u32,
        lpEnvironment: *const c_void,
        lpCurrentDirectory: *const u16,
        lpStartupInfo: *const STARTUPINFOW,
        lpProcessInformation: *mut PROCESS_INFORMATION,
    ) -> i32;

    fn VirtualAllocEx(
        hProcess: *mut c_void,
        lpAddress: *const c_void,
        dwSize: usize,
        flAllocationType: u32,
        flProtect: u32,
    ) -> *mut c_void;

    fn WriteProcessMemory(
        hProcess: *mut c_void,
        lpBaseAddress: *mut c_void,
        lpBuffer: *const c_void,
        nSize: usize,
        lpNumberOfBytesWritten: *mut usize,
    ) -> i32;

    fn CreateRemoteThread(
        hProcess: *mut c_void,
        lpThreadAttributes: *const c_void,
        dwStackSize: usize,
        lpStartAddress: unsafe extern "system" fn(*mut c_void) -> u32,
        lpParameter: *mut c_void,
        dwCreationFlags: u32,
        lpThreadId: *mut u32,
    ) -> *mut c_void;

    fn ResumeThread(hThread: *mut c_void) -> u32;
    fn WaitForSingleObject(hHandle: *mut c_void, dwMilliseconds: u32) -> u32;
    fn CloseHandle(hObject: *mut c_void) -> i32;
    fn GetModuleHandleA(lpModuleName: *const u8) -> *mut c_void;
    fn GetProcAddress(hModule: *mut c_void, lpProcName: *const u8) -> usize;
}

fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn main() {
    println!("==================================================");
    println!(" GladeLoader -- Standalone Injector for Tiny Glade");
    println!("==================================================");

    let current_dir = std::env::current_dir().unwrap_or_default();
    let default_game_path = PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Tiny Glade\tiny-glade.exe");
    
    let game_exe = if current_dir.join("tiny-glade.exe").exists() {
        current_dir.join("tiny-glade.exe")
    } else if default_game_path.exists() {
        default_game_path
    } else {
        eprintln!("[-] Error: tiny-glade.exe not found!");
        return;
    };

    let game_dir = game_exe.parent().unwrap();
    let dll_path = if current_dir.join("glade_loader.dll").exists() {
        current_dir.join("glade_loader.dll")
    } else {
        game_dir.join("glade_loader.dll")
    };

    if !dll_path.exists() {
        eprintln!("[-] Error: glade_loader.dll not found at {:?}", dll_path);
        return;
    }

    println!("[+] Target game: {:?}", game_exe);
    println!("[+] Target DLL:  {:?}", dll_path);

    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dw_flags = 0x00000001; // STARTF_USESHOWWINDOW
        si.w_show_window = 1; // SW_SHOWNORMAL
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();

        let mut cmd_line_wide = to_wide_null(&format!("\"{}\"", game_exe.to_str().unwrap()));
        let game_dir_wide = to_wide_null(game_dir.to_str().unwrap());

        println!("[*] Launching game process in suspended state...");
        let success = CreateProcessW(
            std::ptr::null(),
            cmd_line_wide.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_SUSPENDED | DETACHED_PROCESS,
            std::ptr::null(),
            game_dir_wide.as_ptr(),
            &mut si,
            &mut pi,
        );

        if success == 0 {
            eprintln!("[-] Failed to launch game process. Error: {}", std::io::Error::last_os_error());
            return;
        }

        println!("[+] Game created successfully (PID: {})", pi.dw_process_id);

        let dll_path_wide: Vec<u16> = OsStr::new(dll_path.to_str().unwrap()).encode_wide().chain(Some(0)).collect();
        let bytes_len = dll_path_wide.len() * 2;

        let remote_mem = VirtualAllocEx(
            pi.h_process,
            std::ptr::null(),
            bytes_len,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );

        if remote_mem.is_null() {
            eprintln!("[-] VirtualAllocEx failed.");
            ResumeThread(pi.h_thread);
            return;
        }

        let mut written = 0;
        WriteProcessMemory(
            pi.h_process,
            remote_mem,
            dll_path_wide.as_ptr() as _,
            bytes_len,
            &mut written,
        );

        let k32 = GetModuleHandleA(b"kernel32.dll\0".as_ptr());
        let load_lib_addr = GetProcAddress(k32, b"LoadLibraryW\0".as_ptr());

        println!("[*] Injecting glade_loader.dll into game memory...");
        let remote_thread = CreateRemoteThread(
            pi.h_process,
            std::ptr::null(),
            0,
            std::mem::transmute(load_lib_addr),
            remote_mem,
            0,
            std::ptr::null_mut(),
        );

        if !remote_thread.is_null() {
            WaitForSingleObject(remote_thread, 3000);
            CloseHandle(remote_thread);
            println!("[+] Injection successful!");
        } else {
            eprintln!("[-] CreateRemoteThread failed.");
        }

        println!("[+] Resuming game...");
        ResumeThread(pi.h_thread);
        CloseHandle(pi.h_thread);
        CloseHandle(pi.h_process);
        println!("[OK] GladeLoader finished. Have fun in Tiny Glade!");
    }
}
