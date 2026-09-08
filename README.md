# 🏰 Infinite Glade

[![Release](https://img.shields.io/github/v/release/romrem30-eng/InfiniteGlade?style=for-the-badge&color=brightgreen)](https://github.com/romrem30-eng/InfiniteGlade/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Target: Tiny Glade](https://img.shields.io/badge/Game-Tiny%20Glade-blue?style=for-the-badge)](https://store.steampowered.com/app/2198150/Tiny_Glade/)

**Infinite Glade** is the first gameplay & camera overhaul mod for **Tiny Glade**, removing all boundary restrictions and expanding the building canvas into an endless flat horizon.

---

## ✨ Features

- 🏗️ **Infinite Build Area**: Build castles, houses, walls, towers, roofs, stairs, and plant trees anywhere without being stopped by the invisible border.
- 🦅 **Free Camera & 3.7x Zoom**: Unlocked camera panning past the clearing edge and extended maximum zoom distance from `40m` to `150m`.
- 🌅 **Infinite Flat Horizon**: The 27-meter background terrain hills are flattened to $Y = 0.0$, creating a seamless endless canvas for mega-builds.
- 🧹 **Zero Clutter / Clean Skirt**: Photomode border stones, flickering rocks (Z-fighting), 2D billboards, and 1,500+ distant background bushes are cleanly submerged underground.
- ⚡ **Anti-Tamper Bypass**: Built-in runtime integrity check bypass for stable, crash-free execution at 60+ FPS.

---

## 📦 Quick Installation (1-Minute)

1. Download **`InfiniteGlade-v1.0.0.zip`** from [**Releases**](https://github.com/romrem30-eng/InfiniteGlade/releases).
2. Extract the archive contents into your **Tiny Glade** folder:
   - Default Steam path: `C:\Program Files (x86)\Steam\steamapps\common\Tiny Glade\`
3. Run **`install.bat`** (or copy `GladeLoader.exe`, `glade_loader.dll`, and the `assets\` folder manually).
4. Launch the game using **`GladeLoader.exe`** (or your normal shortcut)!

---

## 🛠️ How It Works (Technical Overview)

Tiny Glade is built on Rust using the **Bevy ECS** engine with custom Vulkan rendering (`Rhapsody`). The mod works on two levels:

1. **Native Runtime Patching (`glade_loader.dll`)**:
   - Injected into `tiny-glade.exe` at startup.
   - Bypasses the binary's internal anti-tamper verification loop (`RVA +0x176F5F`).
   - Hooks/patches 4 border constraint checks to always return `true`:
     - `is_within_glade_shape` (`RVA +0x9D4A10`)
     - `GladeBorder::is_pos_inside` (`RVA +0x91C400`)
     - `GladeBorder::is_shape_inside` (`RVA +0x91C420`)
     - `GladeBorder::is_curve2_inside` (`RVA +0x91C710`)
   - Unlocks camera pan delta & perimeter clamps and bumps max zoom to `150.0m` (`RVA +0x2EB6074`).
   - Removes photomode border stones by intercepting `display_glade_border` (`RVA +0x2069E41`).

2. **Horizon Mesh Engineering (`assets/meshes/*.json`)**:
   - `terrain.json`: 1,078 skirt vertices normalized to $Y = 0.0$ with up-normals $[0, 1, 0]$.
   - `terrain_rocks.json`, `billboard_plants*.json`, `far_distance_tree.json`: Model vertices dropped to $Y = -500.0$. Buffer sizes and vertex counts are preserved to guarantee Vulkan SSBO allocations never panic (`assertion failed: size > 0`).

---

## 🔧 Building from Source

Requirements:
- [Rust](https://www.rust-lang.org/) (nightly or latest stable with 2024 edition support)
- Visual Studio Build Tools / MSVC x64

```bash
cd glade_loader
cargo build --release
```
Compiled output will be in `target/release/GladeLoader.exe` and `target/release/glade_loader.dll`.

---

## 📜 License

This project is licensed under the [MIT License](LICENSE).
