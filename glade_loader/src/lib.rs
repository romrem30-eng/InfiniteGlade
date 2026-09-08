use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::Memory::VirtualProtect;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static INIT_DONE: AtomicBool = AtomicBool::new(false);
static GOALS_CALL_COUNT: AtomicU64 = AtomicU64::new(0);

fn get_log_path() -> PathBuf {
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let mut p = PathBuf::from(userprofile);
        p.push("Saved Games");
        p.push("Tiny Glade");
        let _ = std::fs::create_dir_all(&p);
        p.push("glade_loader.log");
        return p;
    }
    PathBuf::from("glade_loader.log")
}

pub fn log(msg: &str) {
    let path = get_log_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[GladeLoader] {}", msg);
    }
}

// Hook definitions (Bevy system functions take 8 parameters on x64 MSVC)
type System8Fn = unsafe extern "system" fn(
    *mut c_void, *mut c_void, *mut c_void, *mut c_void,
    *mut c_void, *mut c_void, *mut c_void, *mut c_void,
);
static mut ORIGINAL_STARTUP_SHEEP: Option<System8Fn> = None;
static mut ORIGINAL_UPDATE_SHEEP_GOALS: Option<System8Fn> = None;

unsafe extern "C" fn fake_restart_app(_app_id: u32) -> bool {
    log(">>> SteamAPI_RestartAppIfNecessary intercepted -> returning false");
    false
}

unsafe extern "system" fn hooked_startup_sheep(
    a1: *mut c_void, a2: *mut c_void, a3: *mut c_void, a4: *mut c_void,
    a5: *mut c_void, a6: *mut c_void, a7: *mut c_void, a8: *mut c_void,
) {
    log("==================================================");
    log(">>> Spawning vanilla flock (Sheep)...");

    unsafe {
        if let Some(orig) = ORIGINAL_STARTUP_SHEEP {
            orig(a1, a2, a3, a4, a5, a6, a7, a8);
        }
    }
    log(">>> Vanilla sheep spawned successfully!");
    log("==================================================");
}

unsafe extern "system" fn hooked_update_sheep_goals(
    a1: *mut c_void, a2: *mut c_void, a3: *mut c_void, a4: *mut c_void,
    a5: *mut c_void, a6: *mut c_void, a7: *mut c_void, a8: *mut c_void,
) {
    let count = GOALS_CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    if count % 300 == 0 {
        log(&format!(">>> update_sheep_goals running (frame #{})", count));
    }

    unsafe {
        if let Some(orig) = ORIGINAL_UPDATE_SHEEP_GOALS {
            orig(a1, a2, a3, a4, a5, a6, a7, a8);
        }
    }
}

