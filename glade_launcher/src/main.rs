#![windows_subsystem = "windows"]

use std::ffi::{c_void, OsStr};
use std::fs;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use eframe::egui::{self, Color32, CornerRadius, DragValue, FontId, Pos2, RichText, Shape, Slider, Stroke, Vec2};

const EMBEDDED_DLL: &[u8] = include_bytes!("../../bin/glade_loader.dll");

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    Checking,
    UpToDate { version: String },
    UpdateRequired { local_version: String, remote_version: String, release_url: String, release_name: String },
    Offline,
}

fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let clean = v.trim().trim_start_matches('v').trim_start_matches('V');
    let mut parts = clean.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch_str = parts.next().unwrap_or("0");
    let patch: u32 = patch_str.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn is_outdated(local: &str, remote: &str) -> bool {
    if let (Some(l), Some(r)) = (parse_version(local), parse_version(remote)) {
        r > l
    } else {
        false
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct GladeConfig {
    pub infinite_glade: bool,
    pub uncapped_camera: bool,
    pub clutter_animations: bool,
    pub clean_borders: bool,
    pub free_clutter_gizmo: bool,
    #[serde(default)]
    pub tutorial_seen: bool,
}

impl Default for GladeConfig {
    fn default() -> Self {
        Self {
            infinite_glade: true,
            uncapped_camera: true,
            clutter_animations: false,
            clean_borders: false,
            free_clutter_gizmo: false,
            tutorial_seen: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppTab {
    ModManager,
    AnimationStudio,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModCategory {
    Clutter,
    AnimatedClutter,
    Replacer,
    Plugin,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModFilter {
    All,
    Clutter,
    Replacers,
    Plugins,
}

#[derive(Clone, Debug)]
pub struct ModEntry {
    pub folder_name: String,
    pub full_path: PathBuf,
    pub is_enabled: bool,
    pub category: ModCategory,
    pub items_count: usize,
    pub replaced_targets: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SimpleMesh {
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub center: [f32; 3],
    pub radius: f32,
}

impl SimpleMesh {
    pub fn demo_windmill() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        // Central hub (octagonal prism)
        let hub_r = 0.25_f32;
        let hub_d = 0.15_f32;
        let hub_center = [0.0_f32, 3.3_f32, 1.35_f32];

        let n = 8;
        let start_v = vertices.len() as u32;
        for i in 0..n {
            let angle = (i as f32 / n as f32) * std::f32::consts::TAU;
            let x = angle.cos() * hub_r;
            let y = angle.sin() * hub_r;
            vertices.push([hub_center[0] + x, hub_center[1] + y, hub_center[2] - hub_d * 0.5]);
            vertices.push([hub_center[0] + x, hub_center[1] + y, hub_center[2] + hub_d * 0.5]);
        }
        for i in 0..n {
            let next = (i + 1) % n;
            let v0 = start_v + i * 2;
            let v1 = start_v + i * 2 + 1;
            let v2 = start_v + next * 2;
            let v3 = start_v + next * 2 + 1;
            indices.extend_from_slice(&[v0, v1, v2, v1, v3, v2]);
        }

        // 4 blades
        let blade_w = 0.22_f32;
        let blade_len = 2.1_f32;
        let blade_t = 0.04_f32;

        for b in 0..4 {
            let b_angle = (b as f32) * std::f32::consts::FRAC_PI_2;
            let cos_b = b_angle.cos();
            let sin_b = b_angle.sin();

            let base_idx = vertices.len() as u32;
            // 8 corners of the blade box
            let corners = [
                [-blade_w * 0.5, 0.2, -blade_t * 0.5],
                [blade_w * 0.5, 0.2, -blade_t * 0.5],
                [blade_w * 0.5, blade_len, -blade_t * 0.5],
                [-blade_w * 0.5, blade_len, -blade_t * 0.5],
                [-blade_w * 0.5, 0.2, blade_t * 0.5],
                [blade_w * 0.5, 0.2, blade_t * 0.5],
                [blade_w * 0.5, blade_len, blade_t * 0.5],
                [-blade_w * 0.5, blade_len, blade_t * 0.5],
            ];

            for c in corners {
                let rx = c[0] * cos_b - c[1] * sin_b;
                let ry = c[0] * sin_b + c[1] * cos_b;
                vertices.push([hub_center[0] + rx, hub_center[1] + ry, hub_center[2] + c[2]]);
            }

            // 6 faces of blade box
            let box_faces = [
                [0, 1, 2, 0, 2, 3], // Front
                [5, 4, 7, 5, 7, 6], // Back
                [4, 0, 3, 4, 3, 7], // Left
                [1, 5, 6, 1, 6, 2], // Right
                [3, 2, 6, 3, 6, 7], // Top
                [4, 5, 1, 4, 1, 0], // Bottom
            ];

            for f in box_faces {
                for &idx in &f {
                    indices.push(base_idx + idx);
                }
            }
        }

        Self {
            vertices,
            indices,
            center: hub_center,
            radius: 2.5,
        }
    }

    pub fn from_glb_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
            return None;
        }
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
        if bytes.len() < 20 + json_len + 8 {
            return None;
        }
        let json_str = std::str::from_utf8(&bytes[20..20 + json_len]).ok()?;
        let gltf: serde_json::Value = serde_json::from_str(json_str).ok()?;

        let bin_chunk_offset = 20 + json_len;
        let bin_len = u32::from_le_bytes(bytes[bin_chunk_offset..bin_chunk_offset + 4].try_into().ok()?) as usize;
        if bytes.len() < bin_chunk_offset + 8 + bin_len {
            return None;
        }
        let bin_data = &bytes[bin_chunk_offset + 8..bin_chunk_offset + 8 + bin_len];

        let meshes = gltf.get("meshes")?.as_array()?;
        let prim = meshes.first()?.get("primitives")?.as_array()?.first()?;

        let pos_acc_idx = prim.get("attributes")?.get("POSITION")?.as_u64()? as usize;
        let idx_acc_idx = prim.get("indices")?.as_u64()? as usize;

        let accessors = gltf.get("accessors")?.as_array()?;
        let buffer_views = gltf.get("bufferViews")?.as_array()?;

        let pos_acc = &accessors[pos_acc_idx];
        let pos_count = pos_acc.get("count")?.as_u64()? as usize;
        let pos_bv_idx = pos_acc.get("bufferView")?.as_u64()? as usize;
        let pos_bv = &buffer_views[pos_bv_idx];
        let pos_offset = (pos_bv.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0)
            + pos_acc.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0)) as usize;

        let mut vertices = Vec::with_capacity(pos_count);
        for i in 0..pos_count {
            let off = pos_offset + i * 12;
            if off + 12 <= bin_data.len() {
                let x = f32::from_le_bytes(bin_data[off..off + 4].try_into().ok()?);
                let y = f32::from_le_bytes(bin_data[off + 4..off + 8].try_into().ok()?);
                let z = f32::from_le_bytes(bin_data[off + 8..off + 12].try_into().ok()?);
                vertices.push([x, y, z]);
            }
        }

        let idx_acc = &accessors[idx_acc_idx];
        let idx_count = idx_acc.get("count")?.as_u64()? as usize;
        let idx_bv_idx = idx_acc.get("bufferView")?.as_u64()? as usize;
        let idx_bv = &buffer_views[idx_bv_idx];
        let idx_offset = (idx_bv.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0)
            + idx_acc.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0)) as usize;
        let ctype = idx_acc.get("componentType")?.as_u64()?;

        let mut indices = Vec::with_capacity(idx_count);
        if ctype == 5123 {
            for i in 0..idx_count {
                let off = idx_offset + i * 2;
                if off + 2 <= bin_data.len() {
                    let idx = u16::from_le_bytes(bin_data[off..off + 2].try_into().ok()?);
                    indices.push(idx as u32);
                }
            }
        } else if ctype == 5125 {
            for i in 0..idx_count {
                let off = idx_offset + i * 4;
                if off + 4 <= bin_data.len() {
                    let idx = u32::from_le_bytes(bin_data[off..off + 4].try_into().ok()?);
                    indices.push(idx);
                }
            }
        }

        if vertices.is_empty() || indices.len() < 3 {
            return None;
        }

        let mut min = vertices[0];
        let mut max = vertices[0];
        for v in &vertices {
            for j in 0..3 {
                if v[j] < min[j] { min[j] = v[j]; }
                if v[j] > max[j] { max[j] = v[j]; }
            }
        }
        let center = [
            (min[0] + max[0]) * 0.5,
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        ];
        let radius = ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2)).sqrt() * 0.5;

        Some(Self {
            vertices,
            indices,
            center,
            radius: radius.max(0.1),
        })
    }
}

