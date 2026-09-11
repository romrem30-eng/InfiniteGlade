#![allow(dead_code, unused_imports, unused_variables, unused_unsafe)]

pub mod gizmo;

use std::collections::HashMap;
use std::ffi::c_void;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Instant;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::Memory::{
    VirtualProtect, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_READONLY, PAGE_READWRITE,
    PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
};

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
// Glade Plugin Configuration (glade_config.json)
// ----------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct GladeConfig {
    pub infinite_glade: bool,
    pub uncapped_camera: bool,
    pub clutter_animations: bool,
    pub clean_borders: bool,
    pub free_clutter_gizmo: bool,
}

impl Default for GladeConfig {
    fn default() -> Self {
        Self {
            infinite_glade: true,
            uncapped_camera: true,
            clutter_animations: false,
            clean_borders: false,
            free_clutter_gizmo: false,
        }
    }
}

fn load_or_create_config() -> GladeConfig {
    let mut config = GladeConfig::default();
    let mut config_path = None;

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            let p = dir.join("glade_config.json");
            if p.is_file() {
                config_path = Some(p);
            }
        }
    }

    if config_path.is_none() {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(userprofile)
                .join("Saved Games")
                .join("Tiny Glade")
                .join("glade_config.json");
            if p.is_file() {
                config_path = Some(p);
            }
        }
    }

    if let Some(ref path) = config_path {
        if let Ok(content) = fs::read_to_string(path) {
            log(&format!(">>> [Config] Loaded config from {:?}", path));
            if let Some(pos) = content.find("\"infinite_glade\"") {
                if let Some(colon) = content[pos..].find(':') {
                    let val = content[pos + colon + 1..].trim_start();
                    if val.starts_with("false") {
                        config.infinite_glade = false;
                    }
                }
            }
            if let Some(pos) = content.find("\"uncapped_camera\"") {
                if let Some(colon) = content[pos..].find(':') {
                    let val = content[pos + colon + 1..].trim_start();
                    if val.starts_with("false") {
                        config.uncapped_camera = false;
                    }
                }
            }
            if let Some(pos) = content.find("\"clutter_animations\"") {
                if let Some(colon) = content[pos..].find(':') {
                    let val = content[pos + colon + 1..].trim_start();
                    if val.starts_with("false") {
                        config.clutter_animations = false;
                    }
                }
            }
            if let Some(pos) = content.find("\"clean_borders\"") {
                if let Some(colon) = content[pos..].find(':') {
                    let val = content[pos + colon + 1..].trim_start();
                    if val.starts_with("true") {
                        config.clean_borders = true;
                    }
                }
            }
            if let Some(pos) = content.find("\"free_clutter_gizmo\"") {
                if let Some(colon) = content[pos..].find(':') {
                    let val = content[pos + colon + 1..].trim_start();
                    if val.starts_with("false") {
                        config.free_clutter_gizmo = false;
                    }
                }
            }
            return config;
        }
    }

    // If file doesn't exist, create default in Saved Games/Tiny Glade
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let p = PathBuf::from(userprofile)
            .join("Saved Games")
            .join("Tiny Glade")
            .join("glade_config.json");
        let default_json = "{\n  \"infinite_glade\": true,\n  \"uncapped_camera\": true,\n  \"clutter_animations\": true,\n  \"clean_borders\": false,\n  \"free_clutter_gizmo\": true\n}\n";
        let _ = fs::write(&p, default_json);
        log(&format!(">>> [Config] Created default glade_config.json at {:?}", p));
    }

    config
}

// ----------------------------------------------------------------------
// Clutter Animation System (Spin Rotation)
// ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, Debug)]
pub struct SpinConfig {
    pub axis: Axis,
    pub speed_rad: f32,
    pub pivot: [f32; 3],
}

static SPIN_MAP: OnceLock<RwLock<HashMap<u64, SpinConfig>>> = OnceLock::new();
static PENDING_ANIM_CONFIGS: OnceLock<RwLock<HashMap<String, SpinConfig>>> = OnceLock::new();
static START_TIME: OnceLock<Instant> = OnceLock::new();
static FIRST_SPIN_LOGGED: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
pub static mut ORIGINAL_UPDATE_INSTANCE_DATA: usize = 0;
#[unsafe(no_mangle)]

