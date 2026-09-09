# Infinite Glade: Technical Deep Dive and Architecture

This document provides a comprehensive technical overview of the reverse engineering, runtime hooking, and mesh engineering techniques utilized to create **Infinite Glade**. It details the internal structure of Tiny Glade, the constraints imposed by its engine, and the precise mechanisms used to bypass them.

---

## 1. Project Background and Motivation

Tiny Glade is an indie building simulation developed by Pouncelight Games. The game is celebrated for its tactile, gridless procedural building mechanics and painterly aesthetic.

However, the vanilla game restricts building interactions to a circular clearing with a radius of approximately 40 meters. When players attempt to drag walls, draw paths, or position structures beyond this boundary, spatial collision functions reject the operations, rendering red boundary markers. Furthermore, the game camera is constrained by an aggressive perimeter clamp and a maximum zoom distance of 40 meters.

The objective of Infinite Glade was to:
1. Eliminate all spatial constraints on construction without destabilizing the procedural meshing pipeline.
2. Extend camera panning and zoom into the stratosphere (150 meters).
3. Transform the surrounding 27-meter decorative hill backdrop into an endless, flat building plane.
4. Suppress visual border clutter (boundary stones, billboard foliage, and Z-fighting rock meshes).
5. Maintain solid real-time rendering performance on mainstream hardware with zero engine panics or crashes.

---

## 2. Engine Architecture Overview

Tiny Glade is implemented in **Rust** using the **Bevy ECS** (Entity Component System) framework and a custom Vulkan-based procedural renderer named **Rhapsody**.

### Key Characteristics of the Target Binary
- **Monolithic Compilation**: The binary (`tiny-glade.exe`) is compiled as a release x64 MSVC binary with aggressive LLVM optimizations, cross-crate inlining, and mangled symbol names.
- **Bevy System Dispatch**: Gameplay systems are organized into stages and schedules. Systems execute in parallel where archetype queries permit, and state mutations are deferred via command buffers.
- **Vulkan SSBO Allocations**: Procedural geometry and asset instances are backed by Vulkan Shader Storage Buffer Objects (SSBOs). The engine strictly enforces buffer allocation sizes through debug and runtime assertions (e.g., `assertion failed: size > 0`).
- **Asset Verification & Mod Tagging**: Tiny Glade has no anti-tamper DRM or anti-cheat. On startup, the engine inspects asset package state. Modified files cause the game to display a `+mods` watermark under the Tiny Glade logo in screenshots (to preserve the integrity of vanilla building challenges) and provide diagnostic info in crash logs if a crash occurs.

---

## 3. The Reverse Engineering Process

The analysis began by analyzing symbol information and disassembling critical systems within `tiny-glade.exe`.

### 3.1 Asset Integrity Verification Bypass
Tiny Glade includes an internal integrity verification pass during startup (`verify_package_integrity_and_show_report`). While this is not an anti-tamper DRM system, during early development remote thread injection timing combined with modified runtime state could trigger early diagnostic aborts.

- **Target Location**: `RVA +0x176F5F` (v1.16.0-pre4) / `0x2D69FF` (v1.16.0)
- **Mechanism**: Evaluates package manifests and runtime state for crash diagnostics.
- **Resolution**: An unconditional jump patch (`E9 DF 00 00 00 90` to offset `+0xDF` / `0x2D6AE3`) routes execution cleanly past the diagnostic handler directly to standard completion, guaranteeing consistent startup when injected.

### 3.2 Spatial Boundary Validation (`GladeBorder`)
The game enforces construction limits using a dedicated spatial boundary component, `GladeBorder`. When the player manipulates a building tool, the cursor position and geometry bounds are queried against four core methods:

1. `is_within_glade_shape` (`RVA +0x9D4A10`)
2. `GladeBorder::is_pos_inside` (`RVA +0x91C400`)
3. `GladeBorder::is_shape_inside` (`RVA +0x91C420`)
4. `GladeBorder::is_curve2_inside` (`RVA +0x91C710`)

Each function accepts coordinates or polygon structures, performs mathematical intersection tests against the active glade boundary, and returns a boolean value in the `AL` register.

- **Resolution**: Rather than hooking these functions through detours, each entry point is patched with:
  ```assembly
  mov al, 1
  ret
  ```
  Binary representation: `B0 01 C3` (3 bytes).
  This eliminates all computational overhead, immediately returning `true` for every spatial query and allowing structural placement anywhere in the coordinate space.

### 3.3 Camera Rig Modification
The camera controller is managed by `country_core::systems::camera_rig::normal::update` and related subroutines. Camera movement was subject to three distinct clamps:

1. **Pan Delta Clamping (`RVA +0x90E88D`)**:
   Camera pan velocity was clamped against a limit stored in `[rdi + 0x64]`. Patched from `jbe` to unconditional `jmp` (`EB 2B`).
2. **Perimeter Clamping (`RVA +0x90E908`)**:
   The camera focus target was clamped to the glade radius. Patched with an unconditional jump (`E9 03 03 00 00 90`) routing directly to the camera position update logic at offset `0x14090ec10`.
3. **Alternate Mode Clamping (`RVA +0x90EA1A`)**:
   Patched from `jbe` to `jmp` (`E9 08 01 00 00 90`).
4. **Stratosphere Zoom Extension (`RVA +0x2EB6074`)**:
   The maximum camera distance was defined by an IEEE-754 single-precision float (`40.0f`) stored in the static data section. By modifying memory protection via `VirtualProtect`, this value was rewritten to `150.0f`, extending camera altitude by 375%.
5. **Zoom Distance Check (`RVA +0xA7B7BE`)**:
   The routine `country_core::systems::camera_rig::MainCamera::zoom` contained an auxiliary boundary check. This check was NOP'd out with six `0x90` bytes.

