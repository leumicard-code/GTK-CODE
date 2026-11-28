
<img width="260" height="100" alt="Vulkan_100px_Dec16" src="https://github.com/user-attachments/assets/a1bab2f3-0a49-4269-8085-5f08f83e2cfd" />


<img width="1571" height="1011" alt="image" src="https://github.com/user-attachments/assets/39058539-bb51-4b11-9805-459d532ae37d" />
<img width="1571" height="1011" alt="image" src="https://github.com/user-attachments/assets/af57dfc0-fd20-46d6-8eb0-92eb7c6ad273" />










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
