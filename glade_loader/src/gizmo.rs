use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::log;

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBeep(u_type: u32) -> i32;
    fn ScreenToClient(h_wnd: windows_sys::Win32::Foundation::HWND, lp_point: *mut POINT) -> i32;
}

#[derive(Clone, Copy, Debug)]
pub struct CameraData {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub last_updated: Instant,
}

#[derive(Clone, Copy, Debug)]
struct ScanCandidate {
    key: u64,
    dist: f32,
}

struct ScanState {
    mouse_x: f32,
    mouse_y: f32,
    win_w: f32,
    win_h: f32,
    frames_left: i32,
    best_candidate: Option<ScanCandidate>,
}

static GIZMO_ENABLED: AtomicBool = AtomicBool::new(false);
static GIZMO_ACTIVE: AtomicBool = AtomicBool::new(false);
static OFFSETS_DIRTY: AtomicBool = AtomicBool::new(false);

static OFFSETS: OnceLock<RwLock<HashMap<u64, [f32; 3]>>> = OnceLock::new();
static SELECTED_KEY: OnceLock<RwLock<Option<u64>>> = OnceLock::new();
static CAMERA_DATA: OnceLock<RwLock<Option<CameraData>>> = OnceLock::new();
static SCAN_STATE: OnceLock<RwLock<Option<ScanState>>> = OnceLock::new();

// Mouse drag state
static IS_MOUSE_DRAGGING: AtomicBool = AtomicBool::new(false);
static DRAG_START_POINT: OnceLock<RwLock<POINT>> = OnceLock::new();
static LIVE_MOUSE_DELTA: OnceLock<RwLock<[f32; 3]>> = OnceLock::new();

static GIZMO_START_TIME: OnceLock<Instant> = OnceLock::new();

fn get_offsets() -> &'static RwLock<HashMap<u64, [f32; 3]>> {
    OFFSETS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_selected_key() -> &'static RwLock<Option<u64>> {
    SELECTED_KEY.get_or_init(|| RwLock::new(None))
}

fn get_camera_data() -> &'static RwLock<Option<CameraData>> {
    CAMERA_DATA.get_or_init(|| RwLock::new(None))
}

fn get_scan_state() -> &'static RwLock<Option<ScanState>> {
    SCAN_STATE.get_or_init(|| RwLock::new(None))
}

pub fn update_camera(eye: [f32; 3], target: [f32; 3]) {
    let mut cam = get_camera_data().write().unwrap();
    *cam = Some(CameraData {
        eye,
        target,
        last_updated: Instant::now(),
    });
}

// Dummy stubs for backwards compatibility if referenced
pub fn register_clutter_hash(_hash: u64) {}
pub fn register_mod_clutter_hash(_hash: u64) {}
pub fn is_clutter_hash(_hash: u64) -> bool { true }
pub fn is_mod_clutter_hash(_hash: u64) -> bool { true }

fn get_offsets_path() -> PathBuf {
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        PathBuf::from(userprofile)
            .join("Saved Games")
            .join("Tiny Glade")
            .join("glade_gizmo_offsets.json")
    } else {
        PathBuf::from("glade_gizmo_offsets.json")
    }
}

pub fn make_instance_key(name_hash: u64, x: f32, z: f32) -> u64 {
    let qx = (x * 100.0).round() as i32;
    let qz = (z * 100.0).round() as i32;
    name_hash ^ (((qx as u64) << 32) | ((qz as u32) as u64))
}

pub fn load_offsets() {
    let path = get_offsets_path();
    if let Ok(content) = fs::read_to_string(&path) {
        let mut map = get_offsets().write().unwrap();
        for line in content.lines() {
            let line = line.trim();
            if let Some(colon) = line.find(':') {
                let key_str = line[..colon].trim().trim_matches('"');
                if let Ok(key) = key_str.parse::<u64>() {
                    let val_str = line[colon + 1..].trim().trim_matches(',').trim();
                    if val_str.starts_with('[') && val_str.ends_with(']') {
                        let nums: Vec<f32> = val_str[1..val_str.len() - 1]
                            .split(',')
                            .filter_map(|s| s.trim().parse::<f32>().ok())
                            .collect();
                        if nums.len() == 3 {
                            map.insert(key, [nums[0], nums[1], nums[2]]);
                        }
                    }
                }
            }
        }
        log(&format!(">>> [Gizmo] Loaded {} clutter 3D offsets from {:?}", map.len(), path));
    }
}

