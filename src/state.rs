// ============================================================================
// state.rs — Core Studio IDE State, Workspace Tree, Vim Mode & Command Palette
// ============================================================================

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{AppConfig, ToolbarStyle};
use crate::document::{Document, IndentStyle, Pos};
use crate::syntax::SyntaxEngine;
use crate::themes::EditorTheme;

#[derive(Clone, Debug)]
pub struct SidebarEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_parent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimMode {
    Off,
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl VimMode {
    pub fn badge(self) -> &'static str {
        match self {
            Self::Off => "",
            Self::Normal => "VIM: NORMAL",
            Self::Insert => "VIM: INSERT",
            Self::Visual => "VIM: VISUAL",
            Self::VisualLine => "VIM: V-LINE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    FormatDocument,
    ToggleComment,
    DuplicateLine,
    MoveLineUp,
    MoveLineDown,
    AddCursorNextOccurrence,
    ToggleSidebar,
    ToggleMinimap,
    ToggleBookmark,
    NextBookmark,
    GoToLine,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleWordWrap,
    ToggleStickyScroll,
    ToggleColorPreviews,
    ToggleRainbowBrackets,
    ToggleWhitespace,
    ToggleIndentGuides,
    ToggleFoldAtCursor,
    UnfoldAll,
    ToggleVimMode,
    OpenFind,
    OpenReplace,
    NewTab,
    CloseFile,
    IndentSpaces4,
    IndentSpaces2,
    IndentTabs,
    AboutDeveloper,
    SetTheme(usize),
}

pub struct PaletteEntry {
    pub label: String,
    pub shortcut: &'static str,
    pub action: PaletteAction,
}

#[derive(Default)]
pub struct HighlightCache {
    pub doc_id: u64,
    pub revision: u64,
    pub theme_idx: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub lines: Vec<Vec<crate::syntax::StyledSpan>>,
}

pub struct EditorState {
    pub docs: Vec<Document>,
    pub active_doc: usize,
    pub theme_idx: usize,
    pub font_size: i32,
    pub font_name: String,
    pub toolbar_style: ToolbarStyle,
    pub syntax: SyntaxEngine,
    pub highlight_cache: HighlightCache,
    pub sidebar_revision: u64,
    pub show_sidebar: bool,
    pub show_minimap: bool,
    pub show_whitespace: bool,
    pub show_indent_guides: bool,
    pub rainbow_brackets: bool,
    pub sticky_scroll: bool,
    pub inline_color_previews: bool,
    pub vim_mode: VimMode,
    pub vim_pending_key: Option<char>,
    pub vim_yank_buffer: String,
    pub find_query: String,
    pub replace_query: String,
    pub autocomplete_open: bool,
    pub autocomplete_selected: usize,
    pub workspace_dir: PathBuf,
    pub sidebar_entries: Vec<SidebarEntry>,
    pub caret_visible: bool,
    pub dragging_selection: bool,
    pub dragging_minimap: bool,
    pub dragging_vscroll: Option<(f64, usize)>,
    pub hover_vscroll: bool,
    pub hover_pos: Option<Pos>,
    pub last_canvas_w: f64,
    pub last_canvas_h: f64,
    pub char_width: f64,
    pub line_height: f64,
}

const WELCOME_SAMPLE: &str = r##"// ============================================================================
// Studio Code Editor — Pure GTK 4 + Pango/Cairo GPU-Accelerated IDE
// ============================================================================
// Highlights:
//   • 24 Studio Themes (Tokyo Night, Dracula, Catppuccin, Matrix Emerald, ...)
//   • Pango/Cairo HiDPI Typography with Ligatures & Hebrew (עברית RTL) Support
//   • Multi-Cursor (Ctrl+Click / Ctrl+D / Alt+Shift+Up/Down)
//   • Code Folding (click gutter ▾), Sticky Scroll, Minimap & Inline Colors
//   • Command Palette (Ctrl+Shift+P), Find & Replace (Ctrl+F / Ctrl+H)
//   • Optional Vim Mode (View -> Vim Mode)

pub struct AudioEngineV10 {
    pub sample_rate: u32,
    pub accent_hex: &'static str, // Inline color preview: #38BDF8 #10B981 #F59E0B
}

impl AudioEngineV10 {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            accent_hex: "#38BDF8",
        }
    }

    pub fn process_frame(&self, samples: &[f32]) -> Vec<f32> {
        samples
            .iter()
            .map(|&s| (s * 1.15).clamp(-1.0, 1.0))
            .collect()
    }
}
"##;

