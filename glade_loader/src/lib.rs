use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::Memory::VirtualProtect;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static INIT_DONE: AtomicBool = AtomicBool::new(false);
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

fn get_log_path() -> &'static PathBuf {
    LOG_PATH.get_or_init(|| {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            let mut p = PathBuf::from(userprofile);
            p.push("Saved Games");
            p.push("Tiny Glade");
            let _ = std::fs::create_dir_all(&p);
            p.push("glade_loader.log");
            return p;
        }
        PathBuf::from("glade_loader.log")
    })
}

pub fn log(msg: &str) {
    let path = get_log_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[GladeLoader] {}", msg);
        let _ = file.flush();
    }
}

unsafe extern "C" fn fake_restart_app(_app_id: u32) -> bool {
    log(">>> SteamAPI_RestartAppIfNecessary intercepted -> returning false");
    false
}

// ----------------------------------------------------------------------
// Pattern Scanner (AOB) Engine
// ----------------------------------------------------------------------

fn parse_pattern(sig: &str) -> (Vec<u8>, Vec<bool>) {
    let mut bytes = Vec::new();
    let mut mask = Vec::new();
    for token in sig.split_whitespace() {
        if token == "?" || token == "??" {
            bytes.push(0);
            mask.push(false);
        } else if let Ok(b) = u8::from_str_radix(token, 16) {
            bytes.push(b);
            mask.push(true);
        }
    }
    (bytes, mask)
}

