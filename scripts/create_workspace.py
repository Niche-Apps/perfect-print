#!/usr/bin/env python3
import os

base = "/Users/josephsee/clawd/perfect-print/crates"

crates_info = [
    ("perfect-print-core", ["serde", "serde_json", "thiserror"],
     "Canonical document model, units, pages, layers, draw commands, styles, resources"),
    ("perfect-print-layout", ["perfect-print-core", "rustybuzz", "ttf-parser", "fontdb", "log"],
     "Text layout, flow layout, pagination, tables, headers, footers"),
    ("perfect-print-render", ["perfect-print-core", "tiny-skia", "log"],
     "Renderer traits and raster/vector adapters"),
    ("perfect-print-pdf", ["perfect-print-core", "perfect-print-layout", "perfect-print-render", "lopdf", "log"],
     "PDF output from the canonical page model"),
    ("perfect-print-dialog", ["perfect-print-core", "thiserror", "log"],
     "Printer settings, printer capabilities, native dialog abstraction"),
    ("perfect-print-backend-macos", ["perfect-print-core", "perfect-print-pdf", "log"],
     "macOS native print backend"),
    ("perfect-print-backend-windows", ["perfect-print-core", "perfect-print-pdf", "log"],
     "Windows native print backend"),
    ("perfect-print-backend-linux", ["perfect-print-core", "perfect-print-pdf", "log"],
     "Linux CUPS/IPP backend"),
    ("perfect-print-preview", ["perfect-print-core", "perfect-print-render", "perfect-print-pdf", "log"],
     "Preview support"),
    ("perfect-print-tauri", ["perfect-print-core", "perfect-print-pdf", "log"],
     "Tauri integration"),
    ("perfect-print-egui", ["perfect-print-core", "perfect-print-render", "log"],
     "egui integration"),
    ("perfect-print-iced", ["perfect-print-core", "perfect-print-render", "log"],
     "iced integration"),
    ("perfect-print-cli", ["perfect-print-core", "perfect-print-layout", "perfect-print-render", "perfect-print-pdf", "perfect-print-dialog", "clap", "env_logger", "anyhow"],
     "CLI for render, verify, printers, capabilities, print, diagnostics"),
    ("perfect-print", ["perfect-print-core", "perfect-print-layout", "perfect-print-render", "perfect-print-pdf", "perfect-print-dialog", "log"],
     "Public ergonomic API"),
]

ws_simple = {
    "serde": '{ workspace = true }',
    "serde_json": '{ workspace = true }',
    "thiserror": '{ workspace = true }',
    "anyhow": '{ workspace = true }',
    "rustybuzz": '{ workspace = true }',
    "ttf-parser": '{ workspace = true }',
    "fontdb": '{ workspace = true }',
    "lopdf": '{ workspace = true }',
    "tiny-skia": '{ workspace = true }',
    "image": '{ workspace = true }',
    "log": '{ workspace = true }',
    "env_logger": '{ workspace = true }',
    "clap": '{ workspace = true }',
    "insta": '{ workspace = true }',
    "approx": '{ workspace = true }',
    "objc2": '{ workspace = true, optional = true }',
    "objc2-foundation": '{ workspace = true, optional = true }',
    "core-graphics": '{ workspace = true, optional = true }',
    "core-text": '{ workspace = true, optional = true }',
    "core-foundation": '{ workspace = true, optional = true }',
    "cups-sys": '{ workspace = true, optional = true }',
}

for name, deps, desc in crates_info:
    crate_dir = os.path.join(base, name)
    src_dir = os.path.join(crate_dir, "src")
    os.makedirs(src_dir, exist_ok=True)

    dep_lines = []
    for d in deps:
        if d.startswith("perfect-print-"):
            dep_lines.append(f'{d} = {{ path = "../{d}" }}')
        elif d in ws_simple:
            dep_lines.append(f"{d} = {ws_simple[d]}")
        else:
            dep_lines.append(f'{d} = {{ workspace = true }}')

    deps_section = "\n".join(dep_lines)

    cargo_toml = f"""[package]
name = "{name}"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
description = "{desc}"

[dependencies]
{deps_section}
"""

    with open(os.path.join(crate_dir, "Cargo.toml"), "w") as f:
        f.write(cargo_toml)

    lib_rs = f"//! {desc}\n"
    with open(os.path.join(src_dir, "lib.rs"), "w") as f:
        f.write(lib_rs)

print(f"Created {len(crates_info)} crates")