/// Decodes `file://` URIs and resolves relative paths to canonical `PathBuf`s.
pub fn resolve_cli_path(raw: &Path) -> PathBuf {
    let s = raw.to_string_lossy();
    let cleaned = if let Some(rest) = s.strip_prefix("file://") {
        let mut bytes = Vec::with_capacity(rest.len());
        let b = rest.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                if let Ok(hex_str) = std::str::from_utf8(&b[i + 1..i + 3]) {
                    if let Ok(val) = u8::from_str_radix(hex_str, 16) {
                        bytes.push(val);
                        i += 3;
                        continue;
                    }
                }
            }
            bytes.push(b[i]);
            i += 1;
        }
        PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        raw.to_path_buf()
    };

    let abs = if cleaned.is_absolute() {
        cleaned
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(cleaned)
    };
    abs.canonicalize().unwrap_or(abs)
}

impl EditorState {
    pub fn new_with_args(cli_args: &[PathBuf]) -> Self {
        let cfg = AppConfig::load();
        let mut docs = Vec::new();
        let mut workspace_dir = cfg
            .workspace_dir
            .clone()
            .filter(|d| d.is_dir())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Open CLI files first if supplied
        for raw in cli_args {
            let p = resolve_cli_path(raw);
            if p.is_dir() {
                workspace_dir = p;
            } else if p.is_file() {
                if let Ok(text) = fs::read_to_string(&p) {
                    let title = p
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("file")
                        .to_string();
                    if let Some(parent) = p.parent() {
                        workspace_dir = parent.to_path_buf();
                    }
                    docs.push(Document::new_with_text(&title, Some(p), &text));
                }
            }
        }

        // Restore previously open files from session if no CLI files were passed
        if docs.is_empty() {
            for p in &cfg.open_files {
                if p.is_file() {
                    if let Ok(text) = fs::read_to_string(p) {
                        let title = p
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("file")
                            .to_string();
                        docs.push(Document::new_with_text(&title, Some(p.clone()), &text));
                    }
                }
            }
        }

        if docs.is_empty() {
            docs.push(Document::new_with_text("welcome.rs", None, WELCOME_SAMPLE));
        }

        let active_doc = cfg.active_doc.min(docs.len().saturating_sub(1));

        let mut st = Self {
            docs,
            active_doc,
            theme_idx: cfg.theme_idx % EditorTheme::all().len(),
            font_size: cfg.font_size.clamp(9, 36),
            font_name: if cfg.font_name.trim().is_empty() {
                "Cascadia Code".to_string()
            } else {
                cfg.font_name
            },
            toolbar_style: cfg.toolbar_style,
            syntax: SyntaxEngine::new(),
            highlight_cache: HighlightCache::default(),
            sidebar_revision: 0,
            show_sidebar: cfg.show_sidebar,
            show_minimap: cfg.show_minimap,
            show_whitespace: cfg.show_whitespace,
            show_indent_guides: cfg.show_indent_guides,
            rainbow_brackets: cfg.rainbow_brackets,
            sticky_scroll: cfg.sticky_scroll,
            inline_color_previews: cfg.inline_color_previews,
            vim_mode: if cfg.vim_enabled {
                VimMode::Normal
            } else {
                VimMode::Off
            },
            vim_pending_key: None,
            vim_yank_buffer: String::new(),
            find_query: String::new(),
            replace_query: String::new(),
            autocomplete_open: false,
            autocomplete_selected: 0,
            workspace_dir,
            sidebar_entries: Vec::new(),
            caret_visible: true,
            dragging_selection: false,
            dragging_minimap: false,
            dragging_vscroll: None,
            hover_vscroll: false,
            hover_pos: None,
            last_canvas_w: 900.0,
            last_canvas_h: 600.0,
            char_width: 9.0,
            line_height: 22.0,
        };
        st.refresh_sidebar();
        st
    }

    pub fn doc(&self) -> &Document {
        &self.docs[self.active_doc]
    }

    pub fn doc_mut(&mut self) -> &mut Document {
        &mut self.docs[self.active_doc]
    }