fn init_loader() {
    if INIT_DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::sleep(Duration::from_millis(100));
    log("==========================================");
    log("GladeLoader v1.0.0 initializing...");
    log("Target: Tiny Glade (Bevy Engine)");

    unsafe {
        // Prevent Steam restart
        let _ = minhook::MinHook::create_hook_api(
            "steam_api64.dll",
            "SteamAPI_RestartAppIfNecessary",
            fake_restart_app as *mut c_void,
        );
        let _ = minhook::MinHook::enable_all_hooks();

        let base = GetModuleHandleA(std::ptr::null()) as usize;
        log(&format!("Base image address: 0x{:X}", base));

        // Patch 1: Bypass Unrecognized Files integrity check
        const RVA_INTEGRITY_CHECK: usize = 0x176F5F;
        let patch_addr = (base + RVA_INTEGRITY_CHECK) as *mut u8;
        const PAGE_EXECUTE_READWRITE: u32 = 0x40;
        let mut old_protect = 0;
        if VirtualProtect(patch_addr as _, 6, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            let patch: [u8; 6] = [0xE9, 0x90, 0x00, 0x00, 0x00, 0x90];
            std::ptr::copy_nonoverlapping(patch.as_ptr(), patch_addr, 6);
            let mut dummy = 0;
            VirtualProtect(patch_addr as _, 6, old_protect, &mut dummy);
            log(">>> [Anti-Tamper] Integrity check bypassed!");
        }

        // Patch 2: Infinite Build Area (All border validation methods return true)
        const RVA_IS_WITHIN_GLADE: usize = 0x9D4A10;
        const RVA_IS_POS_INSIDE: usize = 0x91C400;
        const RVA_IS_SHAPE_INSIDE: usize = 0x91C420;
        const RVA_IS_CURVE2_INSIDE: usize = 0x91C710;

        let patches_always_true = [
            (RVA_IS_WITHIN_GLADE, "is_within_glade_shape"),
            (RVA_IS_POS_INSIDE, "GladeBorder::is_pos_inside"),
            (RVA_IS_SHAPE_INSIDE, "GladeBorder::is_shape_inside"),
            (RVA_IS_CURVE2_INSIDE, "GladeBorder::is_curve2_inside"),
        ];

        let mov_al_1_ret: [u8; 3] = [0xB0, 0x01, 0xC3];
        for (rva, name) in patches_always_true {
            let ptr = (base + rva) as *mut u8;
            if VirtualProtect(ptr as _, 3, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
                std::ptr::copy_nonoverlapping(mov_al_1_ret.as_ptr(), ptr, 3);
                let mut dummy = 0;
                VirtualProtect(ptr as _, 3, old_protect, &mut dummy);
                log(&format!(">>> [Infinite Glade] {} patched (always TRUE)!", name));
            }
        }

        // Patch 3: Unlocked Camera Pan (country_core::systems::camera_rig::normal::update)
        // 3a: Bypass pan delta clamp against [rdi + 0x64]
        const RVA_CAM_DELTA_CLAMP: usize = 0x90E88D;
        let cam_delta_ptr = (base + RVA_CAM_DELTA_CLAMP) as *mut u8;
        if VirtualProtect(cam_delta_ptr as _, 2, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            // jbe -> jmp (EB 2B)
            let patch: [u8; 2] = [0xEB, 0x2B];
            std::ptr::copy_nonoverlapping(patch.as_ptr(), cam_delta_ptr, 2);
            let mut dummy = 0;
            VirtualProtect(cam_delta_ptr as _, 2, old_protect, &mut dummy);
            log(">>> [Camera Unlimit] Camera pan delta clamp UNLOCKED!");
        }

        // 3b: Bypass glade perimeter clamp (jump directly to camera focus update at 0x14090ec10)
        const RVA_CAM_POS_CLAMP: usize = 0x90E908;
        let cam_pos_ptr = (base + RVA_CAM_POS_CLAMP) as *mut u8;
        if VirtualProtect(cam_pos_ptr as _, 6, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            // jne 0x14090ec10 -> jmp 0x14090ec10; nop
            let patch: [u8; 6] = [0xE9, 0x03, 0x03, 0x00, 0x00, 0x90];
            std::ptr::copy_nonoverlapping(patch.as_ptr(), cam_pos_ptr, 6);
            let mut dummy = 0;
            VirtualProtect(cam_pos_ptr as _, 6, old_protect, &mut dummy);
            log(">>> [Camera Unlimit] Camera position glade boundary clamp UNLOCKED!");
        }

        // 3c: Bypass alternate-mode camera clamp
        const RVA_CAM_ALT_CLAMP: usize = 0x90EA1A;
        let cam_alt_ptr = (base + RVA_CAM_ALT_CLAMP) as *mut u8;
        if VirtualProtect(cam_alt_ptr as _, 6, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            // jbe 0x14090eb27 -> jmp 0x14090eb27; nop
            let patch: [u8; 6] = [0xE9, 0x08, 0x01, 0x00, 0x00, 0x90];
            std::ptr::copy_nonoverlapping(patch.as_ptr(), cam_alt_ptr, 6);
            let mut dummy = 0;
            VirtualProtect(cam_alt_ptr as _, 6, old_protect, &mut dummy);
            log(">>> [Camera Unlimit] Camera alternate clamp UNLOCKED!");
        }

        // Patch 4: 3.7x Camera Zoom Out (max zoom 40.0m -> 150.0m)
        const RVA_MAX_ZOOM: usize = 0x2EB6074;
        let zoom_ptr = (base + RVA_MAX_ZOOM) as *mut f32;
        const PAGE_READWRITE: u32 = 0x04;
        if VirtualProtect(zoom_ptr as _, 4, PAGE_READWRITE, &mut old_protect) != 0 {
            *zoom_ptr = 150.0;
            let mut dummy = 0;
            VirtualProtect(zoom_ptr as _, 4, old_protect, &mut dummy);
            log(">>> [Camera Unlimit] Max zoom distance increased to 150m (3.7x)!");
        }

        // Patch 5: Allow Zoom Anywhere (country_core::systems::camera_rig::MainCamera::zoom distance check)
        const RVA_ZOOM_CHECK: usize = 0xA7B7BE;
        let zoom_check_ptr = (base + RVA_ZOOM_CHECK) as *mut u8;
        if VirtualProtect(zoom_check_ptr as _, 6, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            // NOP out the jbe clamp (6 bytes)
            let patch: [u8; 6] = [0x90; 6];
            std::ptr::copy_nonoverlapping(patch.as_ptr(), zoom_check_ptr, 6);
            let mut dummy = 0;
            VirtualProtect(zoom_check_ptr as _, 6, old_protect, &mut dummy);
            log(">>> [Camera Unlimit] Zoom anywhere check UNLOCKED!");
        }

        // Patch 6: Clean Glade Border Debris & Stones (display_glade_border: always take photomode exit)
        const RVA_BORDER_BRANCH: usize = 0x2069E41;
        let border_branch_ptr = (base + RVA_BORDER_BRANCH) as *mut u8;
        if VirtualProtect(border_branch_ptr as _, 2, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            // 75 19 (jne) -> 90 90 (NOP): cleanly exits without drawing border markers/stones
            let patch: [u8; 2] = [0x90, 0x90];
            std::ptr::copy_nonoverlapping(patch.as_ptr(), border_branch_ptr, 2);
            let mut dummy = 0;
            VirtualProtect(border_branch_ptr as _, 2, old_protect, &mut dummy);
            log(">>> [Clean Glade] Glade border markers & boundary stones cleanly REMOVED!");
        }

        // Patch 7: Clean Backdrop Trees & Bushes (country_core::startup::clearing::init_clearing::closure_env$1)
        // By returning None (mov dword ptr [rcx], 0; mov rax, rcx; ret), FilterMap yields 0 background trees/bushes.
        // This safely leaves the forest vector empty with 0 crashes and removes the entire backdrop clutter ring!
        const RVA_CLEARING_FILTER: usize = 0x19BC480;
        let clearing_ptr = (base + RVA_CLEARING_FILTER) as *mut u8;
        if VirtualProtect(clearing_ptr as _, 10, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            // mov dword ptr [rcx], 0 (C7 01 00 00 00 00)
            // mov rax, rcx (48 89 C8)
            // ret (C3)
            let patch: [u8; 10] = [0xC7, 0x01, 0x00, 0x00, 0x00, 0x00, 0x48, 0x89, 0xC8, 0xC3];
            std::ptr::copy_nonoverlapping(patch.as_ptr(), clearing_ptr, 10);
            let mut dummy = 0;
            VirtualProtect(clearing_ptr as _, 10, old_protect, &mut dummy);
            log(">>> [Clean Glade] Backdrop trees & bushes cleanly REMOVED (0 items spawned)!");
        }

        const RVA_STARTUP_SHEEP: usize = 0x16F0EF0;
        const RVA_UPDATE_SHEEP_GOALS: usize = 0x28A4D50;

        let target_startup = (base + RVA_STARTUP_SHEEP) as *mut c_void;
        match minhook::MinHook::create_hook(target_startup, hooked_startup_sheep as *mut c_void) {
            Ok(orig) => {
                ORIGINAL_STARTUP_SHEEP = Some(std::mem::transmute(orig));
                let _ = minhook::MinHook::enable_hook(target_startup);
                log("Hook startup_sheep enabled (RVA +0x16F0EF0)");
            }
            Err(e) => log(&format!("Failed startup_sheep hook: {:?}", e)),
        }

        let target_goals = (base + RVA_UPDATE_SHEEP_GOALS) as *mut c_void;
        match minhook::MinHook::create_hook(target_goals, hooked_update_sheep_goals as *mut c_void) {
            Ok(orig) => {
                ORIGINAL_UPDATE_SHEEP_GOALS = Some(std::mem::transmute(orig));
                let _ = minhook::MinHook::enable_hook(target_goals);
                log("Hook update_sheep_goals enabled (RVA +0x28A4D50)");
            }
            Err(e) => log(&format!("Failed update_sheep_goals hook: {:?}", e)),
        }
    }
    log("All GladeLoader hooks active!");
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateThread(
        lpThreadAttributes: *const c_void,
        dwStackSize: usize,
        lpStartAddress: unsafe extern "system" fn(*mut c_void) -> u32,
        lpParameter: *mut c_void,
        dwCreationFlags: u32,
        lpThreadId: *mut u32,
    ) -> *mut c_void;
}

unsafe extern "system" fn init_thread_proc(_param: *mut c_void) -> u32 {
    init_loader();
    0
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "system" fn DllMain(
    _hinst_dll: *mut c_void,
    fdw_reason: u32,
    _lp_reserved: *mut c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if fdw_reason == DLL_PROCESS_ATTACH {
        if !INITIALIZED.swap(true, Ordering::SeqCst) {
            unsafe {
                CreateThread(
                    std::ptr::null(),
                    0,
                    init_thread_proc,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                );
            }
        }
    }
    1
}