pub struct Preview3D {
    pub cam_yaw: f32,
    pub cam_pitch: f32,
    pub start_time: Instant,
    pub cached_mesh: Option<SimpleMesh>,
    pub loaded_path: Option<PathBuf>,
}

impl Default for Preview3D {
    fn default() -> Self {
        Self {
            cam_yaw: 0.35,
            cam_pitch: 0.25,
            start_time: Instant::now(),
            cached_mesh: None,
            loaded_path: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AnimationStudioState {
    pub mod_name: String,
    pub prop_file_name: String,
    pub selected_glb_path: Option<PathBuf>,
    pub axis: String,
    pub speed_deg: f32,
    pub pivot_x: f32,
    pub pivot_y: f32,
    pub pivot_z: f32,
    pub generate_manifest: bool,
    pub studio_status: String,
}

impl Default for AnimationStudioState {
    fn default() -> Self {
        Self {
            mod_name: "WindmillDemo".to_string(),
            prop_file_name: "windmill_blades.glb".to_string(),
            selected_glb_path: None,
            axis: "Z".to_string(),
            speed_deg: 40.0,
            pivot_x: 0.0,
            pivot_y: 3.3,
            pivot_z: 1.35,
            generate_manifest: true,
            studio_status: "Draft loaded: Windmill Blades template ready".to_string(),
        }
    }
}

pub struct GladeApp {
    game_dir: Option<PathBuf>,
    game_exe: Option<PathBuf>,
    mods_dir: Option<PathBuf>,
    mods_disabled_dir: Option<PathBuf>,
    config: GladeConfig,
    config_path: Option<PathBuf>,
    installed_mods: Vec<ModEntry>,
    selected_filter: ModFilter,
    status_text: String,
    update_status: UpdateStatus,
    update_rx: Option<std::sync::mpsc::Receiver<UpdateStatus>>,
    current_tab: AppTab,
    studio: AnimationStudioState,
    preview: Preview3D,
    show_tutorial: bool,
}

impl GladeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (game_dir, game_exe) = Self::find_game();
        let (mods_dir, mods_disabled_dir) = Self::find_mods_dir();
        let (config, config_path) = Self::load_config(&game_dir);
        let tutorial_seen = config.tutorial_seen;

        let mut app = Self {
            game_dir,
            game_exe,
            mods_dir,
            mods_disabled_dir,
            config,
            config_path,
            installed_mods: Vec::new(),
            selected_filter: ModFilter::All,
            status_text: "Ready to launch".to_string(),
            update_status: UpdateStatus::Checking,
            update_rx: None,
            current_tab: AppTab::ModManager,
            studio: AnimationStudioState::default(),
            preview: Preview3D::default(),
            show_tutorial: !tutorial_seen,
        };

        app.trigger_update_check();
        app.refresh_mods();
        app.ensure_loader_installed();
        app
    }

    pub fn trigger_update_check(&mut self) {
        self.update_status = UpdateStatus::Checking;
        let (tx, rx) = std::sync::mpsc::channel();
        let local_ver = env!("CARGO_PKG_VERSION").to_string();

        std::thread::spawn(move || {
            let output = Command::new("curl.exe")
                .args(["-s", "-m", "4", "-H", "User-Agent: GladeApp", "https://api.github.com/repos/romrem30-eng/InfiniteGlade/releases/latest"])
                .creation_flags(0x08000000)
                .output();

            if let Ok(out) = output {
                if let Ok(txt) = String::from_utf8(out.stdout) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&txt) {
                        if let Some(tag) = json.get("tag_name").and_then(|t| t.as_str()) {
                            let remote_ver = tag.to_string();
                            let url = json.get("html_url")
                                .and_then(|u| u.as_str())
                                .unwrap_or("https://github.com/romrem30-eng/InfiniteGlade/releases")
                                .to_string();
                            let name = json.get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or(&remote_ver)
                                .to_string();

                            if is_outdated(&local_ver, &remote_ver) {
                                let _ = tx.send(UpdateStatus::UpdateRequired {
                                    local_version: local_ver,
                                    remote_version: remote_ver,
                                    release_url: url,
                                    release_name: name,
                                });
                                return;
                            } else {
                                let _ = tx.send(UpdateStatus::UpToDate { version: remote_ver });
                                return;
                            }
                        }
                    }
                }
            }
            let _ = tx.send(UpdateStatus::Offline);
        });

