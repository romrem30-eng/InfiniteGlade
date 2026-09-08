# 🏰 Infinite Glade

[![Release](https://img.shields.io/github/v/release/romrem30-eng/InfiniteGlade?style=for-the-badge&color=brightgreen)](https://github.com/romrem30-eng/InfiniteGlade/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Target: Tiny Glade](https://img.shields.io/badge/Game-Tiny%20Glade%20v1.16.0-blue?style=for-the-badge)](https://store.steampowered.com/app/2198150/Tiny_Glade/)
[![Status: Proof of Concept](https://img.shields.io/badge/Status-Proof%20of%20Concept-orange?style=for-the-badge)](https://github.com/romrem30-eng/InfiniteGlade/issues)

**Infinite Glade** is the first gameplay & camera overhaul mod for **Tiny Glade**, removing all boundary restrictions and expanding the building canvas into an endless flat horizon with 150-meter stratosphere camera zoom.

> [!NOTE]
> **Project Status: Proof of Concept (PoC / Tech Preview)**  
> This project demonstrates that Tiny Glade's internal boundary and camera constraints can be completely unlocked at runtime without sacrificing engine stability or rendering performance. Active development and community feedback are welcome!

---

## ✨ Features

- 🏗️ **Infinite Build Area**: Build castles, houses, walls, towers, roofs, stairs, and plant trees anywhere without being stopped by the invisible border.
- 🦅 **Free Camera & 3.7x Zoom**: Unlocked camera panning past the clearing edge and extended maximum zoom distance from `40m` to `150m`.
- 🌅 **Infinite Flat Horizon**: The 27-meter background terrain hills are flattened to $Y = 0.0$, creating a seamless endless canvas for mega-builds.
- 🧹 **Zero Clutter / Clean Skirt**: Photomode border stones, flickering rocks (Z-fighting), 2D billboards, and 1,500+ distant background bushes are cleanly submerged underground.
- ⚡ **Anti-Tamper Bypass**: Built-in runtime integrity check bypass for stable, crash-free execution at 60+ FPS.

---

## 🖥️ Verified Test Rig & Performance Benchmark

The mod has been thoroughly tested on mainstream mobile gaming hardware and delivers smooth, rock-solid performance:

| Component | Specification |
| :--- | :--- |
| **CPU** | AMD Ryzen 5 4600H (6 Cores / 12 Threads @ up to 4.0 GHz) |
| **GPU** | NVIDIA GeForce GTX 1650 Ti (4 GB VRAM) |
| **RAM** | 16 GB DDR4 |
| **OS** | Windows 11 / 10 (x64) |
| **Target Build** | Steam `v1.16.0-pre4` |
| **Framerate** | **Solid 60 FPS** during active building, terrain painting, and 150m stratosphere zoom |

---

## 📦 Quick Installation (1-Minute)

1. Download **`InfiniteGlade-v1.0.0.zip`** from [**Releases**](https://github.com/romrem30-eng/InfiniteGlade/releases).
2. Extract the archive contents into your **Tiny Glade** folder:
   - Default Steam path: `C:\Program Files (x86)\Steam\steamapps\common\Tiny Glade\`
3. Run **`install.bat`** (or copy `GladeLoader.exe`, `glade_loader.dll`, and the `assets\` folder manually).
4. Launch the game using **`GladeLoader.exe`** (or your normal desktop shortcut)!

---

## 💾 Save Game Safety & Backup Advice

> [!IMPORTANT]
> **Always back up your saves before creating massive out-of-bounds projects.**  
> Tiny Glade safely stores all objects placed outside the glade in your standard world saves. However, if you later launch the game in **vanilla mode (without the loader)**, vanilla boundary restrictions will prevent you from editing or extending structures located beyond the 40m perimeter.

- **Save Folder Location:**  
  `%USERPROFILE%\Saved Games\Tiny Glade\Steam\<YourSteamID>\saves\`
- To back up your worlds, simply make a copy of the `saves` folder.

---

## ⚠️ Known Edge Cases & Technical Notes

1. **Static RVAs vs. Official Game Updates:**  
   The current PoC release relies on hardcoded RVAs (Relative Virtual Addresses) matched specifically against Steam build `v1.16.0-pre4`.  
   *If an official game update changes function offsets and the loader fails to hook, please open an [Issue](https://github.com/romrem30-eng/InfiniteGlade/issues). We will promptly re-scan the binary and release updated offsets! Dynamic AOB (Array of Bytes) pattern scanning is on the roadmap.*
2. **Horizon Edge Visibility at 150m:**  
   Because the background 27m obstructive hills have been flattened to $Y = 0.0$, pulling the camera out to the maximum 150m stratosphere distance will reveal the circular edge of the procedural world ground disc.
3. **VRAM Scaling with Mega-Structures:**  
   Tiny Glade uses a real-time procedural voxel meshing pipeline (Marching Cubes / SDF). When building monumental kingdoms with thousands of walls and roofs, monitor your GPU VRAM usage on 4GB cards.

---

## 🛠️ How It Works (Technical Overview)

Tiny Glade is built in Rust on the **Bevy ECS** engine with custom Vulkan rendering (`Rhapsody`). The mod works on two complementary layers:

1. **Native Runtime Patching (`glade_loader.dll`)**:
   - Injected into `tiny-glade.exe` at startup via remote thread injection.
   - Bypasses the binary's internal anti-tamper verification loop (`RVA +0x176F5F`).
   - Patches 4 border constraint methods to return `true` (`mov al, 1; ret`):
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

## 💬 Community & Feedback

- Found a bug or compatibility issue with a new game version? Open a [GitHub Issue](https://github.com/romrem30-eng/InfiniteGlade/issues).
- Want to showcase your colossal castles or discuss ideas? Start a [GitHub Discussion](https://github.com/romrem30-eng/InfiniteGlade/discussions)!

---

## 📜 License

This project is licensed under the [MIT License](LICENSE).