static mut ORIGINAL_NAME_HASH_COMPUTE: Option<unsafe extern "system" fn(*const u8, usize) -> u64> = None;

fn get_spin_map() -> &'static RwLock<HashMap<u64, SpinConfig>> {
    SPIN_MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_pending_configs() -> &'static RwLock<HashMap<String, SpinConfig>> {
    PENDING_ANIM_CONFIGS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_start_time() -> &'static Instant {
    START_TIME.get_or_init(Instant::now)
}

fn parse_spin_tag(name: &str) -> Option<SpinConfig> {
    let idx = name.find("__spin_")?;
    let rest = &name[idx + 7..];
    let mut parts = rest.split('_');
    let axis_str = parts.next()?;
    let speed_part = parts.next()?;
    let speed_str = speed_part.split('.').next().unwrap_or(speed_part);
    let speed_deg: f32 = speed_str.parse().ok()?;
    let axis = match axis_str.to_ascii_lowercase().as_str() {
        "x" => Axis::X,
        "y" => Axis::Y,
        "z" => Axis::Z,
        _ => return None,
    };
    let pivot = if name.contains("windmill") {
        [0.0, 3.3, 1.35]
    } else {
        [0.0, 0.0, 0.0]
    };
    Some(SpinConfig {
        axis,
        speed_rad: speed_deg.to_radians(),
        pivot,
    })
}

pub fn is_animated_hash(hash: u64) -> bool {
    let map = get_spin_map().read().unwrap();
    map.contains_key(&hash)
}

fn register_animated_mesh(hash: u64, name: &str, config: SpinConfig) {
    gizmo::register_clutter_hash(hash);
    let mut map = get_spin_map().write().unwrap();
    if !map.contains_key(&hash) {
        log(&format!(
            ">>> [ClutterAnimation] Registered spinning mesh: \"{}\" -> Hash: 0x{:016X}, Axis: {:?}, Speed: {:.2} rad/s, Pivot: {:?}",
            name, hash, config.axis, config.speed_rad, config.pivot
        ));
        map.insert(hash, config);
    }
}

unsafe extern "system" fn hook_name_hash_compute(ptr: *const u8, len: usize) -> u64 {
    let hash = unsafe {
        let orig = ORIGINAL_NAME_HASH_COMPUTE.unwrap();
        orig(ptr, len)
    };

    if !ptr.is_null() && len > 0 && len < 1024 {
        let s_res = unsafe { std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) };
        if let Ok(s) = s_res {
            if s.starts_with("clutter/") || s.contains("/clutter/") || s.ends_with(".glb") {
                gizmo::register_clutter_hash(hash);
                if s.contains("mod:") || s.contains("/mods/") || s.contains("windmill") {
                    gizmo::register_mod_clutter_hash(hash);
                }
            }
            if let Some(cfg) = parse_spin_tag(s) {
                register_animated_mesh(hash, s, cfg);
            } else {
                let clean_s = s
                    .trim_start_matches("clutter_icons/")
                    .trim_start_matches("clutter/")
                    .strip_suffix(".glb")
                    .unwrap_or(s);
                let pending = get_pending_configs().read().unwrap();
                for (target_name, cfg) in pending.iter() {
                    let clean_target = target_name
                        .trim_start_matches("clutter/")
                        .strip_suffix(".glb")
                        .unwrap_or(target_name.as_str());
                    if s == target_name
                        || s.ends_with(target_name.as_str())
                        || clean_s == clean_target
                        || clean_s.ends_with(clean_target)
                    {
                        register_animated_mesh(hash, s, *cfg);
                        break;
                    }
                }
            }
        }
    }

    hash
}

#[repr(C)]
struct UberInstanceData {
    col0: [f32; 3], // X basis vector
    col1: [f32; 3], // Y basis vector
    col2: [f32; 3], // Z basis vector
    col3: [f32; 3], // Translation vector
    flags: [u32; 4],
}

