# Studio Code Editor — GTK 4 + Pango/Cairo

Pure Rust + **GTK 4 + Pango/Cairo** GPU-accelerated upgrade of Shay Kadosh's Studio Code Editor (`CODE-EDIT`), keeping the original FLTK version completely intact.

## Features
- **24 Built-in Studio Themes** with dynamic GTK 4 CSS styling (`Tokyo Night`, `Dracula`, `Catppuccin`, `Matrix Emerald`, `Cyberpunk Neon`, `Nord`, `Gruvbox`, `Kanagawa`, `Rose Pine`, etc.)
- **Pango/Cairo Typography**: True coding ligatures (`->`, `=>`, `!=`, `::`), full **Hebrew RTL & BiDi** support, HiDPI sharpness, searchable system font picker
- **Multi-Cursor**: `Ctrl+Click`, `Ctrl+D` (next match), `Alt+Shift+Up/Down`
- **Code Folding** (click gutter `▾`/`▸` or `Ctrl+Shift+[`), **Sticky Scroll**, **Indent Guides**, **Rainbow Brackets**, **Inline `#RRGGBB` Color Previews**, **Whitespace Markers**
- **Live Minimap** (clickable & draggable) + smooth custom scrollbar
- **Find & Replace** (`Ctrl+F` / `Ctrl+H`), **Go to Line** (`Ctrl+G`), **Command Palette** (`Ctrl+Shift+P`), **Autocomplete** (`Ctrl+Space` or automatic while typing)
- **Optional Vim Mode** (`Normal` / `Insert` / `Visual` / `VisualLine`)
- **Hebrew Keyboard Layout Hotkeys** (`Ctrl+ש/ד/ב/כ` work identically to `Ctrl+A/S/C/F`)

## Build (inside `my-ubuntu`)
```bash
podman exec my-ubuntu bash -lc 'source /etc/profile.d/flutter-android.sh && cd /workspace/GTK-CODE-EDITOR && cargo build --release'
```