        self.update_rx = Some(rx);
    }

    fn find_game() -> (Option<PathBuf>, Option<PathBuf>) {
        let default_path = PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Tiny Glade\tiny-glade.exe");
        if default_path.is_file() {
            let dir = default_path.parent().unwrap().to_path_buf();
            return (Some(dir), Some(default_path));
        }

        if let Ok(cur) = std::env::current_dir() {
            let exe = cur.join("tiny-glade.exe");
            if exe.is_file() {
                return (Some(cur), Some(exe));
            }
        }

        (None, None)
    }

    fn find_mods_dir() -> (Option<PathBuf>, Option<PathBuf>) {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            let steam_saved = PathBuf::from(userprofile)
                .join("Saved Games")
                .join("Tiny Glade")
                .join("Steam");

            if let Ok(entries) = fs::read_dir(&steam_saved) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    let mods = p.join("mods");
                    if mods.is_dir() {
                        let disabled = p.join("mods_disabled");
                        let _ = fs::create_dir_all(&disabled);
                        return (Some(mods), Some(disabled));
                    }
                }
            }
        }
        (None, None)
    }

    fn load_config(game_dir: &Option<PathBuf>) -> (GladeConfig, Option<PathBuf>) {
        let mut target_path = None;
        if let Some(gdir) = game_dir {
            let p = gdir.join("glade_config.json");
            if p.is_file() {
                target_path = Some(p);
            }
        }

        if target_path.is_none() {
            if let Ok(userprofile) = std::env::var("USERPROFILE") {
                let p = PathBuf::from(userprofile)
                    .join("Saved Games")
                    .join("Tiny Glade")
                    .join("glade_config.json");
                if p.is_file() {
                    target_path = Some(p);
                }
            }
        }

        if let Some(ref p) = target_path {
            if let Ok(text) = fs::read_to_string(p) {
                if let Ok(cfg) = serde_json::from_str::<GladeConfig>(&text) {
                    return (cfg, target_path);
                }
            }
        }

        let default_cfg = GladeConfig::default();
        let fallback_path = if let Ok(userprofile) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(userprofile)
                .join("Saved Games")
                .join("Tiny Glade")
                .join("glade_config.json");
            let _ = fs::write(&p, serde_json::to_string_pretty(&default_cfg).unwrap_or_default());
            Some(p)
        } else {
            None
        };

        (default_cfg, fallback_path)
    }

    fn save_config(&self) {
        if let Some(ref p) = self.config_path {
            if let Ok(json) = serde_json::to_string_pretty(&self.config) {
                let _ = fs::write(p, json);
            }
        }
    }

    fn ensure_loader_installed(&mut self) {
        if let Some(ref gdir) = self.game_dir {
            let dll_dest = gdir.join("glade_loader.dll");
            let needs_write = match fs::read(&dll_dest) {
                Ok(existing) => existing != EMBEDDED_DLL,
                Err(_) => true,
            };
            if needs_write {
                if fs::write(&dll_dest, EMBEDDED_DLL).is_ok() {
                    self.status_text = "GladeLoader DLL installed / updated".to_string();
                }
            }
        }
    }

    fn inspect_mod_folder(path: &Path, is_enabled: bool) -> Option<ModEntry> {
        if !path.is_dir() { return None; }
        let raw_name = path.file_name()?.to_string_lossy().to_string();
        let clean_name = raw_name.trim_end_matches(".disabled").to_string();

        let mut category = ModCategory::Other;
        let mut items_count = 0;
        let mut replaced_targets = Vec::new();

        let mut has_dll = false;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("dll") {
                    has_dll = true;
                    break;
                }
            }
        }

        if has_dll {
            category = ModCategory::Plugin;
        } else {
            let clutter_json = path.join("clutter").join("clutter.json");
            let root_clutter = path.join("clutter.json");
            let cjson = if clutter_json.is_file() { Some(clutter_json) } else if root_clutter.is_file() { Some(root_clutter) } else { None };

            if let Some(ref c) = cjson {
                if let Ok(txt) = fs::read_to_string(c) {
                    items_count = txt.matches("\"mesh\"").count();
                }

                let mut has_animations = false;
                if path.join("animations.json").is_file() || path.join("clutter").join("animations.json").is_file() {
                    has_animations = true;
                } else if let Ok(sub) = fs::read_dir(path.join("clutter")) {
                    for f in sub.flatten() {
                        let fnm = f.file_name().to_string_lossy().to_string();
                        if fnm.contains("__spin_") {
                            has_animations = true;
                            break;
                        }
                    }
                }
                category = if has_animations { ModCategory::AnimatedClutter } else { ModCategory::Clutter };
            } else {
                fn collect_targets(dir: &Path, rel: &str, out: &mut Vec<String>) {
                    if let Ok(entries) = fs::read_dir(dir) {
                        for e in entries.flatten() {
                            let p = e.path();
                            let fname = e.file_name().to_string_lossy().to_string();
                            if p.is_dir() {
                                let next_rel = if rel.is_empty() { fname } else { format!("{}/{}", rel, fname) };
                                collect_targets(&p, &next_rel, out);
                            } else {
                                let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
                                let is_data = ext == "json" || ext == "png" || ext == "ogg" || ext == "dds" || ext == "obj";
                                let is_meta = fname.eq_ignore_ascii_case("manifest.json") 
                                    || fname.eq_ignore_ascii_case("package.json")
                                    || fname.to_lowercase().starts_with("readme")
                                    || fname.to_lowercase().starts_with("license");
                                if is_data && !is_meta {
                                    let target = if rel.is_empty() { fname } else { format!("{}/{}", rel, fname) };
                                    out.push(target);
                                }
                            }
                        }
                    }
                }

                let assets_dir = path.join("assets");
                if assets_dir.is_dir() {
                    collect_targets(&assets_dir, "assets", &mut replaced_targets);
                } else {
                    collect_targets(path, "", &mut replaced_targets);
                }

                if !replaced_targets.is_empty() {
                    category = ModCategory::Replacer;
                }
            }
        }

        Some(ModEntry {
            folder_name: clean_name,
            full_path: path.to_path_buf(),
            is_enabled,
            category,
            items_count,
            replaced_targets,
        })
    }

    fn refresh_mods(&mut self) {
        self.installed_mods.clear();
        let (Some(mdir), Some(ddir)) = (&self.mods_dir, &self.mods_disabled_dir) else { return };

        if let Ok(entries) = fs::read_dir(mdir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if name.ends_with(".disabled") {
                        let clean = name.trim_end_matches(".disabled");
                        let target = ddir.join(clean);
                        let _ = fs::rename(&path, &target);
                    }
                }
            }
        }

        if let Ok(entries) = fs::read_dir(mdir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(mod_entry) = Self::inspect_mod_folder(&path, true) {
                    self.installed_mods.push(mod_entry);
                }
            }
        }

        if let Ok(entries) = fs::read_dir(ddir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(mod_entry) = Self::inspect_mod_folder(&path, false) {
                    self.installed_mods.push(mod_entry);
                }
            }
        }

        self.installed_mods.sort_by(|a, b| a.folder_name.to_lowercase().cmp(&b.folder_name.to_lowercase()));
    }

    fn toggle_mod(&mut self, idx: usize) {
        if idx >= self.installed_mods.len() { return; }
        let (Some(mdir), Some(ddir)) = (&self.mods_dir, &self.mods_disabled_dir) else { return };

        let entry = &mut self.installed_mods[idx];
        let (src, dst) = if entry.is_enabled {
            (entry.full_path.clone(), ddir.join(&entry.folder_name))
        } else {
            (entry.full_path.clone(), mdir.join(&entry.folder_name))
        };

        if fs::rename(&src, &dst).is_ok() {
            entry.full_path = dst;
            entry.is_enabled = !entry.is_enabled;
            self.status_text = format!("Mod '{}' {}", entry.folder_name, if entry.is_enabled { "enabled" } else { "disabled" });
        } else {
            self.status_text = format!("Failed to toggle mod '{}'", entry.folder_name);
        }
    }

    fn install_mod_zip(&mut self, zip_path: &Path) {
        let Some(ref mdir) = self.mods_dir else { return };
        let Ok(file) = fs::File::open(zip_path) else { return };
        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(_) => return,
        };

        let mod_stem = zip_path.file_stem().unwrap_or_default().to_string_lossy();
        let dest_dir = mdir.join(mod_stem.as_ref());
        let _ = fs::create_dir_all(&dest_dir);

        for i in 0..archive.len() {
            if let Ok(mut item) = archive.by_index(i) {
                let outpath = match item.enclosed_name() {
                    Some(path) => dest_dir.join(path),
                    None => continue,
                };
                if item.is_dir() {
                    let _ = fs::create_dir_all(&outpath);
                } else {
                    if let Some(p) = outpath.parent() {
                        let _ = fs::create_dir_all(p);
                    }
                    let mut outfile = match fs::File::create(&outpath) {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    let _ = std::io::copy(&mut item, &mut outfile);
                }
            }
        }
        self.refresh_mods();
        self.status_text = format!("Installed mod: {}", mod_stem);
    }

    fn get_active_mesh(&mut self) -> SimpleMesh {
        if let Some(ref path) = self.studio.selected_glb_path {
            if self.preview.loaded_path.as_ref() != Some(path) {
                if let Ok(bytes) = fs::read(path) {
                    if let Some(mesh) = SimpleMesh::from_glb_bytes(&bytes) {
                        self.preview.cached_mesh = Some(mesh);
                        self.preview.loaded_path = Some(path.clone());
                    }
                }
            }
        } else {
            // Load demo windmill if not loaded yet
            if self.preview.cached_mesh.is_none() {
                if let Some(ref mdir) = self.mods_dir {
                    let p = mdir.join("WindmillPack").join("clutter").join("windmill_blades__spin_z_40.glb");
                    if let Ok(bytes) = fs::read(&p) {
                        if let Some(mesh) = SimpleMesh::from_glb_bytes(&bytes) {
                            self.preview.cached_mesh = Some(mesh);
                        }
                    }
                }
                if self.preview.cached_mesh.is_none() {
                    self.preview.cached_mesh = Some(SimpleMesh::demo_windmill());
                }
            }
        }

        self.preview.cached_mesh.clone().unwrap_or_else(SimpleMesh::demo_windmill)
    }

    fn get_demo_glb_bytes(&self) -> Vec<u8> {
        if let Some(ref mdir) = self.mods_dir {
            let windmill_path = mdir.join("WindmillPack").join("clutter").join("windmill_blades__spin_z_40.glb");
            if let Ok(bytes) = fs::read(&windmill_path) {
                return bytes;
            }
        }
        let json_chunk = br#"{"asset":{"version":"2.0"},"scenes":[{"nodes":[0]}],"nodes":[{"name":"Blades"}],"scene":0}"#;
        let json_len = ((json_chunk.len() + 3) / 4) * 4;
        let mut glb = Vec::new();
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        let total_len = 12 + 8 + json_len as u32;
        glb.extend_from_slice(&total_len.to_le_bytes());
        glb.extend_from_slice(&(json_len as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(json_chunk);
        while glb.len() < (12 + 8 + json_len) {
            glb.push(b' ');
        }
        glb
    }

    fn install_studio_prop(&mut self) {
        let Some(ref mdir) = self.mods_dir else {
            self.studio.studio_status = "Error: Mods folder not found".to_string();
            return;
        };

        let clean_mod_name = self.studio.mod_name.trim().to_string();
        if clean_mod_name.is_empty() {
            self.studio.studio_status = "Error: Mod name cannot be empty".to_string();
            return;
        }

        let mut prop_fname = self.studio.prop_file_name.trim().to_string();
        if !prop_fname.ends_with(".glb") {
            prop_fname.push_str(".glb");
        }

        let target_mod_dir = mdir.join(&clean_mod_name);
        let target_clutter_dir = target_mod_dir.join("clutter");
        let _ = fs::create_dir_all(&target_clutter_dir);

        let dest_glb = target_clutter_dir.join(&prop_fname);
        let glb_bytes = if let Some(ref src) = self.studio.selected_glb_path {
            fs::read(src).unwrap_or_else(|_| self.get_demo_glb_bytes())
        } else {
            self.get_demo_glb_bytes()
        };
        let _ = fs::write(&dest_glb, glb_bytes);

        // 1. market.json
        let market_json = format!(
            "{{\n  \"name\": \"{}\",\n  \"version\": \"1.0.0\",\n  \"description\": \"Animated clutter created with GladeApp Animation Studio.\",\n  \"author\": \"GladeApp Creator\"\n}}\n",
            clean_mod_name
        );
        let _ = fs::write(target_mod_dir.join("market.json"), &market_json);

        // 2. mod-id.json
        let mod_id_path = target_mod_dir.join("mod-id.json");
        if !mod_id_path.exists() {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
            let mod_id_json = format!("{{\n  \"mod_id\": \"cb72f9e0-{:016x}\"\n}}\n", nanos);
            let _ = fs::write(&mod_id_path, &mod_id_json);
        }

        // 3. animations.json (with multi-key mapping so game loader always matches)
        let stem = prop_fname.strip_suffix(".glb").unwrap_or(&prop_fname);
        let anim_json = format!(
            "{{\n  \"clutter/{}\": {{\n    \"axis\": \"{}\",\n    \"speed\": {:.1},\n    \"pivot\": [{:.2}, {:.2}, {:.2}]\n  }},\n  \"{}\": {{\n    \"axis\": \"{}\",\n    \"speed\": {:.1},\n    \"pivot\": [{:.2}, {:.2}, {:.2}]\n  }},\n  \"{}\": {{\n    \"axis\": \"{}\",\n    \"speed\": {:.1},\n    \"pivot\": [{:.2}, {:.2}, {:.2}]\n  }}\n}}\n",
            prop_fname, self.studio.axis, self.studio.speed_deg, self.studio.pivot_x, self.studio.pivot_y, self.studio.pivot_z,
            prop_fname, self.studio.axis, self.studio.speed_deg, self.studio.pivot_x, self.studio.pivot_y, self.studio.pivot_z,
            stem, self.studio.axis, self.studio.speed_deg, self.studio.pivot_x, self.studio.pivot_y, self.studio.pivot_z
        );
        let _ = fs::write(target_clutter_dir.join("animations.json"), &anim_json);
        let _ = fs::write(target_mod_dir.join("animations.json"), &anim_json);

        // 4. clutter.json (with format: 1 and category Furniture)
        if self.studio.generate_manifest {
            let clutter_json = format!(
                "{{\n  \"format\": 1,\n  \"items\": [\n    {{\n      \"mesh\": \"{}\",\n      \"category\": \"Furniture\",\n      \"sound\": \"Bench\"\n    }}\n  ]\n}}\n",
                prop_fname
            );
            let _ = fs::write(target_clutter_dir.join("clutter.json"), &clutter_json);
        }

        self.refresh_mods();
        self.studio.studio_status = format!("Installed '{}' to game successfully!", clean_mod_name);
        self.status_text = format!("Installed animated mod: {}", clean_mod_name);
    }

    fn export_studio_zip(&mut self) {
        let clean_mod_name = self.studio.mod_name.trim().to_string();
        if clean_mod_name.is_empty() {
            self.studio.studio_status = "Error: Mod name cannot be empty".to_string();
            return;
        }

        let default_zip_name = format!("{}.zip", clean_mod_name);
        let Some(zip_dest) = rfd::FileDialog::new()
            .set_file_name(&default_zip_name)
            .add_filter("Zip Archive", &["zip"])
            .save_file()
        else {
            return;
        };

        let mut prop_fname = self.studio.prop_file_name.trim().to_string();
        if !prop_fname.ends_with(".glb") {
            prop_fname.push_str(".glb");
        }

        let glb_bytes = if let Some(ref src) = self.studio.selected_glb_path {
            fs::read(src).unwrap_or_else(|_| self.get_demo_glb_bytes())
        } else {
            self.get_demo_glb_bytes()
        };

        let stem = prop_fname.strip_suffix(".glb").unwrap_or(&prop_fname);
        let anim_json = format!(
            "{{\n  \"clutter/{}\": {{\n    \"axis\": \"{}\",\n    \"speed\": {:.1},\n    \"pivot\": [{:.2}, {:.2}, {:.2}]\n  }},\n  \"{}\": {{\n    \"axis\": \"{}\",\n    \"speed\": {:.1},\n    \"pivot\": [{:.2}, {:.2}, {:.2}]\n  }},\n  \"{}\": {{\n    \"axis\": \"{}\",\n    \"speed\": {:.1},\n    \"pivot\": [{:.2}, {:.2}, {:.2}]\n  }}\n}}\n",
            prop_fname, self.studio.axis, self.studio.speed_deg, self.studio.pivot_x, self.studio.pivot_y, self.studio.pivot_z,
            prop_fname, self.studio.axis, self.studio.speed_deg, self.studio.pivot_x, self.studio.pivot_y, self.studio.pivot_z,
            stem, self.studio.axis, self.studio.speed_deg, self.studio.pivot_x, self.studio.pivot_y, self.studio.pivot_z
        );

        let clutter_json = format!(
            "{{\n  \"format\": 1,\n  \"items\": [\n    {{\n      \"mesh\": \"{}\",\n      \"category\": \"Furniture\",\n      \"sound\": \"Bench\"\n    }}\n  ]\n}}\n",
            prop_fname
        );

        let market_json = format!(
            "{{\n  \"name\": \"{}\",\n  \"version\": \"1.0.0\",\n  \"description\": \"Animated clutter created with GladeApp Animation Studio.\",\n  \"author\": \"GladeApp Creator\"\n}}\n",
            clean_mod_name
        );

        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
        let mod_id_json = format!("{{\n  \"mod_id\": \"cb72f9e0-{:016x}\"\n}}\n", nanos);

        let file = match fs::File::create(&zip_dest) {
            Ok(f) => f,
            Err(e) => {
                self.studio.studio_status = format!("Failed to create zip: {:?}", e);
                return;
            }
        };

        let mut zip_writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let _ = zip_writer.start_file(format!("clutter/{}", prop_fname), options);
        let _ = zip_writer.write_all(&glb_bytes);

        let _ = zip_writer.start_file("market.json", options);
        let _ = zip_writer.write_all(market_json.as_bytes());

        let _ = zip_writer.start_file("mod-id.json", options);
        let _ = zip_writer.write_all(mod_id_json.as_bytes());

        let _ = zip_writer.start_file("clutter/animations.json", options);
        let _ = zip_writer.write_all(anim_json.as_bytes());

        let _ = zip_writer.start_file("animations.json", options);
        let _ = zip_writer.write_all(anim_json.as_bytes());

        if self.studio.generate_manifest {
            let _ = zip_writer.start_file("clutter/clutter.json", options);
            let _ = zip_writer.write_all(clutter_json.as_bytes());
        }

        let _ = zip_writer.finish();
        self.studio.studio_status = format!("Exported package to {:?}", zip_dest.file_name().unwrap_or_default());
    }

    fn launch_game(&mut self) {
        self.save_config();
        self.ensure_loader_installed();

        let Some(ref gexe) = self.game_exe else {
            self.status_text = "Error: tiny-glade.exe not found".to_string();
            return;
        };

        let gdir = gexe.parent().unwrap();
        let dll_path = gdir.join("glade_loader.dll");

        if !dll_path.is_file() {
            self.status_text = "Error: glade_loader.dll missing".to_string();
            return;
        }

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
            fn VirtualAllocEx(h: *mut c_void, a: *const c_void, s: usize, at: u32, p: u32) -> *mut c_void;
            fn WriteProcessMemory(h: *mut c_void, b: *mut c_void, buf: *const c_void, s: usize, w: *mut usize) -> i32;
            fn CreateRemoteThread(h: *mut c_void, a: *const c_void, st: usize, sp: unsafe extern "system" fn(*mut c_void) -> u32, p: *mut c_void, f: u32, id: *mut u32) -> *mut c_void;
            fn ResumeThread(h: *mut c_void) -> u32;
            fn WaitForSingleObject(h: *mut c_void, ms: u32) -> u32;
            fn CloseHandle(h: *mut c_void) -> i32;
            fn GetModuleHandleA(m: *const u8) -> *mut c_void;
            fn GetProcAddress(m: *mut c_void, p: *const u8) -> usize;
        }

        fn to_wide(s: &str) -> Vec<u16> {
            OsStr::new(s).encode_wide().chain(Some(0)).collect()
        }

        unsafe {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            si.dw_flags = 0x00000001;
            si.w_show_window = 1;

            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let app_name_w = to_wide(&gexe.to_string_lossy());
            let dir_w = to_wide(&gdir.to_string_lossy());

            let success = CreateProcessW(
                app_name_w.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                0x00000004,
                std::ptr::null(),
                dir_w.as_ptr(),
                &si,
                &mut pi,
            );

            if success == 0 {
                self.status_text = "Failed to spawn tiny-glade.exe".to_string();
                return;
            }

            let dll_str = dll_path.to_string_lossy().to_string();
            let dll_w = to_wide(&dll_str);
            let bytes_len = dll_w.len() * 2;

            let mem = VirtualAllocEx(pi.h_process, std::ptr::null(), bytes_len, 0x1000 | 0x2000, 0x04);
            if !mem.is_null() {
                let mut written = 0;
                WriteProcessMemory(pi.h_process, mem, dll_w.as_ptr() as _, bytes_len, &mut written);
                let k32 = GetModuleHandleA(b"kernel32.dll\0".as_ptr());
                let load_lib = GetProcAddress(k32, b"LoadLibraryW\0".as_ptr());
                let thread = CreateRemoteThread(pi.h_process, std::ptr::null(), 0, std::mem::transmute(load_lib), mem, 0, std::ptr::null_mut());
                if !thread.is_null() {
                    WaitForSingleObject(thread, 3000);
                    CloseHandle(thread);
                }
            }

            ResumeThread(pi.h_thread);
            CloseHandle(pi.h_thread);
            CloseHandle(pi.h_process);
        }

        self.status_text = "Game launched with GladeLoader!".to_string();
    }

    fn render_3d_preview(&mut self, ui: &mut egui::Ui) {
        let size = Vec2::new(260.0, 250.0);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::drag());

        if response.dragged() {
            self.preview.cam_yaw += response.drag_delta().x * 0.015;
            self.preview.cam_pitch = (self.preview.cam_pitch + response.drag_delta().y * 0.015).clamp(-1.4, 1.4);
        }
        if response.double_clicked() {
            self.preview.cam_yaw = 0.35;
            self.preview.cam_pitch = 0.25;
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(16, 18, 22));
        painter.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, Color32::from_rgb(38, 44, 54)), egui::StrokeKind::Inside);

        let mesh = self.get_active_mesh();
        let elapsed = self.preview.start_time.elapsed().as_secs_f32();
        let rot_angle = elapsed * self.studio.speed_deg.to_radians();

        let cos_r = rot_angle.cos();
        let sin_r = rot_angle.sin();

        let pivot = [self.studio.pivot_x, self.studio.pivot_y, self.studio.pivot_z];
        let axis = self.studio.axis.as_str();

        let cos_yaw = self.preview.cam_yaw.cos();
        let sin_yaw = self.preview.cam_yaw.sin();
        let cos_pitch = self.preview.cam_pitch.cos();
        let sin_pitch = self.preview.cam_pitch.sin();

        let scale = (rect.width() * 0.38) / mesh.radius;
        let cx = rect.center().x;
        let cy = rect.center().y;

        // Transform and project vertex to screen pos and depth
        let transform_v = |v: [f32; 3]| -> (Pos2, [f32; 3]) {
            // 1. Subtract pivot
            let dx = v[0] - pivot[0];
            let dy = v[1] - pivot[1];
            let dz = v[2] - pivot[2];

            // 2. Rotate around chosen axis
            let (rx, ry, rz) = match axis {
                "X" => (dx, dy * cos_r - dz * sin_r, dy * sin_r + dz * cos_r),
                "Y" => (dx * cos_r + dz * sin_r, dy, -dx * sin_r + dz * cos_r),
                _ => (dx * cos_r - dy * sin_r, dx * sin_r + dy * cos_r, dz), // "Z" default
            };

            // 3. Add pivot back, center relative to mesh center
            let mx = rx + pivot[0] - mesh.center[0];
            let my = ry + pivot[1] - mesh.center[1];
            let mz = rz + pivot[2] - mesh.center[2];

            // 4. Camera rotation (Yaw around Y, Pitch around X)
            let cy_x = mx * cos_yaw + mz * sin_yaw;
            let cy_z = -mx * sin_yaw + mz * cos_yaw;

            let cp_y = my * cos_pitch - cy_z * sin_pitch;
            let cp_z = my * sin_pitch + cy_z * cos_pitch;

            let screen_pos = Pos2::new(cx + cy_x * scale, cy - cp_y * scale);
            (screen_pos, [cy_x, cp_y, cp_z])
        };

        // Light direction (top-right-front)
        let light = [0.55_f32, 0.75_f32, 0.35_f32];
        let l_len = (light[0] * light[0] + light[1] * light[1] + light[2] * light[2]).sqrt();
        let light = [light[0] / l_len, light[1] / l_len, light[2] / l_len];

        struct TriRender {
            pts: [Pos2; 3],
            avg_z: f32,
            color: Color32,
        }

        let mut tri_list = Vec::with_capacity(mesh.indices.len() / 3);

        for chunk in mesh.indices.chunks_exact(3) {
            let i0 = chunk[0] as usize;
            let i1 = chunk[1] as usize;
            let i2 = chunk[2] as usize;

            if i0 >= mesh.vertices.len() || i1 >= mesh.vertices.len() || i2 >= mesh.vertices.len() {
                continue;
            }

            let (p0, v0) = transform_v(mesh.vertices[i0]);
            let (p1, v1) = transform_v(mesh.vertices[i1]);
            let (p2, v2) = transform_v(mesh.vertices[i2]);

            // Face normal in camera space
            let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
            let nx = e1[1] * e2[2] - e1[2] * e2[1];
            let ny = e1[2] * e2[0] - e1[0] * e2[2];
            let nz = e1[0] * e2[1] - e1[1] * e2[0];

            let n_len = (nx * nx + ny * ny + nz * nz).sqrt();
            if n_len < 0.0001 || nz <= 0.0 {
                continue; // Backface cull
            }

            let nx = nx / n_len;
            let ny = ny / n_len;
            let nz = nz / n_len;

            let diffuse = (nx * light[0] + ny * light[1] + nz * light[2]).max(0.25).min(1.0);
            let r = (210.0 * diffuse) as u8;
            let g = (180.0 * diffuse) as u8;
            let b = (140.0 * diffuse) as u8;
            let color = Color32::from_rgb(r, g, b);

            let avg_z = (v0[2] + v1[2] + v2[2]) / 3.0;
            tri_list.push(TriRender { pts: [p0, p1, p2], avg_z, color });
        }

        // Painter's algorithm: sort from farthest to nearest
        tri_list.sort_by(|a, b| a.avg_z.partial_cmp(&b.avg_z).unwrap_or(std::cmp::Ordering::Equal));

        for tri in tri_list {
            painter.add(Shape::convex_polygon(
                tri.pts.to_vec(),
                tri.color,
                Stroke::new(0.5, Color32::from_rgba_premultiplied(35, 38, 44, 70)),
            ));
        }

        // Draw pivot marker (red glowing sphere)
        let (pivot_pt, _) = transform_v(pivot);
        painter.circle_filled(pivot_pt, 4.5, Color32::from_rgb(255, 75, 75));
        painter.circle_stroke(pivot_pt, 7.0, Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 100, 100, 100)));

        // Axis line through pivot
        let axis_vec = match axis {
            "X" => [0.8, 0.0, 0.0],
            "Y" => [0.0, 0.8, 0.0],
            _ => [0.0, 0.0, 0.8],
        };
        let p_end = [pivot[0] + axis_vec[0], pivot[1] + axis_vec[1], pivot[2] + axis_vec[2]];
        let (axis_end_pt, _) = transform_v(p_end);
        let axis_color = match axis {
            "X" => Color32::from_rgb(240, 80, 80),
            "Y" => Color32::from_rgb(80, 220, 120),
            _ => Color32::from_rgb(80, 160, 240),
        };
        painter.line_segment([pivot_pt, axis_end_pt], Stroke::new(2.0, axis_color));

        // Overlays
        painter.text(
            Pos2::new(rect.left() + 8.0, rect.top() + 8.0),
            egui::Align2::LEFT_TOP,
            format!("Spin: {} ({:.0} deg/s)", axis, self.studio.speed_deg),
            FontId::proportional(11.0),
            Color32::from_rgb(212, 155, 85),
        );

        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - 10.0),
            egui::Align2::CENTER_BOTTOM,
            "Drag mouse to orbit 3D view",
            FontId::proportional(10.0),
            Color32::from_rgb(120, 128, 142),
        );

        // Continuous 60fps repaint for smooth animation
        ui.ctx().request_repaint();
    }
}