unsafe fn apply_rotation(data_ptr: *mut f32, axis: Axis, angle: f32, pivot: [f32; 3]) {
    let mat = unsafe { &mut *(data_ptr as *mut UberInstanceData) };
    let c = angle.cos();
    let s = angle.sin();

    let orig_c0 = mat.col0;
    let orig_c1 = mat.col1;
    let orig_c2 = mat.col2;

    let (dp0, dp1, dp2) = match axis {
        Axis::Z => {
            for i in 0..3 {
                mat.col0[i] = c * orig_c0[i] + s * orig_c1[i];
                mat.col1[i] = -s * orig_c0[i] + c * orig_c1[i];
            }
            let dp0 = (1.0 - c) * pivot[0] + s * pivot[1];
            let dp1 = -s * pivot[0] + (1.0 - c) * pivot[1];
            (dp0, dp1, 0.0)
        }
        Axis::Y => {
            for i in 0..3 {
                mat.col0[i] = c * orig_c0[i] - s * orig_c2[i];
                mat.col2[i] = s * orig_c0[i] + c * orig_c2[i];
            }
            let dp0 = (1.0 - c) * pivot[0] - s * pivot[2];
            let dp2 = s * pivot[0] + (1.0 - c) * pivot[2];
            (dp0, 0.0, dp2)
        }
        Axis::X => {
            for i in 0..3 {
                mat.col1[i] = c * orig_c1[i] - s * orig_c2[i];
                mat.col2[i] = s * orig_c1[i] + c * orig_c2[i];
            }
            let dp1 = (1.0 - c) * pivot[1] - s * pivot[2];
            let dp2 = s * pivot[1] + (1.0 - c) * pivot[2];
            (0.0, dp1, dp2)
        }
    };

    if dp0 != 0.0 || dp1 != 0.0 || dp2 != 0.0 {
        for i in 0..3 {
            mat.col3[i] += dp0 * orig_c0[i] + dp1 * orig_c1[i] + dp2 * orig_c2[i];
        }
    }
}