pub fn save_offsets() {
    let path = get_offsets_path();
    let snapshot = {
        let map = get_offsets().read().unwrap();
        map.clone()
    };

    let mut out = String::from("{\n");
    let mut first = true;
    for (k, v) in snapshot.iter() {
        if !first {
            out.push_str(",\n");
        }
        first = false;
        out.push_str(&format!("  \"{}\": [{:.3}, {:.3}, {:.3}]", k, v[0], v[1], v[2]));
    }
    out.push_str("\n}\n");
    let _ = fs::write(&path, out);
    OFFSETS_DIRTY.store(false, Ordering::SeqCst);
    log(&format!(">>> [Gizmo] Saved {} clutter offsets to {:?}", snapshot.len(), path));
}

/// Project world coordinates [x, y, z] to screen space [sx, sy] and compute distance to mouse cursor
fn project_to_screen_distance(
    prop_pos: [f32; 3],
    cam: &CameraData,
    mouse_x: f32,
    mouse_y: f32,
    win_w: f32,
    win_h: f32,
) -> Option<f32> {
    let fx = cam.target[0] - cam.eye[0];
    let fy = cam.target[1] - cam.eye[1];
    let fz = cam.target[2] - cam.eye[2];
    let flen = (fx * fx + fy * fy + fz * fz).sqrt();
    if flen < 0.001 {
        return None;
    }
    let (fx, fy, fz) = (fx / flen, fy / flen, fz / flen);

    // Right vector: cross(forward, [0, 1, 0])
    let rx = fz;
    let ry = 0.0;
    let rz = -fx;
    let rlen = (rx * rx + rz * rz).sqrt();
    if rlen < 0.001 {
        return None;
    }
    let (rx, rz) = (rx / rlen, rz / rlen);

    // Up vector: cross(right, forward)
    let ux = ry * fz - rz * fy;
    let uy = rz * fx - rx * fz;
    let uz = rx * fy - ry * fx;

    // Vector from camera eye to prop
    let dx = prop_pos[0] - cam.eye[0];
    let dy = prop_pos[1] - cam.eye[1];
    let dz = prop_pos[2] - cam.eye[2];

    // Depth along camera forward axis
    let depth = dx * fx + dy * fy + dz * fz;
    if depth <= 0.1 {
        return None; // Object is behind camera
    }

    // Camera space coordinates
    let cam_x = dx * rx + dy * ry + dz * rz;
    let cam_y = dx * ux + dy * uy + dz * uz;

    // Perspective projection (~45 deg vertical FOV)
    let aspect = (win_w / win_h).max(0.1);
    let fov_factor = 2.414_f32; // cot(45 deg / 2)
    let ndc_x = (cam_x * fov_factor) / (depth * aspect);
    let ndc_y = (cam_y * fov_factor) / depth;

    // Screen pixel coordinates
    let sx = (ndc_x * 0.5 + 0.5) * win_w;
    let sy = (0.5 - ndc_y * 0.5) * win_h;

    let dist = ((sx - mouse_x).powi(2) + (sy - mouse_y).powi(2)).sqrt();
    Some(dist)
}

pub fn apply_gizmo_offset(name_hash: u64, data_ptr: *mut f32) {
    if !GIZMO_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if data_ptr.is_null() || (data_ptr as usize) < 0x10000 {
        return;
    }

    unsafe {
        let col3_ptr = data_ptr.add(9);
        let orig_x = *col3_ptr;
        let orig_y = *col3_ptr.add(1);
        let orig_z = *col3_ptr.add(2);

        let key = make_instance_key(name_hash, orig_x, orig_z);

        // 1. If cursor scan is active, test this prop against the cursor!
        {
            let mut scan_guard = get_scan_state().write().unwrap();
            if let Some(scan) = scan_guard.as_mut() {
                if scan.frames_left > 0 {
                    let cam_opt = *get_camera_data().read().unwrap();
                    let dist = if let Some(cam) = cam_opt {
                        project_to_screen_distance(
                            [orig_x, orig_y, orig_z],
                            &cam,
                            scan.mouse_x,
                            scan.mouse_y,
                            scan.win_w,
                            scan.win_h,
                        ).unwrap_or(999999.0)
                    } else {
                        // Fallback: 3D distance to glade center / camera target
                        ((orig_x).powi(2) + (orig_z).powi(2)).sqrt()
                    };

                    let update_best = match scan.best_candidate {
                        Some(prev) => dist < prev.dist,
                        None => true,
                    };

                    if update_best {
                        scan.best_candidate = Some(ScanCandidate { key, dist });
                    }
                }
            }
        }

        // 2. Apply saved 3D offset [X, Y, Z]
        let saved_offset = {
            let map = get_offsets().read().unwrap();
            map.get(&key).copied()
        };
        if let Some(off) = saved_offset {
            *col3_ptr += off[0];
            *col3_ptr.add(1) += off[1];
            *col3_ptr.add(2) += off[2];
        }

        // 3. If Gizmo mode is active and this is the currently selected instance:
        if GIZMO_ACTIVE.load(Ordering::Relaxed) {
            let is_sel = {
                let sel = get_selected_key().read().unwrap();
                *sel == Some(key)
            };

            if is_sel {
                // Live mouse drag delta [dx, dy, dz]
                if IS_MOUSE_DRAGGING.load(Ordering::Relaxed) {
                    let m_delta = *LIVE_MOUSE_DELTA.get_or_init(|| RwLock::new([0.0, 0.0, 0.0])).read().unwrap();
                    *col3_ptr += m_delta[0];
                    *col3_ptr.add(1) += m_delta[1];
                    *col3_ptr.add(2) += m_delta[2];
                }

                // Visual bobbing cue (7cm up and down)
                let elapsed = GIZMO_START_TIME.get_or_init(Instant::now).elapsed().as_secs_f32();
                let bob = (elapsed * 6.0).sin() * 0.07;
                *col3_ptr.add(1) += bob;
            }
        }
    }
}