    pub fn theme(&self) -> &'static EditorTheme {
        EditorTheme::by_index(self.theme_idx)
    }

    pub fn save_session(&self) {
        let cfg = AppConfig {
            theme_idx: self.theme_idx,
            font_size: self.font_size,
            font_name: self.font_name.clone(),
            font_path: None,
            toolbar_style: self.toolbar_style,
            show_minimap: self.show_minimap,
            show_sidebar: self.show_sidebar,
            show_whitespace: self.show_whitespace,
            show_indent_guides: self.show_indent_guides,
            rainbow_brackets: self.rainbow_brackets,
            sticky_scroll: self.sticky_scroll,
            inline_color_previews: self.inline_color_previews,
            vim_enabled: self.vim_mode != VimMode::Off,
            workspace_dir: Some(self.workspace_dir.clone()),
            open_files: self.docs.iter().filter_map(|d| d.path.clone()).collect(),
            active_doc: self.active_doc,
        };
        cfg.save();
    }

    pub fn refresh_sidebar(&mut self) {
        self.sidebar_entries.clear();
        if let Some(parent) = self.workspace_dir.parent() {
            self.sidebar_entries.push(SidebarEntry {
                name: "..".to_string(),
                path: parent.to_path_buf(),
                is_dir: true,
                is_parent: true,
            });
        }
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.workspace_dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "target" || name == ".git" || name == "node_modules" {
                    continue;
                }
                let is_dir = p.is_dir();
                let item = SidebarEntry {
                    name,
                    path: p,
                    is_dir,
                    is_parent: false,
                };
                if is_dir {
                    dirs.push(item);
                } else {
                    files.push(item);
                }
            }
        }
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.sidebar_entries.extend(dirs);
        self.sidebar_entries.extend(files);
        self.sidebar_revision = self.sidebar_revision.wrapping_add(1);
    }

    pub fn open_file_from_path(&mut self, path: PathBuf) -> Result<(), String> {
        let resolved = resolve_cli_path(&path);
        if let Some(existing_idx) = self
            .docs
            .iter()
            .position(|d| d.path.as_ref() == Some(&resolved))
        {
            self.active_doc = existing_idx;
            self.save_session();
            return Ok(());
        }

        let bytes = fs::read(&resolved).map_err(|e| format!("Failed to read file: {e}"))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let title = resolved
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();
        self.docs
            .push(Document::new_with_text(&title, Some(resolved), &text));
        self.active_doc = self.docs.len() - 1;
        self.save_session();
        Ok(())
    }

    pub fn new_tab(&mut self) {
        let next_num = self.docs.len() + 1;
        let title = format!("untitled_{next_num}.rs");
        self.docs.push(Document::new_with_text(&title, None, ""));
        self.active_doc = self.docs.len() - 1;
        self.save_session();
    }

    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.docs.len() {
            return;
        }
        if self.docs.len() == 1 {
            self.docs[0] = Document::new_with_text("untitled.rs", None, "");
            self.active_doc = 0;
        } else {
            self.docs.remove(idx);
            if self.active_doc >= self.docs.len() {
                self.active_doc = self.docs.len() - 1;
            }
        }
        self.save_session();
    }

    pub fn close_active_tab(&mut self) {
        self.close_tab(self.active_doc);
    }

    pub fn zoom_in(&mut self) {
        self.font_size = (self.font_size + 1).min(36);
        self.save_session();
    }

    pub fn zoom_out(&mut self) {
        self.font_size = (self.font_size - 1).max(9);
        self.save_session();
    }

    pub fn zoom_reset(&mut self) {
        self.font_size = 15;
        self.save_session();
    }

    pub fn toggle_vim_mode(&mut self) {
        self.vim_mode = if self.vim_mode == VimMode::Off {
            VimMode::Normal
        } else {
            VimMode::Off
        };
        self.vim_pending_key = None;
        self.save_session();
    }

    pub fn visible_rows(&self) -> usize {
        ((self.last_canvas_h / self.line_height.max(12.0)).floor() as usize).max(1)
    }

    pub fn visible_cols(&self) -> usize {
        let gutter = 72.0;
        let right = if self.show_minimap { 86.0 } else { 14.0 };
        let usable = (self.last_canvas_w - gutter - right).max(80.0);
        ((usable / self.char_width.max(6.0)).floor() as usize).max(10)
    }

    pub fn ensure_cursor_visible(&mut self) {
        let rows = self.visible_rows();
        let cols = self.visible_cols();
        self.doc_mut().ensure_cursor_visible(rows, cols);
    }

    pub fn filtered_palette_entries(&self, query: &str) -> Vec<PaletteEntry> {
        let mut all = vec![
            PaletteEntry { label: "Format Document (rustfmt / auto)".into(), shortcut: "Ctrl+Shift+I", action: PaletteAction::FormatDocument },
            PaletteEntry { label: "Toggle Line Comment".into(), shortcut: "Ctrl+/", action: PaletteAction::ToggleComment },
            PaletteEntry { label: "Duplicate Line / Selection".into(), shortcut: "Ctrl+Shift+D", action: PaletteAction::DuplicateLine },
            PaletteEntry { label: "Move Line Up".into(), shortcut: "Alt+Up", action: PaletteAction::MoveLineUp },
            PaletteEntry { label: "Move Line Down".into(), shortcut: "Alt+Down", action: PaletteAction::MoveLineDown },
            PaletteEntry { label: "Add Cursor to Next Occurrence".into(), shortcut: "Ctrl+D", action: PaletteAction::AddCursorNextOccurrence },
            PaletteEntry { label: "Find in File".into(), shortcut: "Ctrl+F", action: PaletteAction::OpenFind },
            PaletteEntry { label: "Find and Replace".into(), shortcut: "Ctrl+H", action: PaletteAction::OpenReplace },
            PaletteEntry { label: "Go to Line...".into(), shortcut: "Ctrl+G", action: PaletteAction::GoToLine },
            PaletteEntry { label: "Toggle File Explorer Sidebar".into(), shortcut: "Ctrl+B", action: PaletteAction::ToggleSidebar },
            PaletteEntry { label: "Toggle Live Minimap".into(), shortcut: "Ctrl+M", action: PaletteAction::ToggleMinimap },
            PaletteEntry { label: "Toggle Vim Mode (Normal / Insert / Visual)".into(), shortcut: "", action: PaletteAction::ToggleVimMode },
            PaletteEntry { label: "Toggle Bookmark on Current Line".into(), shortcut: "F2", action: PaletteAction::ToggleBookmark },
            PaletteEntry { label: "Jump to Next Bookmark".into(), shortcut: "Shift+F2", action: PaletteAction::NextBookmark },
            PaletteEntry { label: "Toggle Code Fold at Cursor".into(), shortcut: "Ctrl+Shift+[", action: PaletteAction::ToggleFoldAtCursor },
            PaletteEntry { label: "Unfold All Code Blocks".into(), shortcut: "Ctrl+Shift+]", action: PaletteAction::UnfoldAll },
            PaletteEntry { label: "Toggle Sticky Scroll Headers".into(), shortcut: "", action: PaletteAction::ToggleStickyScroll },
            PaletteEntry { label: "Toggle Inline Hex Color Previews".into(), shortcut: "", action: PaletteAction::ToggleColorPreviews },
            PaletteEntry { label: "Toggle Rainbow Brackets".into(), shortcut: "", action: PaletteAction::ToggleRainbowBrackets },
            PaletteEntry { label: "Toggle Indent Guides".into(), shortcut: "", action: PaletteAction::ToggleIndentGuides },
            PaletteEntry { label: "Toggle Whitespace Markers".into(), shortcut: "", action: PaletteAction::ToggleWhitespace },
            PaletteEntry { label: "Toggle Word Wrap".into(), shortcut: "Alt+Z", action: PaletteAction::ToggleWordWrap },
            PaletteEntry { label: "Zoom In Font".into(), shortcut: "Ctrl++", action: PaletteAction::ZoomIn },
            PaletteEntry { label: "Zoom Out Font".into(), shortcut: "Ctrl+-", action: PaletteAction::ZoomOut },
            PaletteEntry { label: "Reset Zoom (15px)".into(), shortcut: "Ctrl+0", action: PaletteAction::ZoomReset },
            PaletteEntry { label: "Indentation: 4 Spaces".into(), shortcut: "", action: PaletteAction::IndentSpaces4 },
            PaletteEntry { label: "Indentation: 2 Spaces".into(), shortcut: "", action: PaletteAction::IndentSpaces2 },
            PaletteEntry { label: "Indentation: Tabs".into(), shortcut: "", action: PaletteAction::IndentTabs },
            PaletteEntry { label: "New Untitled Tab".into(), shortcut: "Ctrl+N", action: PaletteAction::NewTab },
            PaletteEntry { label: "Close Active Tab".into(), shortcut: "Ctrl+W", action: PaletteAction::CloseFile },
            PaletteEntry { label: "About Studio Code Editor".into(), shortcut: "", action: PaletteAction::AboutDeveloper },
        ];

        for (i, t) in EditorTheme::all().iter().enumerate() {
            all.push(PaletteEntry {
                label: format!("Theme: {}", t.name),
                shortcut: "",
                action: PaletteAction::SetTheme(i),
            });
        }

        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|e| e.label.to_lowercase().contains(&q))
            .collect()
    }

    pub fn cycle_indent_style(&mut self) {
        let next = match self.doc().indent_style {
            IndentStyle::Spaces(4) => IndentStyle::Spaces(2),
            IndentStyle::Spaces(2) => IndentStyle::Tabs,
            _ => IndentStyle::Spaces(4),
        };
        self.doc_mut().indent_style = next;
    }
}
