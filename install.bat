@echo off
setlocal enabledelayedexpansion
title Infinite Glade - Mod Installer

echo =======================================================
echo          Infinite Glade - 1-Click Installer
echo =======================================================
echo.

set "GAME_DIR=C:\Program Files (x86)\Steam\steamapps\common\Tiny Glade"

if not exist "!GAME_DIR!\tiny-glade.exe" (
    if exist "tiny-glade.exe" (
        set "GAME_DIR=%CD%"
    ) else (
        echo [ERROR] Tiny Glade installation not found at default path.
        echo Please place this installer inside your Tiny Glade folder.
        pause
        exit /b 1
    )
)

echo [OK] Detected Tiny Glade at: "!GAME_DIR!"
echo.

echo [1/2] Installing GladeLoader & DLL...
copy /Y "bin\GladeLoader.exe" "!GAME_DIR!\GladeLoader.exe" >nul
copy /Y "bin\glade_loader.dll" "!GAME_DIR!\glade_loader.dll" >nul

echo [2/2] Installing Infinite Horizon Meshes...
if not exist "!GAME_DIR!\assets\meshes" mkdir "!GAME_DIR!\assets\meshes"

for %%F in (terrain.json terrain_rocks.json billboard_plants.json billboard_plants_2.json distant_billboard.json tree_billboard.json far_distance_tree.json forest_terrain.json) do (
    if exist "assets\meshes\%%F" (
        copy /Y "assets\meshes\%%F" "!GAME_DIR!\assets\meshes\%%F" >nul
    )
)

echo.
echo =======================================================
echo [SUCCESS] Installation Complete!
echo Run GladeLoader.exe in your Tiny Glade folder to play!
echo =======================================================
echo.
pause