pub fn init_gizmo() {
    GIZMO_ENABLED.store(true, Ordering::SeqCst);
    load_offsets();

    std::thread::spawn(|| {
        log(">>> [Gizmo] Universal Clutter 3D Gizmo active (Point mouse at clutter & press 'G')");

        let mut prev_g_down = false;
        let mut last_g_toggle_time = Instant::now();

        let mut prev_r_down = false;
        let mut prev_numpad5_down = false;

        let mut prev_drag_down = false;
        let mut last_save_time = Instant::now();

        loop {
            std::thread::sleep(Duration::from_millis(16)); // ~60 Hz
            let dt = 0.016_f32;

            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.is_null() {
                continue;
            }

            // Decrement scan frame counter if active
            {
                let mut scan_guard = get_scan_state().write().unwrap();
                if let Some(scan) = scan_guard.as_mut() {
                    if scan.frames_left > 0 {
                        scan.frames_left -= 1;
                    }
                }
            }

            // 1. Toggle Gizmo with 'G' (0x47) with 250ms debounce
            let g_down = unsafe { (GetAsyncKeyState(0x47) as u16 & 0x8000) != 0 };
            if g_down && !prev_g_down && last_g_toggle_time.elapsed() > Duration::from_millis(250) {
                last_g_toggle_time = Instant::now();

                if GIZMO_ACTIVE.load(Ordering::SeqCst) {
                    // Turn OFF Gizmo mode
                    GIZMO_ACTIVE.store(false, Ordering::SeqCst);
                    *get_selected_key().write().unwrap() = None;
                    save_offsets();
                    unsafe { MessageBeep(0xFFFFFFFF) }; // Soft standard click
                    log(">>> [Gizmo] Mode DEACTIVATED. Offsets permanently saved.");
                } else {
                    // Turn ON Gizmo mode: Aim with mouse cursor!
                    let mut pt = POINT { x: 0, y: 0 };
                    unsafe { GetCursorPos(&mut pt) };
                    unsafe { ScreenToClient(hwnd, &mut pt) };

                    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                    unsafe { GetClientRect(hwnd, &mut rect) };
                    let win_w = ((rect.right - rect.left) as f32).max(1.0);
                    let win_h = ((rect.bottom - rect.top) as f32).max(1.0);

                    // Initiate cursor aim scan
                    {
                        let mut scan_guard = get_scan_state().write().unwrap();
                        *scan_guard = Some(ScanState {
                            mouse_x: pt.x as f32,
                            mouse_y: pt.y as f32,
                            win_w,
                            win_h,
                            frames_left: 3, // Scan for 3 frames (~48ms)
                            best_candidate: None,
                        });
                    }

                    // Wait 40ms to allow render frames to test all visible clutter
                    std::thread::sleep(Duration::from_millis(40));

                    // Read scan result
                    let candidate = {
                        let scan_guard = get_scan_state().read().unwrap();
                        scan_guard.as_ref().and_then(|s| s.best_candidate)
                    };

                    if let Some(cand) = candidate {
                        *get_selected_key().write().unwrap() = Some(cand.key);
                        GIZMO_ACTIVE.store(true, Ordering::SeqCst);
                        unsafe { MessageBeep(0) }; // High activation chime
                        log(&format!(
                            ">>> [Gizmo] Mode ACTIVE! Locked clutter prop: 0x{:016X} (Screen dist: {:.1}px). Controls: O/U (Up/Down), J/L (Left/Right), I/K (Fwd/Back)",
                            cand.key, cand.dist
                        ));
                    } else {
                        log(">>> [Gizmo] No clutter prop detected under cursor. Point directly at any clutter and press 'G'.");
                    }

                    // Clear scan state
                    *get_scan_state().write().unwrap() = None;
                }
            }
            prev_g_down = g_down;

            if !GIZMO_ACTIVE.load(Ordering::Relaxed) {
                if OFFSETS_DIRTY.load(Ordering::Relaxed) && last_save_time.elapsed() > Duration::from_millis(500) {
                    save_offsets();
                    last_save_time = Instant::now();
                }
                continue;
            }

            // 2. Smooth Continuous 3D Movement:
            let shift_down = unsafe { (GetAsyncKeyState(VK_SHIFT as i32) as u16 & 0x8000) != 0 };
            let ctrl_down = unsafe { (GetAsyncKeyState(VK_CONTROL as i32) as u16 & 0x8000) != 0 };
            let speed = if shift_down {
                3.5_f32
            } else if ctrl_down {
                0.25_f32
            } else {
                1.2_f32
            };

            // Axis Y: Up / Down
            let key_o = unsafe { (GetAsyncKeyState(0x4F) as u16 & 0x8000) != 0 };
            let key_bracket_close = unsafe { (GetAsyncKeyState(0xDD) as u16 & 0x8000) != 0 };
            let key_pageup = unsafe { (GetAsyncKeyState(VK_PRIOR as i32) as u16 & 0x8000) != 0 };
            let key_numpad_plus = unsafe { (GetAsyncKeyState(0x6B) as u16 & 0x8000) != 0 };
            let is_up = key_o || key_bracket_close || key_pageup || key_numpad_plus;

            let key_u = unsafe { (GetAsyncKeyState(0x55) as u16 & 0x8000) != 0 };
            let key_bracket_open = unsafe { (GetAsyncKeyState(0xDB) as u16 & 0x8000) != 0 };
            let key_pagedown = unsafe { (GetAsyncKeyState(VK_NEXT as i32) as u16 & 0x8000) != 0 };
            let key_numpad_minus = unsafe { (GetAsyncKeyState(0x6D) as u16 & 0x8000) != 0 };
            let is_down = key_u || key_bracket_open || key_pagedown || key_numpad_minus;

            // Axis X: Left / Right
            let key_j = unsafe { (GetAsyncKeyState(0x4A) as u16 & 0x8000) != 0 };
            let key_numpad4 = unsafe { (GetAsyncKeyState(0x64) as u16 & 0x8000) != 0 };
            let is_left = key_j || key_numpad4;

            let key_l = unsafe { (GetAsyncKeyState(0x4C) as u16 & 0x8000) != 0 };
            let key_numpad6 = unsafe { (GetAsyncKeyState(0x66) as u16 & 0x8000) != 0 };
            let is_right = key_l || key_numpad6;

            // Axis Z: Forward / Backward
            let key_i = unsafe { (GetAsyncKeyState(0x49) as u16 & 0x8000) != 0 };
            let key_numpad8 = unsafe { (GetAsyncKeyState(0x68) as u16 & 0x8000) != 0 };
            let is_fwd = key_i || key_numpad8;

            let key_k = unsafe { (GetAsyncKeyState(0x4B) as u16 & 0x8000) != 0 };
            let key_numpad2 = unsafe { (GetAsyncKeyState(0x62) as u16 & 0x8000) != 0 };
            let is_back = key_k || key_numpad2;

            let mut move_dx = 0.0_f32;
            let mut move_dy = 0.0_f32;
            let mut move_dz = 0.0_f32;

            if is_up { move_dy += speed * dt; }
            if is_down { move_dy -= speed * dt; }
            if is_right { move_dx += speed * dt; }
            if is_left { move_dx -= speed * dt; }
            if is_fwd { move_dz -= speed * dt; }
            if is_back { move_dz += speed * dt; }

            if move_dx != 0.0 || move_dy != 0.0 || move_dz != 0.0 {
                let sel_k = *get_selected_key().read().unwrap();
                if let Some(key) = sel_k {
                    {
                        let mut map = get_offsets().write().unwrap();
                        let entry = map.entry(key).or_insert([0.0, 0.0, 0.0]);
                        entry[0] += move_dx;
                        entry[1] = (entry[1] + move_dy).max(0.0);
                        entry[2] += move_dz;
                    }
                    OFFSETS_DIRTY.store(true, Ordering::SeqCst);
                    last_save_time = Instant::now();
                }
            }

            // 3. Reset 3D Offset: 'R' (0x52) or NumPad5 (0x65)
            let r_down = unsafe { (GetAsyncKeyState(0x52) as u16 & 0x8000) != 0 };
            let numpad5_down = unsafe { (GetAsyncKeyState(0x65) as u16 & 0x8000) != 0 };
            let is_reset = (r_down && !prev_r_down) || (numpad5_down && !prev_numpad5_down);

            if is_reset {
                let sel_k = *get_selected_key().read().unwrap();
                if let Some(key) = sel_k {
                    {
                        let mut map = get_offsets().write().unwrap();
                        map.remove(&key);
                        log(&format!(">>> [Gizmo] Offset reset to (0, 0, 0) for key 0x{:016X}", key));
                    }
                    save_offsets();
                    unsafe { MessageBeep(0xFFFFFFFF) };
                }
            }
            prev_r_down = r_down;
            prev_numpad5_down = numpad5_down;

            // 4. Mouse Drag (Alt + LMB or MMB)
            let mut pt = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut pt) };

            let lmb_down = unsafe { (GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000) != 0 };
            let mmb_down = unsafe { (GetAsyncKeyState(VK_MBUTTON as i32) as u16 & 0x8000) != 0 };
            let alt_down = unsafe { (GetAsyncKeyState(VK_MENU as i32) as u16 & 0x8000) != 0 };

            let is_drag_held = mmb_down || (alt_down && lmb_down);

            if is_drag_held && !prev_drag_down {
                let sel_k = *get_selected_key().read().unwrap();
                if sel_k.is_some() {
                    IS_MOUSE_DRAGGING.store(true, Ordering::SeqCst);
                    *DRAG_START_POINT.get_or_init(|| RwLock::new(POINT { x: 0, y: 0 })).write().unwrap() = pt;
                    *LIVE_MOUSE_DELTA.get_or_init(|| RwLock::new([0.0, 0.0, 0.0])).write().unwrap() = [0.0, 0.0, 0.0];
                }
            } else if !is_drag_held && prev_drag_down {
                if IS_MOUSE_DRAGGING.swap(false, Ordering::SeqCst) {
                    let m_delta = *LIVE_MOUSE_DELTA.get_or_init(|| RwLock::new([0.0, 0.0, 0.0])).read().unwrap();
                    let sel_k = *get_selected_key().read().unwrap();
                    if let Some(key) = sel_k {
                        if m_delta[0].abs() > 0.005 || m_delta[1].abs() > 0.005 || m_delta[2].abs() > 0.005 {
                            {
                                let mut map = get_offsets().write().unwrap();
                                let entry = map.entry(key).or_insert([0.0, 0.0, 0.0]);
                                entry[0] += m_delta[0];
                                entry[1] = (entry[1] + m_delta[1]).max(0.0);
                                entry[2] += m_delta[2];
                                log(&format!(">>> [Gizmo] Drag committed -> X: {:+.2}m, Y: {:+.2}m, Z: {:+.2}m", entry[0], entry[1], entry[2]));
                            }
                            save_offsets();
                        }
                    }
                    *LIVE_MOUSE_DELTA.get_or_init(|| RwLock::new([0.0, 0.0, 0.0])).write().unwrap() = [0.0, 0.0, 0.0];
                }
            } else if IS_MOUSE_DRAGGING.load(Ordering::Relaxed) {
                let start_pt = *DRAG_START_POINT.get_or_init(|| RwLock::new(POINT { x: 0, y: 0 })).read().unwrap();
                let scale = if shift_down { 0.06 } else if ctrl_down { 0.006 } else { 0.02 };

                let dx_px = (pt.x - start_pt.x) as f32;
                let dy_px = (start_pt.y - pt.y) as f32;

                if shift_down && alt_down {
                    *LIVE_MOUSE_DELTA.get_or_init(|| RwLock::new([0.0, 0.0, 0.0])).write().unwrap() = [0.0, 0.0, -dy_px * scale];
                } else {
                    *LIVE_MOUSE_DELTA.get_or_init(|| RwLock::new([0.0, 0.0, 0.0])).write().unwrap() = [dx_px * scale, dy_px * scale, 0.0];
                }
            }

            prev_drag_down = is_drag_held;

            if OFFSETS_DIRTY.load(Ordering::Relaxed) && last_save_time.elapsed() > Duration::from_millis(600) {
                save_offsets();
                last_save_time = Instant::now();
            }
        }
    });
}
