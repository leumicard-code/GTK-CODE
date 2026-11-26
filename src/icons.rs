// ============================================================================
// icons.rs — Embedded Google Material Symbols Sharp (wght 600 Bold) SVGs
// Zero runtime file dependencies; dynamically recolored for all 24 themes!
// ============================================================================

use gtk::gdk;
use gtk::glib;

use crate::themes::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialIcon {
    NoteAdd,
    FileOpen,
    Save,
    FolderOpen,
    Undo,
    Redo,
    Search,
    FindReplace,
    FormatSize,
    ZoomOut,
    ZoomIn,
    ViewSidebar,
    VerticalSplit,
    Terminal,
    Palette,
}

impl MaterialIcon {
    pub fn raw_svg(self) -> &'static str {
        match self {
            Self::NoteAdd => include_str!("../assets/icons/note_add.svg"),
            Self::FileOpen => include_str!("../assets/icons/file_open.svg"),
            Self::Save => include_str!("../assets/icons/save.svg"),
            Self::FolderOpen => include_str!("../assets/icons/folder_open.svg"),
            Self::Undo => include_str!("../assets/icons/undo.svg"),
            Self::Redo => include_str!("../assets/icons/redo.svg"),
            Self::Search => include_str!("../assets/icons/search.svg"),
            Self::FindReplace => include_str!("../assets/icons/find_replace.svg"),
            Self::FormatSize => include_str!("../assets/icons/format_size.svg"),
            Self::ZoomOut => include_str!("../assets/icons/zoom_out.svg"),
            Self::ZoomIn => include_str!("../assets/icons/zoom_in.svg"),
            Self::ViewSidebar => include_str!("../assets/icons/view_sidebar.svg"),
            Self::VerticalSplit => include_str!("../assets/icons/vertical_split.svg"),
            Self::Terminal => include_str!("../assets/icons/terminal.svg"),
            Self::Palette => include_str!("../assets/icons/palette.svg"),
        }
    }

    /// Renders a crisp 2x HiDPI-supersampled GPU texture in the exact active theme color.
    pub fn to_texture(self, color: Rgb) -> Option<gdk::Texture> {
        let hex = color.hex();
        let colored = self
            .raw_svg()
            .replace("height=\"24\"", "height=\"48\"")
            .replace("width=\"24\"", &format!("width=\"48\" fill=\"{hex}\""));
        gdk::Texture::from_bytes(&glib::Bytes::from_owned(colored.into_bytes())).ok()
    }
}