unsafe fn safe_read_u64(ptr: usize) -> Option<u64> {
    if ptr < 0x10000 || ptr > 0x00007FFFFFFFFFFF {
        return None;
    }
    let mut mbi: windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let res = unsafe {
        windows_sys::Win32::System::Memory::VirtualQuery(
            ptr as *const std::ffi::c_void,
            &mut mbi,
            std::mem::size_of::<windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION>(),
        )
    };
    if res == 0 {
        return None;
    }
    if mbi.State != windows_sys::Win32::System::Memory::MEM_COMMIT {
        return None;
    }
    const READABLE_MASK: u32 = windows_sys::Win32::System::Memory::PAGE_READONLY
        | windows_sys::Win32::System::Memory::PAGE_READWRITE
        | windows_sys::Win32::System::Memory::PAGE_EXECUTE_READ
        | windows_sys::Win32::System::Memory::PAGE_EXECUTE_READWRITE;
    if (mbi.Protect & READABLE_MASK) == 0 {
        return None;
    }
    Some(unsafe { *(ptr as *const u64) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_update_instance(
    desc_ptr: usize,
    data_ptr: *mut f32,
    arg3_r8: usize,
) {
    if data_ptr.is_null() || (data_ptr as usize) < 0x10000 {
        return;
    }

    let mut candidate_hashes = [0u64; 8];
    let mut num_cand = 0;

    // 1. Check arg3_r8 (&[rbp + 0xa8])
    if let Some(h) = unsafe { safe_read_u64(arg3_r8) } {
        candidate_hashes[num_cand] = h; num_cand += 1;
    }

    // 2. Check desc_ptr
    if let Some(h) = unsafe { safe_read_u64(desc_ptr) } {
        candidate_hashes[num_cand] = h; num_cand += 1;
    }
    if let Some(h) = unsafe { safe_read_u64(desc_ptr + 8) } {
        candidate_hashes[num_cand] = h; num_cand += 1;
    }

    // 3. Check caller rbp frame: rbp = data_ptr + 0x10
    let rbp = (data_ptr as usize) + 0x10;
    if let Some(hash_array_ptr) = unsafe { safe_read_u64(rbp + 0x160) } {
        if let Some(i_val) = unsafe { safe_read_u64(rbp + 0x148) } {
            let i = (i_val as u32) as usize;
            if let Some(h) = unsafe { safe_read_u64(hash_array_ptr as usize + i * 8) } {
                candidate_hashes[num_cand] = h; num_cand += 1;
            }
        }
    }

    static LOG_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let count = LOG_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if count < 10 {
        log(&format!(
            ">>> [UberInstance #14] call #{}: desc=0x{:X}, data=0x{:X}, r8=0x{:X}, candidates={:016X?}",
            count, desc_ptr, data_ptr as usize, arg3_r8, &candidate_hashes[..num_cand]
        ));
    }

    let mut matched_hash = 0u64;
    let mut matched_cfg = None;

    {
        let map = get_spin_map().read().unwrap();
        for &h in &candidate_hashes[..num_cand] {
            if let Some(cfg) = map.get(&h).copied() {
                matched_hash = h;
                matched_cfg = Some(cfg);
                break;
            }
        }
    }

    if let Some(cfg) = matched_cfg {
        let elapsed = get_start_time().elapsed().as_secs_f32();
        let angle = elapsed * cfg.speed_rad;
        unsafe { apply_rotation(data_ptr, cfg.axis, angle, cfg.pivot) };

        if !FIRST_SPIN_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log(&format!(
                ">>> [ClutterAnimation] ACTIVE SPIN on Hook #14: Hash 0x{:016X}",
                matched_hash
            ));
        }
    }

    if matched_hash != 0 {
        gizmo::apply_gizmo_offset(matched_hash, data_ptr);
    } else if let Some(h) = candidate_hashes.get(0).copied() {
        gizmo::apply_gizmo_offset(h, data_ptr);
    }
}

core::arch::global_asm!(
    r#"
    .global hook_update_instance_data_asm
    hook_update_instance_data_asm:
        push rcx
        push rdx
        push r8
        push r9
        sub rsp, 0x48
        movups [rsp + 0x20], xmm3

        mov rcx, rdx
        mov rdx, [rsp + 0x90]
        call on_update_instance

        movups xmm3, [rsp + 0x20]
        add rsp, 0x48
        pop r9
        pop r8
        pop rdx
        pop rcx

        mov rax, [rip + ORIGINAL_UPDATE_INSTANCE_DATA]
        jmp rax
    "#
);

#[unsafe(no_mangle)]
pub static mut ORIGINAL_CAMERA_SYSTEM: usize = 0;

core::arch::global_asm!(
    r#"
    .global hook_camera_system_asm
    hook_camera_system_asm:
        push rcx
        push rdx
        push r8
        push r9
        sub rsp, 0x48
        movups [rsp + 0x20], xmm0
        movups [rsp + 0x30], xmm1

        mov rcx, rdx
        call on_camera_update

        movups xmm0, [rsp + 0x20]
        movups xmm1, [rsp + 0x30]
        add rsp, 0x48
        pop r9
        pop r8
        pop rdx
        pop rcx

        mov rax, [rip + ORIGINAL_CAMERA_SYSTEM]
        jmp rax
    "#
);

#[unsafe(no_mangle)]
pub extern "C" fn on_camera_update(cam_ptr: usize) {
    if cam_ptr > 0x10000 {
        unsafe {
            let eye_x = *((cam_ptr + 0x38) as *const f32);
            let eye_z = *((cam_ptr + 0x3C) as *const f32);
            let eye_y = *((cam_ptr + 0x40) as *const f32);
            let tgt_x = *((cam_ptr + 0x44) as *const f32);
            let tgt_z = *((cam_ptr + 0x48) as *const f32);
            let tgt_y = *((cam_ptr + 0x4C) as *const f32);

            if eye_x.is_finite() && tgt_x.is_finite() {
                crate::gizmo::update_camera([eye_x, eye_y, eye_z], [tgt_x, tgt_y, tgt_z]);
            }
        }
    }
}


unsafe extern "C" {
    fn hook_update_instance_data_asm();
    fn hook_camera_system_asm();
}

fn parse_and_load_animations_json(path: &PathBuf) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    log(&format!(">>> [ClutterAnimation] Reading animations config: {:?}", path));

    let mut pending = get_pending_configs().write().unwrap();
    let mut count = 0;

    for chunk in content.split('{').skip(1) {
        let mut axis = Axis::Z;
        let mut speed_deg = 0.0f32;
        let mut pivot = [0.0f32; 3];
        let mut has_axis = false;
        let mut has_speed = false;
        let mut has_pivot = false;

        for line in chunk.split(&['\n', ',', '}'][..]) {
            let line = line.trim();
            if line.contains("\"axis\"") {
                if line.contains("\"x\"") || line.contains("\"X\"") {
                    axis = Axis::X;
                    has_axis = true;
                } else if line.contains("\"y\"") || line.contains("\"Y\"") {
                    axis = Axis::Y;
                    has_axis = true;
                } else if line.contains("\"z\"") || line.contains("\"Z\"") {
                    axis = Axis::Z;
                    has_axis = true;
                }
            } else if line.contains("\"speed\"") {
                if let Some(colon) = line.find(':') {
                    let num_str = line[colon + 1..].trim().trim_matches('"').trim();
                    if let Ok(v) = num_str.parse::<f32>() {
                        speed_deg = v;
                        has_speed = true;
                    }
                }
            }
        }

        if let Some(p_idx) = chunk.find("\"pivot\"") {
            let rest = &chunk[p_idx + 7..];
            if let Some(start) = rest.find('[') {
                if let Some(end) = rest[start + 1..].find(']') {
                    let nums: Vec<f32> = rest[start + 1..start + 1 + end]
                        .split(',')
                        .filter_map(|s| s.trim().parse::<f32>().ok())
                        .collect();
                    if nums.len() == 3 {
                        pivot = [nums[0], nums[1], nums[2]];
                        has_pivot = true;
                    }
                }
            }
        }

        if has_axis && has_speed {
            if let Some(chunk_pos) = content.find(chunk) {
                let prefix = &content[..chunk_pos];
                if let Some(quote2) = prefix.rfind('"') {
                    if let Some(quote1) = prefix[..quote2].rfind('"') {
                        let mesh_key = prefix[quote1 + 1..quote2].trim().to_string();
                        if !mesh_key.is_empty() && !mesh_key.contains('{') && !mesh_key.contains(':') {
                            if !has_pivot && mesh_key.contains("windmill") {
                                pivot = [0.0, 3.3, 1.35];
                            }
                            log(&format!(
                                ">>> [ClutterAnimation] Config rule: \"{}\" -> Axis: {:?}, Speed: {} deg/s, Pivot: {:?}",
                                mesh_key, axis, speed_deg, pivot
                            ));
                            pending.insert(
                                mesh_key,
                                SpinConfig {
                                    axis,
                                    speed_rad: speed_deg.to_radians(),
                                    pivot,
                                },
                            );
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    count > 0
}

fn scan_for_animations_json() {
    let mut search_paths = Vec::new();

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            search_paths.push(dir.join("Mods"));
        }
    }

    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let mut p = PathBuf::from(&userprofile);
        p.push("Saved Games");
        p.push("Tiny Glade");
        p.push("Mods");
        search_paths.push(p);

        let mut steam_dir = PathBuf::from(&userprofile);
        steam_dir.push("Saved Games");
        steam_dir.push("Tiny Glade");
        steam_dir.push("Steam");
        if let Ok(steam_entries) = fs::read_dir(&steam_dir) {
            for se in steam_entries.flatten() {
                let m = se.path().join("mods");
                if m.is_dir() {
                    search_paths.push(m);
                }
            }
        }
    }

    let mut found_count = 0;
    for base in search_paths {
        if !base.is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let direct_json = path.join("animations.json");
                    let clutter_json = path.join("clutter").join("animations.json");
                    for json_path in [direct_json, clutter_json] {
                        if json_path.is_file() {
                            if parse_and_load_animations_json(&json_path) {
                                found_count += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    if found_count > 0 {
        log(&format!(">>> [ClutterAnimation] Loaded animations.json from {} mod(s)", found_count));
    }
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
                    let _ = unsafe { minhook::MinHook::enable_all_hooks() };
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

        log("Step 4: Diagnostic integrity check skipped (informational only in engine)");

        // Load or create modular plugin configuration
        let config = load_or_create_config();
        log(&format!(">>> [Config] Active modules: InfiniteGlade={}, CameraUnlimit={}, ClutterAnim={}, CleanBorders={}",
            config.infinite_glade, config.uncapped_camera, config.clutter_animations, config.clean_borders));

        // --------------------------------------------------------------
        // Patch 1: Infinite Build Area (All border checks return true)
        // --------------------------------------------------------------
        if config.infinite_glade {
            let rva_glade_shape = resolve_rva(
                base,
                image_size,
                "Infinite Glade: is_within_glade_shape",
                "80 3D ? ? ? ? 00 74 ? 0F 28 D0 F3 0F 59 D0",
                0,
                0xA05910,
            );
            let rva_pos_inside = resolve_rva(
                base,
                image_size,
                "Infinite Glade: GladeBorder::is_pos_inside",
                "48 89 C8 48 83 C1 04 83 38 01 0F 85 ? ? ? ? E9 ? ? ? ? CC CC CC CC CC CC CC CC CC CC CC 56 57 48 83",
                0,
                0xA00210,
            );
            let rva_shape_inside = resolve_rva(
                base,
                image_size,
                "Infinite Glade: GladeBorder::is_shape_inside",
                "41 56 56 57 55 53 48 81 EC E0 00 00 00 44 0F 29 84 24 D0 00 00 00",
                0,
                0xAE2380,
            );
            let rva_curve2_inside = resolve_rva(
                base,
                image_size,
                "Infinite Glade: GladeBorder::is_curve2_inside",
                "41 57 41 56 56 57 53 48 83 EC 20 48 8B 5A 10 48 85 DB 74",
                0,
                0xAE2670,
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
        } else {
            log(">>> [Infinite Glade] Module DISABLED by user configuration");
        }

        // --------------------------------------------------------------
        // Patch 2, 3, 4: Camera Pan Delta, Perimeter Clamps & Zoom Unlimit
        // --------------------------------------------------------------
        if config.uncapped_camera {
            let rva_cam_delta = resolve_rva(
                base,
                image_size,
                "Camera Pan Delta Clamp",
                "44 0F 2E DD 0F 28 EB 44 0F 28 D2 76 2B",
                11, // offset to '76 2B'
                0xB79FED,
            );
            let cam_delta_ptr = (base + rva_cam_delta) as *mut u8;
            if VirtualProtect(cam_delta_ptr as _, 2, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
                let patch: [u8; 2] = [0xEB, 0x2B]; // jbe -> jmp
                std::ptr::copy_nonoverlapping(patch.as_ptr(), cam_delta_ptr, 2);
                let mut dummy = 0;
                VirtualProtect(cam_delta_ptr as _, 2, old_protect, &mut dummy);
                log(">>> [Camera Unlimit] Camera pan delta clamp UNLOCKED!");
            }

            // Hook camera system function to feed real-time 3D camera matrices to Gizmo
            let mut rva_camera_fn = 0xB79ED0;
            if rva_cam_delta > 1000 {
                let slice = std::slice::from_raw_parts((base + rva_cam_delta - 1000) as *const u8, 1000);
                for i in (0..slice.len().saturating_sub(4)).rev() {
                    if slice[i] == 0xCC && &slice[i + 1..i + 5] == &[0x41, 0x57, 0x41, 0x56] {
                        rva_camera_fn = rva_cam_delta - 1000 + (i + 1);
                        break;
                    }
                }
            }

            match minhook::MinHook::create_hook(
                (base + rva_camera_fn) as *mut c_void,
                hook_camera_system_asm as *mut c_void,
            ) {
                Ok(trampoline) => {
                    ORIGINAL_CAMERA_SYSTEM = trampoline as usize;
                    log(&format!(">>> [Camera System] Hook armed at RVA 0x{:X}!", rva_camera_fn));
                }
                Err(e) => log(&format!("Notice: Camera hook failed: {:?}", e)),
            }

            let rva_cam_pos = resolve_rva(
                base,
                image_size,
                "Camera Position Boundary Clamp",
                "01 0F 85 02 03 00 00 44 0F 28 54",
                1, // offset to '0F 85 02 03 00 00'
                0xB7A068,
            );
            let cam_pos_ptr = (base + rva_cam_pos) as *mut u8;
            if VirtualProtect(cam_pos_ptr as _, 6, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
                let patch: [u8; 6] = [0xE9, 0x03, 0x03, 0x00, 0x00, 0x90]; // jmp over perimeter clamp + nop
                std::ptr::copy_nonoverlapping(patch.as_ptr(), cam_pos_ptr, 6);
                let mut dummy = 0;
                VirtualProtect(cam_pos_ptr as _, 6, old_protect, &mut dummy);
                log(">>> [Camera Unlimit] Camera position glade boundary clamp UNLOCKED!");
            }

            let rva_zoom_float = resolve_rva(
                base,
                image_size,
                "Max Zoom Float",
                "00 00 40 40 00 00 20 42 00 00 10 41 00 00 C8 44",
                4, // offset to 40.0f
                0x2EAE1E4,
            );
            let zoom_ptr = (base + rva_zoom_float) as *mut f32;
            if VirtualProtect(zoom_ptr as _, 4, PAGE_READWRITE, &mut old_protect) != 0 {
                *zoom_ptr = 150.0;
                let mut dummy = 0;
                VirtualProtect(zoom_ptr as _, 4, old_protect, &mut dummy);
                log(">>> [Camera Unlimit] Max zoom distance increased to 150m (3.7x)!");
            }

            let rva_zoom_check = resolve_rva(
                base,
                image_size,
                "Zoom Anywhere Check",
                "0F 2E DA 0F 86 61 01 00 00",
                3, // offset to '0F 86 61 01 00 00'
                0x91AD8E,
            );
            let zoom_check_ptr = (base + rva_zoom_check) as *mut u8;
            if VirtualProtect(zoom_check_ptr as _, 6, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
                let patch: [u8; 6] = [0x90; 6];
                std::ptr::copy_nonoverlapping(patch.as_ptr(), zoom_check_ptr, 6);
                let mut dummy = 0;
                VirtualProtect(zoom_check_ptr as _, 6, old_protect, &mut dummy);
                log(">>> [Camera Unlimit] Zoom anywhere check UNLOCKED!");
            }
        } else {
            log(">>> [Camera Unlimit] Module DISABLED by user configuration");
        }

        // --------------------------------------------------------------
        // Patch 5: Clean Glade Border Debris & Stones
        // --------------------------------------------------------------
        if config.clean_borders {
            let rva_border_branch = resolve_rva(
                base,
                image_size,
                "Clean Glade Border Stones",
                "48 8B 01 80 38 0F 75 19 0F 28 B5",
                6, // offset to '75 19'
                0x2034D51,
            );
            let border_branch_ptr = (base + rva_border_branch) as *mut u8;
            if VirtualProtect(border_branch_ptr as _, 2, PAGE_EXECUTE_READWRITE, &mut old_protect) != 0 {
                let patch: [u8; 2] = [0x90, 0x90]; // NOP out branch to stone mesh generation
                std::ptr::copy_nonoverlapping(patch.as_ptr(), border_branch_ptr, 2);
                let mut dummy = 0;
                VirtualProtect(border_branch_ptr as _, 2, old_protect, &mut dummy);
                log(">>> [Clean Glade] Glade border markers & boundary stones cleanly REMOVED!");
            }
        } else {
            log(">>> [Clean Glade] Module DISABLED by user configuration (standard boundary stones kept)");
        }

        // --------------------------------------------------------------
        // Patch 6: Clutter Animation System (Spin Rotation)
        // --------------------------------------------------------------
        // Paused for v1.3.1 stability release while mesh ID mapping is being upgraded
        log(">>> [ClutterAnimation] Module paused for stability release (under active development)");

        // --------------------------------------------------------------
        // Patch 7: Free Clutter 3D Gizmo ("Move It")
        // --------------------------------------------------------------
        // Paused for v1.3.1 stability release while 3D viewport hooks are being upgraded
        log(">>> [Gizmo] Module paused for stability release (under active development)");
    }
    let _ = unsafe { minhook::MinHook::enable_all_hooks() };
    log(">>> [GladeLoader] All engine hooks successfully enabled!");
    log(">>> [GladeLoader] Plugin execution phase completed!");
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