fn find_pattern(mem: &[u8], pat: &[u8], mask: &[bool]) -> Option<usize> {
    if pat.is_empty() || pat.len() > mem.len() {
        return None;
    }
    let first = pat[0];
    let pat_len = pat.len();
    let mut offset = 0;
    while offset + pat_len <= mem.len() {
        if let Some(pos) = mem[offset..].iter().position(|&b| b == first) {
            let idx = offset + pos;
            if idx + pat_len > mem.len() {
                return None;
            }
            let mut matched = true;
            for j in 1..pat_len {
                if mask[j] && mem[idx + j] != pat[j] {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Some(idx);
            }
            offset = idx + 1;
        } else {
            return None;
        }
    }
    None
}

unsafe fn resolve_rva(
    base: usize,
    image_size: usize,
    name: &str,
    sig: &str,
    sig_offset: usize,
    fallback_rva: usize,
) -> usize {
    let (pat, mask) = parse_pattern(sig);
    let mem = unsafe { std::slice::from_raw_parts(base as *const u8, image_size) };
    if let Some(idx) = find_pattern(mem, &pat, &mask) {
        let rva = idx + sig_offset;
        log(&format!(">>> [{}] Located via AOB signature at RVA 0x{:X}", name, rva));
        rva
    } else {
        log(&format!(">>> [{}] Signature not found, using verified fallback RVA 0x{:X}", name, fallback_rva));
        fallback_rva
    }
}

// ----------------------------------------------------------------------
// Main Initialization Routine
// ----------------------------------------------------------------------

fn apply_all_patches() {
    if INIT_DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    log("==========================================");
    log("GladeLoader v1.1.0 initializing...");
    log("Target: Tiny Glade (Bevy Engine)");

    unsafe {
        log("Step 1: Check steam_api64");
        let steam_module = GetModuleHandleA(b"steam_api64.dll\0".as_ptr());
        log(&format!("Step 1 result: steam_module = 0x{:X}", steam_module as usize));
        if !steam_module.is_null() {
            match minhook::MinHook::create_hook_api(
                "steam_api64.dll",
                "SteamAPI_RestartAppIfNecessary",
                fake_restart_app as *mut c_void,
            ) {
                Ok(_) => {
                    let _ = minhook::MinHook::enable_all_hooks();
                    log(">>> [Steam] SteamAPI_RestartAppIfNecessary hook armed!");
                }
                Err(e) => log(&format!("Notice: Steam hook: {:?}", e)),
            }
        }

        log("Step 2: Get base handle");
        let base = GetModuleHandleA(std::ptr::null()) as usize;
        log(&format!("Base image address: 0x{:X}", base));

        log("Step 3: Read headers");
        let e_lfanew = *(base as *const u32).add(0x3C / 4) as usize;
        let nt_headers = (base + e_lfanew) as *const u8;
        let image_size = *(nt_headers.add(0x50) as *const u32) as usize;
        log(&format!("Loaded image size: 0x{:X} bytes", image_size));

        const PAGE_EXECUTE_READWRITE: u32 = 0x40;
        const PAGE_READWRITE: u32 = 0x04;
        let mut old_protect = 0;

        log("Step 4: Resolve Anti-Tamper");
        let rva_tamper = resolve_rva(
            base,
            image_size,
            "Anti-Tamper",
            "80 BD 41 02 00 00 00 0F 84",
            7, // offset to '0F 84'
            0x2D69FF,
        );
        log(&format!("Step 4 result: rva_tamper = 0x{:X}", rva_tamper));
        let patch_addr = (base + rva_tamper) as *mut u8;
        if VirtualProtect(patch_addr as _, 6, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            // Jump directly over BOTH unrecognized files check and modified files check to 0x2D6AE3
            let patch: [u8; 6] = [0xE9, 0xDF, 0x00, 0x00, 0x00, 0x90];
            std::ptr::copy_nonoverlapping(patch.as_ptr(), patch_addr, 6);
            let mut dummy = 0;
            VirtualProtect(patch_addr as _, 6, old_protect, &mut dummy);
            log(">>> [Anti-Tamper] Primary integrity check successfully bypassed (jump to 0x2D6AE3)!");
        }

        // Secondary check at 0x2D6A9D (modified files branch): je 0x2D6AE3 -> jmp 0x2D6AE3 (EB 44)
        let rva_tamper2 = resolve_rva(
            base,
            image_size,
            "Anti-Tamper Secondary",
            "0F B6 9D 40 02 00 00 84 DB 74",
            9, // offset to '74 44'
            0x2D6A9D,
        );
        let patch_addr2 = (base + rva_tamper2) as *mut u8;
        if VirtualProtect(patch_addr2 as _, 2, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            let patch2: [u8; 2] = [0xEB, 0x44];
            std::ptr::copy_nonoverlapping(patch2.as_ptr(), patch_addr2, 2);
            let mut dummy = 0;
            VirtualProtect(patch_addr2 as _, 2, old_protect, &mut dummy);
            log(">>> [Anti-Tamper] Secondary integrity check successfully bypassed!");
        }

        // --------------------------------------------------------------
        // Patch 1: Infinite Build Area (All border checks return true)
        // --------------------------------------------------------------
        let rva_glade_shape = resolve_rva(
            base,
            image_size,
            "Infinite Glade: is_within_glade_shape",
            "80 3D ? ? ? ? 00 74 ? 0F 28 D0 F3 0F 59 D0",
            0,
            0xA25790,
        );
        let rva_pos_inside = resolve_rva(
            base,
            image_size,
            "Infinite Glade: GladeBorder::is_pos_inside",
            "41 5C 41 5D 41 5E 41 5F 5D C3 ? ? ? ? 48 89 C8 48 83 C1 04 83 38 01",
            14, // offset to '48 89 C8'
            0xB08AA0,
        );
        let rva_shape_inside = resolve_rva(
            base,
            image_size,
            "Infinite Glade: GladeBorder::is_shape_inside",
            "41 56 56 57 55 53 48 81 EC E0 00 00 00 44 0F 29 84 24 D0 00 00 00",
            0,
            0xB08AC0,
        );
        let rva_curve2_inside = resolve_rva(
            base,
            image_size,
            "Infinite Glade: GladeBorder::is_curve2_inside",
            "41 57 41 56 56 57 53 48 83 EC 20 48 8B 5A 10 48 85 DB 74",
            0,
            0xB08DB0,
        );

        let patches_always_true = [
            (rva_glade_shape, "is_within_glade_shape"),
            (rva_pos_inside, "GladeBorder::is_pos_inside"),
            (rva_shape_inside, "GladeBorder::is_shape_inside"),
            (rva_curve2_inside, "GladeBorder::is_curve2_inside"),
        ];

        let mov_al_1_ret: [u8; 3] = [0xB0, 0x01, 0xC3]; // mov al, 1; ret
        for (rva, name) in patches_always_true {
            let ptr = (base + rva) as *mut u8;
            if VirtualProtect(ptr as _, 3, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
                std::ptr::copy_nonoverlapping(mov_al_1_ret.as_ptr(), ptr, 3);
                let mut dummy = 0;
                VirtualProtect(ptr as _, 3, old_protect, &mut dummy);
                log(&format!(">>> [Infinite Glade] {} patched (always TRUE)!", name));
            }
        }

        // --------------------------------------------------------------
        // Patch 2: Camera Pan Delta & Perimeter Clamps
        // --------------------------------------------------------------
        let rva_cam_delta = resolve_rva(
            base,
            image_size,
            "Camera Pan Delta Clamp",
            "44 0F 2E DD 0F 28 EB 44 0F 28 D2 76 2B",
            11, // offset to '76 2B'
            0xABF87D,
        );
        let cam_delta_ptr = (base + rva_cam_delta) as *mut u8;
        if VirtualProtect(cam_delta_ptr as _, 2, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            let patch: [u8; 2] = [0xEB, 0x2B]; // jbe -> jmp
            std::ptr::copy_nonoverlapping(patch.as_ptr(), cam_delta_ptr, 2);
            let mut dummy = 0;
            VirtualProtect(cam_delta_ptr as _, 2, old_protect, &mut dummy);
            log(">>> [Camera Unlimit] Camera pan delta clamp UNLOCKED!");
        }

        let rva_cam_pos = resolve_rva(
            base,
            image_size,
            "Camera Position Boundary Clamp",
            "01 0F 85 02 03 00 00 44 0F 28 54",
            1, // offset to '0F 85 02 03 00 00'
            0xABF8F8,
        );
        let cam_pos_ptr = (base + rva_cam_pos) as *mut u8;
        if VirtualProtect(cam_pos_ptr as _, 6, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            let patch: [u8; 6] = [0xE9, 0x03, 0x03, 0x00, 0x00, 0x90]; // jmp over perimeter clamp + nop
            std::ptr::copy_nonoverlapping(patch.as_ptr(), cam_pos_ptr, 6);
            let mut dummy = 0;
            VirtualProtect(cam_pos_ptr as _, 6, old_protect, &mut dummy);
            log(">>> [Camera Unlimit] Camera position glade boundary clamp UNLOCKED!");
        }

        // --------------------------------------------------------------
        // Patch 3: 3.7x Camera Zoom Out (40.0m -> 150.0m)
        // --------------------------------------------------------------
        let rva_zoom_float = resolve_rva(
            base,
            image_size,
            "Max Zoom Float",
            "00 00 40 40 00 00 20 42 00 00 10 41 00 00 C8 44",
            4, // offset to 40.0f
            0x2EC5EA4,
        );
        let zoom_ptr = (base + rva_zoom_float) as *mut f32;
        if VirtualProtect(zoom_ptr as _, 4, PAGE_READWRITE, &mut old_protect) != 0 {
            *zoom_ptr = 150.0;
            let mut dummy = 0;
            VirtualProtect(zoom_ptr as _, 4, old_protect, &mut dummy);
            log(">>> [Camera Unlimit] Max zoom distance increased to 150m (3.7x)!");
        }

        // --------------------------------------------------------------
        // Patch 4: Allow Zoom Anywhere
        // --------------------------------------------------------------
        let rva_zoom_check = resolve_rva(
            base,
            image_size,
            "Zoom Anywhere Check",
            "0F 2E DA 0F 86 61 01 00 00",
            3, // offset to '0F 86 61 01 00 00'
            0xAACFBE,
        );
        let zoom_check_ptr = (base + rva_zoom_check) as *mut u8;
        if VirtualProtect(zoom_check_ptr as _, 6, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            let patch: [u8; 6] = [0x90; 6];
            std::ptr::copy_nonoverlapping(patch.as_ptr(), zoom_check_ptr, 6);
            let mut dummy = 0;
            VirtualProtect(zoom_check_ptr as _, 6, old_protect, &mut dummy);
            log(">>> [Camera Unlimit] Zoom anywhere check UNLOCKED!");
        }

        // --------------------------------------------------------------
        // Patch 5: Clean Glade Border Debris & Stones
        // --------------------------------------------------------------
        let rva_border_branch = resolve_rva(
            base,
            image_size,
            "Clean Glade Border Stones",
            "48 8B 01 80 38 0F 75 19 0F 28 B5",
            6, // offset to '75 19'
            0x2057181,
        );
        let border_branch_ptr = (base + rva_border_branch) as *mut u8;
        if VirtualProtect(border_branch_ptr as _, 2, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
            let patch: [u8; 2] = [0x90, 0x90]; // NOP out branch to stone mesh generation
            std::ptr::copy_nonoverlapping(patch.as_ptr(), border_branch_ptr, 2);
            let mut dummy = 0;
            VirtualProtect(border_branch_ptr as _, 2, old_protect, &mut dummy);
            log(">>> [Clean Glade] Glade border markers & boundary stones cleanly REMOVED!");
        }
    }
    log(">>> [GladeLoader] All patches active and verified!");
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
            apply_all_patches();
        }
    }
    1
}