impl eframe::App for GladeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let bg_color = Color32::from_rgb(24, 26, 30);
        let text_main = Color32::from_rgb(238, 240, 243);
        let text_sub = Color32::from_rgb(148, 155, 168);
        let accent_green = Color32::from_rgb(72, 155, 94);
        let accent_amber = Color32::from_rgb(212, 155, 85);
        let accent_blue = Color32::from_rgb(70, 140, 230);

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = bg_color;
        visuals.window_fill = bg_color;
        visuals.override_text_color = Some(text_main);
        ui.ctx().set_visuals(visuals);

        ui.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                for file in &i.raw.dropped_files {
                    let path = file.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("zip") {
                        self.install_mod_zip(path);
                    } else if path.extension().and_then(|s| s.to_str()) == Some("glb") {
                        self.studio.selected_glb_path = Some(path.to_path_buf());
                        if let Some(stem) = path.file_stem() {
                            self.studio.mod_name = stem.to_string_lossy().to_string();
                            self.studio.prop_file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        }
                        self.current_tab = AppTab::AnimationStudio;
                    }
                }
            }
        });

        if let Some(ref rx) = self.update_rx {
            if let Ok(new_status) = rx.try_recv() {
                self.update_status = new_status;
                self.update_rx = None;
            }
        }

        ui.add_space(8.0);

        // 1. TOP HEADER BAR
        ui.horizontal(|ui| {
            let local_ver = env!("CARGO_PKG_VERSION");
            ui.label(RichText::new("GladeApp").font(FontId::proportional(22.0)).strong().color(accent_green));
            ui.label(RichText::new(format!("v{}", local_ver)).font(FontId::proportional(12.5)).color(text_sub));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let help_btn = egui::Button::new(RichText::new(" ? ").strong().font(FontId::proportional(12.0)))
                    .fill(Color32::from_rgb(38, 43, 52))
                    .corner_radius(CornerRadius::same(4));
                if ui.add(help_btn).on_hover_text("Open Quick Guide & Tutorial").clicked() {
                    self.show_tutorial = true;
                }

                ui.add_space(4.0);

                match &self.update_status {
                    UpdateStatus::UpdateRequired { release_url, remote_version, .. } => {
                        let btn = egui::Button::new(
                            RichText::new(format!("Update Now ({})", remote_version))
                                .font(FontId::proportional(11.5))
                                .strong()
                                .color(Color32::WHITE)
                        )
                        .fill(Color32::from_rgb(180, 50, 50))
                        .corner_radius(CornerRadius::same(4));

                        if ui.add(btn).clicked() {
                            let _ = open::that(release_url);
                        }
                    }
                    UpdateStatus::Checking => {
                        ui.label(RichText::new("Checking updates...").color(text_sub).font(FontId::proportional(11.5)));
                    }
                    _ => {
                        let btn = egui::Button::new(
                            RichText::new("Check for Updates")
                                .font(FontId::proportional(11.0))
                                .color(text_sub)
                        )
                        .fill(Color32::from_rgb(32, 36, 42))
                        .corner_radius(CornerRadius::same(4));

                        if ui.add(btn).clicked() {
                            self.trigger_update_check();
                        }
                    }
                }

                ui.add_space(8.0);
                if self.game_exe.is_some() {
                    ui.label(RichText::new("Tiny Glade: Ready").color(accent_green).font(FontId::proportional(11.5)));
                } else {
                    ui.label(RichText::new("Tiny Glade: Not detected").color(Color32::from_rgb(220, 90, 90)).font(FontId::proportional(11.5)));
                }
            });
        });

        ui.add_space(4.0);

        // 2. MAIN TABS SELECTOR
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            let mod_tab_active = self.current_tab == AppTab::ModManager;
            let mod_btn = egui::Button::new(
                RichText::new("Mod Manager")
                    .font(FontId::proportional(13.0))
                    .strong()
                    .color(if mod_tab_active { text_main } else { text_sub })
            )
            .fill(if mod_tab_active { Color32::from_rgb(42, 48, 58) } else { Color32::from_rgb(28, 31, 37) })
            .corner_radius(CornerRadius::same(6));
            if ui.add(mod_btn).clicked() {
                self.current_tab = AppTab::ModManager;
            }

            let studio_tab_active = self.current_tab == AppTab::AnimationStudio;
            let studio_btn = egui::Button::new(
                RichText::new("Animation Studio")
                    .font(FontId::proportional(13.0))
                    .strong()
                    .color(if studio_tab_active { text_main } else { text_sub })
            )
            .fill(if studio_tab_active { Color32::from_rgb(42, 48, 58) } else { Color32::from_rgb(28, 31, 37) })
            .corner_radius(CornerRadius::same(6));
            if ui.add(studio_btn).clicked() {
                self.current_tab = AppTab::AnimationStudio;
            }
        });

        ui.separator();
        ui.add_space(6.0);

        // 3. TAB CONTENT
        match self.current_tab {
            AppTab::ModManager => {
                ui.columns(2, |cols| {
                    cols[0].group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("CORE ENGINE").strong().font(FontId::proportional(13.5)).color(accent_amber));
                        });
                        ui.separator();
                        ui.add_space(4.0);

                        if ui.checkbox(&mut self.config.infinite_glade, RichText::new("Infinite Map").strong()).changed() {
                            self.save_config();
                        }
                        ui.label(RichText::new("Removes building borders, expand without limits").color(text_sub).font(FontId::proportional(11.5)));
                        ui.add_space(8.0);

                        if ui.checkbox(&mut self.config.uncapped_camera, RichText::new("Uncapped Camera").strong()).changed() {
                            self.save_config();
                        }
                        ui.label(RichText::new("Free pan & 150m zoom out (3.7x standard distance)").color(text_sub).font(FontId::proportional(11.5)));
                        ui.add_space(8.0);

                        if ui.checkbox(&mut self.config.clutter_animations, RichText::new("Clutter Animation Engine (WIP)").strong()).changed() {
                            self.save_config();
                        }
                        ui.label(RichText::new("Paused for stability; upgrade in progress").color(text_sub).font(FontId::proportional(11.5)));
                        ui.add_space(8.0);

                        if ui.checkbox(&mut self.config.clean_borders, RichText::new("Clean Glade Borders").strong()).changed() {
                            self.save_config();
                        }
                        ui.label(RichText::new("Removes perimeter boundary stones and debris").color(text_sub).font(FontId::proportional(11.5)));
                        ui.add_space(8.0);

                        if ui.checkbox(&mut self.config.free_clutter_gizmo, RichText::new("Free Clutter Gizmo (WIP)").strong()).changed() {
                            self.save_config();
                        }
                        ui.label(RichText::new("Paused for stability; upgrade in progress").color(text_sub).font(FontId::proportional(11.5)));
                        ui.add_space(4.0);
                    });

                    cols[1].group(|ui| {
                        ui.set_width(ui.available_width());

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("INSTALLED MODS").strong().font(FontId::proportional(13.5)).color(accent_amber));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Open Folder").on_hover_text("Open mods directory in Explorer").clicked() {
                                    if let Some(ref mdir) = self.mods_dir {
                                        let _ = Command::new("explorer").arg(mdir).spawn();
                                    }
                                }
                                if ui.button("Import .zip").on_hover_text("Install new mod from .zip archive").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().add_filter("Zip Archive", &["zip"]).pick_file() {
                                        self.install_mod_zip(&path);
                                    }
                                }
                            });
                        });

                        ui.separator();

                        let total_count = self.installed_mods.len();
                        let clutter_count = self.installed_mods.iter().filter(|m| matches!(m.category, ModCategory::Clutter | ModCategory::AnimatedClutter)).count();
                        let replacer_count = self.installed_mods.iter().filter(|m| m.category == ModCategory::Replacer).count();
                        let plugin_count = self.installed_mods.iter().filter(|m| m.category == ModCategory::Plugin).count();

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let filters = [
                                (ModFilter::All, format!("All ({})", total_count)),
                                (ModFilter::Clutter, format!("Clutter ({})", clutter_count)),
                                (ModFilter::Replacers, format!("Replacers ({})", replacer_count)),
                                (ModFilter::Plugins, format!("Plugins ({})", plugin_count)),
                            ];

                            for (filter, label) in filters {
                                let is_active = self.selected_filter == filter;
                                let btn = egui::Button::new(
                                    RichText::new(label)
                                        .font(FontId::proportional(11.0))
                                        .strong()
                                        .color(if is_active { text_main } else { text_sub })
                                )
                                .fill(if is_active { Color32::from_rgb(46, 54, 66) } else { Color32::from_rgb(30, 33, 40) })
                                .corner_radius(CornerRadius::same(4));

                                if ui.add(btn).clicked() {
                                    self.selected_filter = filter;
                                }
                            }
                        });

                        ui.add_space(4.0);

                        let visible_indices: Vec<usize> = self.installed_mods
                            .iter()
                            .enumerate()
                            .filter(|(_, m)| match self.selected_filter {
                                ModFilter::All => true,
                                ModFilter::Clutter => matches!(m.category, ModCategory::Clutter | ModCategory::AnimatedClutter),
                                ModFilter::Replacers => m.category == ModCategory::Replacer,
                                ModFilter::Plugins => m.category == ModCategory::Plugin,
                            })
                            .map(|(i, _)| i)
                            .collect();

                        egui::ScrollArea::vertical().max_height(190.0).show(ui, |ui| {
                            if visible_indices.is_empty() {
                                ui.add_space(20.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(RichText::new("No mods in this category").color(text_sub).italics());
                                    ui.label(RichText::new("Drop .zip files here to install").color(text_sub).font(FontId::proportional(11.0)));
                                });
                            } else {
                                let mut toggle_idx = None;
                                for &idx in &visible_indices {
                                    let m = &self.installed_mods[idx];

                                    ui.group(|ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            let mut enabled = m.is_enabled;
                                            if ui.checkbox(&mut enabled, "").clicked() {
                                                toggle_idx = Some(idx);
                                            }

                                            let display_name = m.folder_name.trim_end_matches(".disabled");
                                            let name_color = if m.is_enabled { text_main } else { text_sub };
                                            ui.label(RichText::new(display_name).strong().color(name_color));

                                            match m.category {
                                                ModCategory::AnimatedClutter => {
                                                    ui.label(RichText::new("ANIMATED").font(FontId::proportional(10.0)).strong().color(Color32::from_rgb(110, 195, 255)));
                                                    if m.items_count > 0 {
                                                        ui.label(RichText::new(format!("({} items)", m.items_count)).color(text_sub).font(FontId::proportional(10.5)));
                                                    }
                                                }
                                                ModCategory::Clutter => {
                                                    ui.label(RichText::new("CLUTTER").font(FontId::proportional(10.0)).strong().color(Color32::from_rgb(120, 210, 140)));
                                                    if m.items_count > 0 {
                                                        ui.label(RichText::new(format!("({} items)", m.items_count)).color(text_sub).font(FontId::proportional(10.5)));
                                                    }
                                                }
                                                ModCategory::Replacer => {
                                                    ui.label(RichText::new("REPLACER").font(FontId::proportional(10.0)).strong().color(Color32::from_rgb(235, 175, 95)));
                                                }
                                                ModCategory::Plugin => {
                                                    ui.label(RichText::new("PLUGIN").font(FontId::proportional(10.0)).strong().color(Color32::from_rgb(195, 140, 245)));
                                                }
                                                ModCategory::Other => {
                                                    ui.label(RichText::new("MOD").font(FontId::proportional(10.0)).color(text_sub));
                                                }
                                            }
                                        });
                                    });
                                }
                                if let Some(idx) = toggle_idx {
                                    self.toggle_mod(idx);
                                }
                            }
                        });
                    });
                });

                ui.add_space(14.0);

                ui.vertical_centered(|ui| {
                    let play_btn = egui::Button::new(
                        RichText::new("PLAY TINY GLADE")
                            .font(FontId::proportional(16.5))
                            .strong()
                            .color(Color32::WHITE)
                    )
                    .fill(accent_green)
                    .min_size(Vec2::new(320.0, 46.0))
                    .corner_radius(CornerRadius::same(8));

                    if ui.add(play_btn).clicked() {
                        self.launch_game();
                    }

                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.status_text).color(text_sub).font(FontId::proportional(12.0)));
                });
            }

            AppTab::AnimationStudio => {
                ui.columns(2, |cols| {
                    // LEFT COLUMN: CONFIGURATION
                    cols[0].group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("PROP CONFIGURATION").strong().font(FontId::proportional(13.5)).color(accent_amber));
                        ui.separator();
                        ui.add_space(4.0);

                        ui.label(RichText::new("Mod Folder Name:").font(FontId::proportional(12.0)));
                        ui.text_edit_singleline(&mut self.studio.mod_name);
                        ui.add_space(6.0);

                        ui.label(RichText::new("3D Model (.glb):").font(FontId::proportional(12.0)));
                        ui.horizontal(|ui| {
                            if ui.button("Browse Model...").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("GLTF 3D Model", &["glb"]).pick_file() {
                                    self.studio.prop_file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                                    self.studio.selected_glb_path = Some(path);
                                }
                            }
                            let label_text = if let Some(ref p) = self.studio.selected_glb_path {
                                p.file_name().unwrap_or_default().to_string_lossy().to_string()
                            } else {
                                "Demo Mesh: Windmill Blades".to_string()
                            };
                            ui.label(RichText::new(label_text).color(accent_green).font(FontId::proportional(11.5)));
                        });
                        ui.add_space(8.0);

                        // Presets
                        ui.label(RichText::new("Quick Presets:").font(FontId::proportional(12.0)).strong());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.spacing_mut().item_spacing.y = 4.0;

                            if ui.button("Windmill (Z, 40 deg/s)").clicked() {
                                self.studio.axis = "Z".to_string();
                                self.studio.speed_deg = 40.0;
                                self.studio.pivot_x = 0.0;
                                self.studio.pivot_y = 3.3;
                                self.studio.pivot_z = 1.35;
                            }
                            if ui.button("Waterwheel (X, 25 deg/s)").clicked() {
                                self.studio.axis = "X".to_string();
                                self.studio.speed_deg = 25.0;
                                self.studio.pivot_x = 0.0;
                                self.studio.pivot_y = 1.5;
                                self.studio.pivot_z = 0.0;
                            }
                            if ui.button("Carousel / Vane (Y, 30 deg/s)").clicked() {
                                self.studio.axis = "Y".to_string();
                                self.studio.speed_deg = 30.0;
                                self.studio.pivot_x = 0.0;
                                self.studio.pivot_y = 0.0;
                                self.studio.pivot_z = 0.0;
                            }
                            if ui.button("Fast Turbine (Z, 120 deg/s)").clicked() {
                                self.studio.axis = "Z".to_string();
                                self.studio.speed_deg = 120.0;
                                self.studio.pivot_x = 0.0;
                                self.studio.pivot_y = 0.0;
                                self.studio.pivot_z = 0.0;
                            }
                        });
                        ui.add_space(8.0);

                        // Axis
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Rotation Axis:").font(FontId::proportional(12.0)));
                            for a in &["X", "Y", "Z"] {
                                if ui.selectable_label(self.studio.axis == *a, *a).clicked() {
                                    self.studio.axis = a.to_string();
                                }
                            }
                        });
                        ui.add_space(6.0);

                        // Speed
                        ui.label(RichText::new(format!("Speed: {:.0} deg/s", self.studio.speed_deg)).font(FontId::proportional(12.0)));
                        ui.add(Slider::new(&mut self.studio.speed_deg, -180.0..=180.0).suffix(" deg/s"));
                        ui.add_space(8.0);

                        // Pivot
                        ui.label(RichText::new("Pivot Offset [X, Y, Z] (meters):").font(FontId::proportional(12.0)));
                        ui.horizontal(|ui| {
                            ui.label("X:");
                            ui.add(DragValue::new(&mut self.studio.pivot_x).speed(0.05));
                            ui.label("Y:");
                            ui.add(DragValue::new(&mut self.studio.pivot_y).speed(0.05));
                            ui.label("Z:");
                            ui.add(DragValue::new(&mut self.studio.pivot_z).speed(0.05));
                        });
                        ui.add_space(8.0);

                        ui.checkbox(&mut self.studio.generate_manifest, RichText::new("Generate clutter.json manifest").font(FontId::proportional(11.5)));
                    });

                    // RIGHT COLUMN: 3D REAL-TIME VIEWPORT & EXPORT
                    cols[1].group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("3D REAL-TIME PREVIEW").strong().font(FontId::proportional(13.5)).color(accent_amber));
                        ui.separator();
                        ui.add_space(4.0);

                        // REAL-TIME 3D VIEWPORT!
                        self.render_3d_preview(ui);

                        ui.add_space(8.0);

                        let install_btn = egui::Button::new(
                            RichText::new("INSTALL DIRECTLY TO GAME")
                                .font(FontId::proportional(13.5))
                                .strong()
                                .color(Color32::WHITE)
                        )
                        .fill(accent_green)
                        .min_size(Vec2::new(ui.available_width(), 38.0))
                        .corner_radius(CornerRadius::same(6));

                        if ui.add(install_btn).clicked() {
                            self.install_studio_prop();
                        }

                        ui.add_space(6.0);

                        let export_btn = egui::Button::new(
                            RichText::new("EXPORT MOD PACKAGE (.ZIP)")
                                .font(FontId::proportional(12.5))
                                .strong()
                                .color(Color32::WHITE)
                        )
                        .fill(accent_blue)
                        .min_size(Vec2::new(ui.available_width(), 34.0))
                        .corner_radius(CornerRadius::same(6));

                        if ui.add(export_btn).clicked() {
                            self.export_studio_zip();
                        }

                        ui.add_space(6.0);
                        ui.label(RichText::new(&self.studio.studio_status).font(FontId::proportional(11.5)).color(accent_amber));
                    });
                });
            }
        }

        // 4. ONBOARDING TUTORIAL MODAL
        if self.show_tutorial {
            egui::Window::new("GladeApp Quick Guide")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([540.0, 360.0])
                .show(ui.ctx(), |ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Welcome to GladeApp!").font(FontId::proportional(18.0)).strong().color(accent_green));
                    ui.label(RichText::new("Here is how to get the most out of your game and mods:").color(text_sub).font(FontId::proportional(12.0)));
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("1. One-Click Play & Mods:").strong().color(accent_amber));
                        ui.label("Check or uncheck mods in the list. Drag and drop any .zip mod directly into GladeApp to install it instantly.");
                    });
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("2. Infinite Map & Camera:").strong().color(accent_amber));
                        ui.label("Building boundaries are unlocked and camera zoom is extended to 150m. Configure modules on the left.");
                    });
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("3. Animation Studio:").strong().color(accent_amber));
                        ui.label("Switch to the 'Animation Studio' tab above to create and preview spinning windmills, waterwheels, and gears in 3D.");
                    });
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("4. Free Clutter Gizmo:").strong().color(accent_amber));
                        ui.label("In game, hover your mouse over any clutter object and press 'G' to lock and move it freely with O/U, J/L, I/K or mouse drag.");
                    });
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("5. Community Spotlight:").strong().color(Color32::from_rgb(110, 195, 255)));
                        ui.label("Special thanks to Tiny Grid by our community! Precision flat building grid designed specifically for Infinite Glade.");
                    });
                    ui.add_space(14.0);

                    ui.vertical_centered(|ui| {
                        let got_it_btn = egui::Button::new(
                            RichText::new("Got It! Let's Build")
                                .font(FontId::proportional(14.0))
                                .strong()
                                .color(Color32::WHITE)
                        )
                        .fill(accent_green)
                        .min_size(Vec2::new(220.0, 36.0))
                        .corner_radius(CornerRadius::same(6));

                        if ui.add(got_it_btn).clicked() {
                            self.config.tutorial_seen = true;
                            self.save_config();
                            self.show_tutorial = false;
                        }
                    });
                    ui.add_space(4.0);
                });
        }

        ui.add_space(4.0);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([860.0, 580.0])
            .with_min_inner_size([780.0, 500.0])
            .with_title("GladeApp - Tiny Glade Launcher"),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    let res = eframe::run_native(
        "GladeApp",
        options,
        Box::new(|cc| Ok(Box::new(GladeApp::new(cc)))),
    );
    if let Err(ref e) = res {
        let _ = std::fs::write("glade_launcher_error.txt", format!("{e:?}"));
    }
    res
}