### 3.4 Elimination of Boundary Artifacts
Vanilla Tiny Glade renders decorative boundary stones and posts along the perimeter through `display_glade_border` (`RVA +0x2069E41`). Additionally, photomode routines evaluate whether to draw boundary lines.

- **Resolution**: The branch at `RVA +0x2069E41` (`75 19` / `jne`) was patched with two `NOP` instructions (`90 90`). This forces the routine to immediately execute the clean exit path, suppressing boundary markers entirely.
- **Backdrop Forest Removal (`RVA +0x19BC480`)**:
  Background tree spawning in `country_core::startup::clearing::init_clearing::closure_env$1` was patched to return an empty collection (`mov dword ptr [rcx], 0; mov rax, rcx; ret`), preventing 1,500+ distant background billboard entities from being instantiated.

---

## 4. Mesh Engineering and Vulkan Storage Buffer Management

While runtime binary patching resolved logic constraints, the game world visual presentation was marred by background terrain geometry: 27-meter steep hills surrounded the glade, and distant foliage generated severe Z-fighting artifacts.

### The Buffer Allocation Dilemma
The immediate instinct when removing unwanted assets is to delete the corresponding JSON mesh files (`assets/meshes/*.json`) or replace their contents with empty arrays `[]`.

However, doing so caused Tiny Glade to crash during the Vulkan initialization pass:
```text
thread 'main' panicked at 'assertion failed: size > 0', rhapsody/src/vulkan/buffer.rs:142
```
The Rhapsody renderer allocates Vulkan SSBOs based on the byte length of deserialized vertex and index buffers. A zero-length buffer violates the precondition `size > 0` and triggers an immediate panic.

### The Mesh Suppression Approaches

#### 1. The Subterranean Translation Workaround (Initial PoC)
To bypass the panic without modifying the graphics engine binary:
1. **Preservation of Buffer Schemas**: Vertex counts, triangle indices, and metadata were left structurally intact.
2. **Coordinate Translation to Subterranean Space**: A processing script parsed the JSON mesh files (`terrain_rocks.json`, `billboard_plants.json`, `billboard_plants_2.json`, `distant_billboard.json`, `far_distance_tree.json`) and translated the Y coordinate (height) of every vertex to `Y = -500.0`.
   - Result: The meshes still allocate valid Vulkan buffers, satisfying engine assertions.
   - The geometry renders 500 meters beneath the terrain plane, invisible to the camera.

#### 2. The Degenerate Triangle Optimization (Engine-Native Best Practice)
As recommended directly by engine developer Tomasz Stachowiak, an even cleaner and mathematically optimal solution is replacing unwanted meshes with a single **degenerate triangle**:
- A single triangle consisting of 3 identical vertices at `[0.0, 0.0, 0.0]`.
- This satisfies the Vulkan buffer allocator assertion (`size > 0`), but because the triangle has zero area, the hardware rasterizer / primitive clipping unit immediately discards it before pixel shading.
- Result: Clean JSON definitions, minimal memory footprint, and zero rasterization cost.

#### 3. Horizon Skirt Normalization (`terrain.json`)
The perimeter skirt mesh containing 1,078 vertices was processed so that all vertex positions were clamped to `Y = 0.0` with surface normal vectors aligned to `[0.0, 1.0, 0.0]`. This transformed the obstructive hills into an endless, flat horizon.

---

## 5. Loader and Process Injection Architecture

The loader (`GladeLoader.exe`) implements native Win32 remote thread injection to guarantee patches are applied before engine startup procedures initialize static resources.

### Injection Sequence
1. **Suspended Process Creation**:
   `GladeLoader` invokes `CreateProcessA` with the `CREATE_SUSPENDED` flag targeting `tiny-glade.exe`. The main thread is frozen before executing entry point routines.
2. **Virtual Memory Allocation**:
   `VirtualAllocEx` reserves a memory region within the target process space with `PAGE_READWRITE` permissions to hold the absolute path to `glade_loader.dll`.
3. **Path Serialization**:
   `WriteProcessMemory` writes the DLL filesystem path into the allocated target buffer.
4. **Remote Execution**:
   `CreateRemoteThread` is called, passing the address of `Kernel32!LoadLibraryA` as the thread procedure and the path buffer as the argument.
5. **Main Thread Resumption**:
   `ResumeThread` releases the suspended primary thread, allowing standard initialization to proceed in tandem with DLL loading.

### DLL Initialization (`glade_loader.dll`)
Inside `DllMain`, upon receiving `DLL_PROCESS_ATTACH`:
1. A dedicated initialization thread is created via `CreateThread` to avoid loader lock deadlocks.
2. `SteamAPI_RestartAppIfNecessary` is detoured via MinHook to always return `false`, preventing Steam from terminating the injected process.
3. Memory protection flags on code pages are temporarily elevated to `PAGE_EXECUTE_READWRITE` via `VirtualProtect`.
4. RVA-relative patches are copied into place using atomic memory writes.
5. Original memory permissions are restored.

---

## 6. Technical Roadmap and Future Improvements

The current v1.0.0 implementation functions as a robust Proof of Concept. Future milestones include:

1. **Dynamic Pattern Scanning (AOB / Signature Scanning)**:
   Replacing hardcoded RVAs with unique byte pattern masks. This will allow the loader to automatically locate target functions across game updates, even if compiler re-alignment shifts function offsets.
2. **Building Dimension Limits**:
   Patching `clamp_wall_height`, `ui_edit_circle`, and `ui_edit_rectangle_dims` to allow procedural building footprints to scale beyond standard vanilla radius ceilings.
3. **External Configuration File**:
   Exposing camera altitude limits, movement speeds, and terrain rendering options through a user-editable `config.toml`.
