// ============================================================================
// main.rs — Studio Code Editor (Pure GTK 4 + Pango/Cairo GPU-Accelerated IDE)
// ============================================================================

#[allow(dead_code)]
mod canvas;
#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod document;
#[allow(dead_code)]
mod icons;
#[allow(dead_code)]
mod state;
#[allow(dead_code)]
mod syntax;
#[allow(dead_code)]
mod themes;

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, DrawingArea, Entry,
    FileDialog, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType, PopoverMenu,
    PopoverMenuBar, ScrolledWindow, SelectionMode, Separator, Window,
};

use crate::canvas::{build_code_canvas, CanvasSignal};
use crate::config::ToolbarStyle;
use crate::document::{EditKind, IndentStyle};
use crate::icons::MaterialIcon;
use crate::state::{EditorState, PaletteAction, VimMode};
use crate::syntax::SyntaxEngine;
use crate::themes::{EditorTheme, Rgb};

const APP_ID: &str = "io.github.leumicard_code.GtkCodeEditor";

struct ToolbarItem {
    btn: Button,
    inner: GtkBox,
    icon_kind: MaterialIcon,
    icon: Image,
    label: Label,
}

impl ToolbarItem {
    fn new(icon_kind: MaterialIcon, text: &str, tooltip: &str) -> Self {
        let icon = Image::new();
        icon.set_pixel_size(20);
        let label = Label::new(Some(text));
        label.set_halign(Align::Center);
        label.set_valign(Align::Center);

        let inner = GtkBox::new(Orientation::Vertical, 2);
        inner.set_halign(Align::Center);
        inner.set_valign(Align::Center);
        inner.append(&icon);
        inner.append(&label);

        let btn = Button::new();
        btn.set_child(Some(&inner));
        btn.set_valign(Align::Center);
        btn.add_css_class("tool-btn");
        btn.set_tooltip_text(Some(tooltip));

        Self {
            btn,
            inner,
            icon_kind,
            icon,
            label,
        }
    }

    fn apply_style(&self, style: ToolbarStyle, color: Rgb) {
        if let Some(tex) = self.icon_kind.to_texture(color) {
            self.icon.set_paintable(Some(&tex));
        }
        self.label.remove_css_class("tool-label-under");
        self.label.remove_css_class("tool-label-beside");
        match style {
            ToolbarStyle::IconsOnly => {
                self.icon.set_pixel_size(20);
                self.label.set_visible(false);
                self.inner.set_orientation(Orientation::Horizontal);
                self.inner.set_spacing(0);
            }
            ToolbarStyle::TextBesideIcons => {
                self.icon.set_pixel_size(18);
                self.label.set_visible(true);
                self.label.add_css_class("tool-label-beside");
                self.inner.set_orientation(Orientation::Horizontal);
                self.inner.set_spacing(5);
            }
            ToolbarStyle::TextUnderIcons => {
                self.icon.set_pixel_size(20);
                self.label.set_visible(true);
                self.label.add_css_class("tool-label-under");
                self.inner.set_orientation(Orientation::Vertical);
                self.inner.set_spacing(2);
            }
        }
    }
}

struct UiHandles {
    window: ApplicationWindow,
    css_provider: CssProvider,
    canvas: DrawingArea,
    tab_box: GtkBox,
    sidebar_box: GtkBox,
    sidebar_header: Label,
    sidebar_list: ListBox,
    toolbar_items: Vec<ToolbarItem>,
    btn_save: Button,
    lbl_save: Label,
    btn_font: Button,
    lbl_font: Label,
    btn_zoom_reset: Button,
    btn_sidebar: Button,
    btn_minimap: Button,
    btn_vim: Button,
    find_bar: GtkBox,
    find_entry: Entry,
    replace_row: GtkBox,
    find_count_lbl: Label,
    goto_bar: GtkBox,
    goto_entry: Entry,
    status_vim: Label,
    status_pos: Label,
    status_lang: Label,
    status_indent: Button,
    status_theme: Button,
    status_zoom: Label,
}

fn apply_theme_css(provider: &CssProvider, st: &EditorState) {
    provider.load_from_string(&st.theme().to_gtk_css());
}

fn set_toggle_class(btn: &Button, active: bool) {
    btn.remove_css_class("tool-btn");
    btn.remove_css_class("tool-btn-active");
    btn.add_css_class(if active {
        "tool-btn-active"
    } else {
        "tool-btn"
    });
}

fn show_about_dialog(parent: &ApplicationWindow) {
    let dlg = Window::builder()
        .title("About Studio Code Editor — GTK 4")
        .transient_for(parent)
        .modal(true)
        .default_width(460)
        .resizable(false)
        .build();
    dlg.add_css_class("studio-window");

    let root = GtkBox::new(Orientation::Vertical, 10);
    root.set_margin_top(20);
    root.set_margin_bottom(18);
    root.set_margin_start(22);
    root.set_margin_end(22);

    let title = Label::new(Some("Studio Code Editor — GTK 4 + Pango/Cairo"));
    title.set_halign(Align::Start);
    title.set_markup("<span size='x-large' weight='bold'>Studio Code Editor</span>");

    let sub = Label::new(Some(
        "Pure Rust + GTK 4 + Pango/Cairo GPU-Accelerated IDE\n\
         Author: Shay Kadosh\n\n\
         • 24 Built-in Studio Color Themes\n\
         • True Ligatures & Hebrew (עברית RTL / BiDi) via Pango\n\
         • Multi-Cursor (Ctrl+Click, Ctrl+D, Alt+Shift+Up/Down)\n\
         • Code Folding, Sticky Scroll, Inline #RRGGBB Color Previews\n\
         • Live Minimap, Autocomplete, Command Palette & Vim Mode",
    ));
    sub.set_halign(Align::Start);
    sub.set_wrap(true);

    let close_btn = Button::with_label("Close");
    close_btn.add_css_class("tool-btn-accent");
    close_btn.set_halign(Align::End);
    {
        let dlg = dlg.clone();
        close_btn.connect_clicked(move |_| dlg.close());
    }

    root.append(&title);
    root.append(&sub);
    root.append(&close_btn);
    dlg.set_child(Some(&root));
    dlg.present();
}

fn show_font_picker(
    parent: &ApplicationWindow,
    state: Rc<RefCell<EditorState>>,
    refresh_all: Rc<dyn Fn()>,
) {
    let dlg = Window::builder()
        .title("Choose Editor Font & Weight")
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .default_height(600)
        .build();
    dlg.add_css_class("studio-window");

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(14);
    root.set_margin_end(14);

    let search = Entry::new();
    search.set_placeholder_text(Some(
        "Filter fonts or weights (Cascadia, Segoe, SemiBold, Bold, JetBrains)...",
    ));
    search.add_css_class("studio-entry");

    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::Single);
    list.add_css_class("palette-list");

    // Discover ALL installed Pango font faces (Family + Weight/Style: Regular, SemiBold, Bold, Italic...)
    #[derive(Clone)]
    struct FontFaceItem {
        display: String,
        family: String,
        pango_desc: String,
        style_tag: String,
        is_mono: bool,
    }

    let mut items: Vec<FontFaceItem> = Vec::new();
    let fm = pangocairo::FontMap::default();
    for fam in fm.list_families() {
        let fam_name = fam.name().to_string();
        let low = fam_name.to_lowercase();
        let is_mono = fam.is_monospace()
            || low.contains("mono")
            || low.contains("code")
            || low.contains("cascadia")
            || low.contains("fira")
            || low.contains("consolas")
            || low.contains("hack");

        let faces = fam.list_faces();
        if faces.is_empty() {
            items.push(FontFaceItem {
                display: fam_name.clone(),
                family: fam_name.clone(),
                pango_desc: fam_name,
                style_tag: "Regular".to_string(),
                is_mono,
            });
        } else {
            for face in faces {
                let face_name = face.face_name().to_string();
                let desc_str = face.describe().to_str().to_string();
                let display = format!("{fam_name} — {face_name}");
                items.push(FontFaceItem {
                    display,
                    family: fam_name.clone(),
                    pango_desc: desc_str,
                    style_tag: face_name,
                    is_mono,
                });
            }
        }
    }
    items.sort_by(|a, b| {
        b.is_mono
            .cmp(&a.is_mono)
            .then_with(|| a.family.to_lowercase().cmp(&b.family.to_lowercase()))
            .then_with(|| a.style_tag.to_lowercase().cmp(&b.style_tag.to_lowercase()))
    });
    items.dedup_by(|a, b| a.pango_desc.eq_ignore_ascii_case(&b.pango_desc));

    let items = Rc::new(items);
    let visible_items: Rc<RefCell<Vec<FontFaceItem>>> = Rc::new(RefCell::new(Vec::new()));
    let selected_spec: Rc<RefCell<String>> =
        Rc::new(RefCell::new(state.borrow().font_name.clone()));

    // Quick Weight Override Bar (Regular / Medium / SemiBold / Bold)
    let weight_row = GtkBox::new(Orientation::Horizontal, 6);
    let weight_lbl = Label::new(Some("Weight:"));
    weight_lbl.add_css_class("muted-label");
    weight_row.append(&weight_lbl);

    // Live Preview Box showing the selected font + weight in real time
    let preview_lbl = Label::new(None);
    preview_lbl.set_halign(Align::Start);
    preview_lbl.set_margin_top(6);
    preview_lbl.set_margin_bottom(6);
    preview_lbl.set_margin_start(10);
    preview_lbl.set_margin_end(10);
    preview_lbl.set_wrap(true);

    let preview_box = GtkBox::new(Orientation::Vertical, 2);
    preview_box.add_css_class("overlay-bar");
    let preview_title = Label::new(Some("Live Preview:"));
    preview_title.set_halign(Align::Start);
    preview_title.add_css_class("palette-shortcut");
    preview_box.append(&preview_title);
    preview_box.append(&preview_lbl);

    let update_preview: Rc<dyn Fn(&str)> = {
        let preview_lbl = preview_lbl.clone();
        let preview_title = preview_title.clone();
        let font_sz = state.borrow().font_size;
        Rc::new(move |spec: &str| {
            preview_title.set_text(&format!("Live Preview — {spec}"));
            let mut fd = pango::FontDescription::from_string(spec);
            fd.set_absolute_size((font_sz.clamp(12, 22) as f64) * (pango::SCALE as f64));
            let attrs = pango::AttrList::new();
            attrs.insert(pango::AttrFontDesc::new(&fd));
            preview_lbl.set_attributes(Some(&attrs));
            preview_lbl.set_text(
                "fn main() -> bool { let ok = x != 0 && y >= 1; // שלום עולם 123 }",
            );
        })
    };

    update_preview(&selected_spec.borrow());

    for (btn_label, weight_suffix) in [
        ("Regular", ""),
        ("Medium", "Medium"),
        ("SemiBold", "SemiBold"),
        ("Bold", "Bold"),
    ] {
        let wb = Button::with_label(btn_label);
        wb.add_css_class("tool-btn");
        let selected_spec = Rc::clone(&selected_spec);
        let update_preview = Rc::clone(&update_preview);
        wb.connect_clicked(move |_| {
            let cur = selected_spec.borrow().clone();
            let fd = pango::FontDescription::from_string(&cur);
            let fam = fd
                .family()
                .map(|s| s.to_string())
                .unwrap_or_else(|| cur.clone());
            let new_spec = if weight_suffix.is_empty() {
                fam
            } else {
                format!("{fam} {weight_suffix}")
            };
            *selected_spec.borrow_mut() = new_spec.clone();
            update_preview(&new_spec);
        });
        weight_row.append(&wb);
    }

    let rebuild = {
        let list = list.clone();
        let items = Rc::clone(&items);
        let visible_items = Rc::clone(&visible_items);
        let cur_font = state.borrow().font_name.clone();
        Rc::new(move |q: &str| {
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            let mut vis = visible_items.borrow_mut();
            vis.clear();
            let ql = q.trim().to_lowercase();
            for item in items.iter() {
                if !ql.is_empty()
                    && !item.display.to_lowercase().contains(&ql)
                    && !item.pango_desc.to_lowercase().contains(&ql)
                {
                    continue;
                }
                vis.push(item.clone());
                let row_box = GtkBox::new(Orientation::Horizontal, 8);
                let lbl = Label::new(Some(&item.display));
                lbl.set_halign(Align::Start);
                lbl.set_hexpand(true);
                let tag = Label::new(Some(if item.is_mono { "MONO" } else { "UI" }));
                tag.add_css_class(if item.is_mono {
                    "palette-shortcut"
                } else {
                    "muted-label"
                });
                row_box.append(&lbl);
                row_box.append(&tag);
                let row = ListBoxRow::new();
                row.set_child(Some(&row_box));
                list.append(&row);
                if item.pango_desc.eq_ignore_ascii_case(&cur_font) {
                    list.select_row(Some(&row));
                }
            }
        })
    };

    rebuild("");

    {
        let rebuild = Rc::clone(&rebuild);
        search.connect_changed(move |e| rebuild(&e.text()));
    }

    // Single-click updates selected font + live preview
    {
        let visible_items = Rc::clone(&visible_items);
        let selected_spec = Rc::clone(&selected_spec);
        let update_preview = Rc::clone(&update_preview);
        list.connect_row_selected(move |_, row_opt| {
            if let Some(row) = row_opt {
                let idx = row.index() as usize;
                if let Some(item) = visible_items.borrow().get(idx) {
                    *selected_spec.borrow_mut() = item.pango_desc.clone();
                    update_preview(&item.pango_desc);
                }
            }
        });
    }

    let apply_and_close: Rc<dyn Fn()> = {
        let state = Rc::clone(&state);
        let selected_spec = Rc::clone(&selected_spec);
        let refresh = Rc::clone(&refresh_all);
        let dlg = dlg.clone();
        Rc::new(move || {
            let spec = selected_spec.borrow().clone();
            if !spec.trim().is_empty() {
                let mut st = state.borrow_mut();
                st.font_name = spec;
                st.save_session();
            }
            refresh();
            dlg.close();
        })
    };

    // Double-click / Enter applies immediately
    {
        let visible_items = Rc::clone(&visible_items);
        let selected_spec = Rc::clone(&selected_spec);
        let apply = Rc::clone(&apply_and_close);
        list.connect_row_activated(move |_, row| {
            let idx = row.index() as usize;
            if let Some(item) = visible_items.borrow().get(idx) {
                *selected_spec.borrow_mut() = item.pango_desc.clone();
                apply();
            }
        });
    }

    // Esc closes dialog
    {
        let kc = gtk::EventControllerKey::new();
        let dlg_esc = dlg.clone();
        kc.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                dlg_esc.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        dlg.add_controller(kc);
    }

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();

    let btn_row = GtkBox::new(Orientation::Horizontal, 8);
    btn_row.set_halign(Align::End);
    let btn_cancel = Button::with_label("Cancel");
    btn_cancel.add_css_class("tool-btn");
    {
        let dlg = dlg.clone();
        btn_cancel.connect_clicked(move |_| dlg.close());
    }
    let btn_apply = Button::with_label("Apply Font");
    btn_apply.add_css_class("tool-btn-accent");
    {
        let apply = Rc::clone(&apply_and_close);
        btn_apply.connect_clicked(move |_| apply());
    }
    btn_row.append(&btn_cancel);
    btn_row.append(&btn_apply);

    root.append(&search);
    root.append(&scroll);
    root.append(&weight_row);
    root.append(&preview_box);
    root.append(&btn_row);
    dlg.set_child(Some(&root));
    dlg.present();
    search.grab_focus();
}

/// Shows an Unsaved Changes warning dialog when closing a dirty tab.
fn request_close_tab(
    parent: &ApplicationWindow,
    state: Rc<RefCell<EditorState>>,
    tab_idx: usize,
    refresh_all: Rc<dyn Fn()>,
) {
    let (is_dirty, title, has_path) = {
        let st = state.borrow();
        if tab_idx >= st.docs.len() {
            return;
        }
        let d = &st.docs[tab_idx];
        (d.dirty, d.title.clone(), d.path.is_some())
    };

    if !is_dirty {
        state.borrow_mut().close_tab(tab_idx);
        refresh_all();
        return;
    }

    let dlg = Window::builder()
        .title("Unsaved Changes")
        .transient_for(parent)
        .modal(true)
        .default_width(430)
        .resizable(false)
        .build();
    dlg.add_css_class("studio-window");

    let root = GtkBox::new(Orientation::Vertical, 14);
    root.set_margin_top(18);
    root.set_margin_bottom(16);
    root.set_margin_start(20);
    root.set_margin_end(20);

    let msg = Label::new(Some(&format!(
        "\"{title}\" has unsaved changes.\nDo you want to save before closing this tab?"
    )));
    msg.set_halign(Align::Start);
    msg.set_wrap(true);

    let btns = GtkBox::new(Orientation::Horizontal, 8);
    btns.set_halign(Align::End);

    let btn_cancel = Button::with_label("Cancel");
    btn_cancel.add_css_class("tool-btn");
    {
        let dlg = dlg.clone();
        btn_cancel.connect_clicked(move |_| dlg.close());
    }

    let btn_discard = Button::with_label("Discard Changes");
    btn_discard.add_css_class("tool-btn");
    {
        let dlg = dlg.clone();
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        btn_discard.connect_clicked(move |_| {
            dlg.close();
            state.borrow_mut().close_tab(tab_idx);
            refresh();
        });
    }

    let btn_save = Button::with_label("Save & Close");
    btn_save.add_css_class("tool-btn-accent");
    {
        let dlg = dlg.clone();
        let parent = parent.clone();
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        btn_save.connect_clicked(move |_| {
            dlg.close();
            if has_path {
                let (path_opt, content) = {
                    let st = state.borrow();
                    if tab_idx < st.docs.len() {
                        (st.docs[tab_idx].path.clone(), st.docs[tab_idx].text())
                    } else {
                        (None, String::new())
                    }
                };
                if let Some(p) = path_opt {
                    if fs::write(&p, content).is_ok() {
                        state.borrow_mut().close_tab(tab_idx);
                        refresh();
                    }
                }
            } else {
                let init_name = title.clone();
                let fdlg = FileDialog::builder()
                    .title("Save File Before Closing")
                    .initial_name(&init_name)
                    .build();
                let st_cl = Rc::clone(&state);
                let rf_cl = Rc::clone(&refresh);
                fdlg.save(Some(&parent), gio::Cancellable::NONE, move |res| {
                    if let Ok(file) = res {
                        if let Some(p) = file.path() {
                            let content = {
                                let st = st_cl.borrow();
                                st.docs.get(tab_idx).map(|d| d.text()).unwrap_or_default()
                            };
                            if fs::write(&p, content).is_ok() {
                                st_cl.borrow_mut().close_tab(tab_idx);
                                rf_cl();
                            }
                        }
                    }
                });
            }
        });
    }

    btns.append(&btn_cancel);
    btns.append(&btn_discard);
    btns.append(&btn_save);
    root.append(&msg);
    root.append(&btns);
    dlg.set_child(Some(&root));
    dlg.present();
}

/// Shows an Unsaved Changes warning dialog when closing the entire window / app.
fn request_close_app(
    parent: &ApplicationWindow,
    state: Rc<RefCell<EditorState>>,
    force_close: Rc<Cell<bool>>,
    refresh_all: Rc<dyn Fn()>,
) {
    let dirty_titles: Vec<String> = {
        let st = state.borrow();
        st.docs
            .iter()
            .filter(|d| d.dirty)
            .map(|d| format!("  • {}", d.title))
            .collect()
    };

    if dirty_titles.is_empty() {
        state.borrow().save_session();
        force_close.set(true);
        parent.close();
        return;
    }

    let dlg = Window::builder()
        .title("Unsaved Changes — Confirm Exit")
        .transient_for(parent)
        .modal(true)
        .default_width(460)
        .resizable(false)
        .build();
    dlg.add_css_class("studio-window");

    let root = GtkBox::new(Orientation::Vertical, 14);
    root.set_margin_top(18);
    root.set_margin_bottom(16);
    root.set_margin_start(20);
    root.set_margin_end(20);

    let msg = Label::new(Some(&format!(
        "You have {} unsaved document(s):\n\n{}\n\nIf you close without saving, your changes will be lost.",
        dirty_titles.len(),
        dirty_titles.join("\n")
    )));
    msg.set_halign(Align::Start);
    msg.set_wrap(true);

    let btns = GtkBox::new(Orientation::Horizontal, 8);
    btns.set_halign(Align::End);

    let btn_cancel = Button::with_label("Cancel");
    btn_cancel.add_css_class("tool-btn");
    {
        let dlg = dlg.clone();
        btn_cancel.connect_clicked(move |_| dlg.close());
    }

    let btn_discard = Button::with_label("Discard & Exit");
    btn_discard.add_css_class("tool-btn");
    {
        let dlg = dlg.clone();
        let parent = parent.clone();
        let state = Rc::clone(&state);
        let force_close = Rc::clone(&force_close);
        btn_discard.connect_clicked(move |_| {
            dlg.close();
            state.borrow().save_session();
            force_close.set(true);
            parent.close();
        });
    }

    let btn_save_all = Button::with_label("Save All & Exit");
    btn_save_all.add_css_class("tool-btn-accent");
    {
        let dlg = dlg.clone();
        let parent = parent.clone();
        let state = Rc::clone(&state);
        let force_close = Rc::clone(&force_close);
        let refresh = Rc::clone(&refresh_all);
        btn_save_all.connect_clicked(move |_| {
            dlg.close();
            // Save all dirty documents that already have a file path
            let mut first_untitled: Option<usize> = None;
            {
                let mut st = state.borrow_mut();
                for (i, d) in st.docs.iter_mut().enumerate() {
                    if d.dirty {
                        if let Some(ref p) = d.path {
                            if fs::write(p, d.text()).is_ok() {
                                d.dirty = false;
                            }
                        } else if first_untitled.is_none() {
                            first_untitled = Some(i);
                        }
                    }
                }
            }

            if let Some(u_idx) = first_untitled {
                // Prompt Save As for untitled dirty file so nothing is lost!
                state.borrow_mut().active_doc = u_idx;
                refresh();
                let init_name = state.borrow().docs[u_idx].title.clone();
                let fdlg = FileDialog::builder()
                    .title("Save Untitled File Before Exiting")
                    .initial_name(&init_name)
                    .build();
                let st_cl = Rc::clone(&state);
                let rf_cl = Rc::clone(&refresh);
                let p_cl = parent.clone();
                let fc_cl = Rc::clone(&force_close);
                fdlg.save(Some(&parent), gio::Cancellable::NONE, move |res| {
                    if let Ok(file) = res {
                        if let Some(p) = file.path() {
                            save_document_to_path(&st_cl, p);
                            rf_cl();
                            // Re-check if any other untitled dirty files remain
                            request_close_app(&p_cl, st_cl, fc_cl, rf_cl);
                        }
                    }
                });
            } else {
                state.borrow().save_session();
                force_close.set(true);
                parent.close();
            }
        });
    }

    btns.append(&btn_cancel);
    btns.append(&btn_discard);
    btns.append(&btn_save_all);
    root.append(&msg);
    root.append(&btns);
    dlg.set_child(Some(&root));
    dlg.present();
}

fn show_command_palette(
    parent: &ApplicationWindow,
    state: Rc<RefCell<EditorState>>,
    dispatch_action: Rc<dyn Fn(PaletteAction)>,
) {
    let dlg = Window::builder()
        .title("Command Palette (Ctrl+Shift+P)")
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .default_height(460)
        .build();
    dlg.add_css_class("studio-window");

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(14);
    root.set_margin_end(14);

    let search = Entry::new();
    search.set_placeholder_text(Some("Type a command or theme name..."));
    search.add_css_class("studio-entry");

    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::Single);
    list.add_css_class("palette-list");

    let current_actions: Rc<RefCell<Vec<PaletteAction>>> = Rc::new(RefCell::new(Vec::new()));

    let rebuild = {
        let state = Rc::clone(&state);
        let list = list.clone();
        let current_actions = Rc::clone(&current_actions);
        Rc::new(move |q: &str| {
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            let entries = state.borrow().filtered_palette_entries(q);
            let mut acts = current_actions.borrow_mut();
            acts.clear();
            for (idx, e) in entries.into_iter().enumerate() {
                acts.push(e.action);
                let row_box = GtkBox::new(Orientation::Horizontal, 10);
                let lbl = Label::new(Some(&e.label));
                lbl.set_halign(Align::Start);
                lbl.set_hexpand(true);
                row_box.append(&lbl);
                if !e.shortcut.is_empty() {
                    let sc = Label::new(Some(e.shortcut));
                    sc.add_css_class("palette-shortcut");
                    row_box.append(&sc);
                }
                let row = ListBoxRow::new();
                row.set_child(Some(&row_box));
                list.append(&row);
                if idx == 0 {
                    list.select_row(Some(&row));
                }
            }
        })
    };

    rebuild("");

    {
        let rebuild = Rc::clone(&rebuild);
        search.connect_changed(move |e| rebuild(&e.text()));
    }

    {
        let current_actions = Rc::clone(&current_actions);
        let dispatch = Rc::clone(&dispatch_action);
        let dlg = dlg.clone();
        let list_ref = list.clone();
        search.connect_activate(move |_| {
            let idx = list_ref
                .selected_row()
                .map(|r| r.index() as usize)
                .unwrap_or(0);
            if let Some(&act) = current_actions.borrow().get(idx) {
                dlg.close();
                dispatch(act);
            }
        });
    }

    {
        let current_actions = Rc::clone(&current_actions);
        let dispatch = Rc::clone(&dispatch_action);
        let dlg = dlg.clone();
        list.connect_row_activated(move |_, row| {
            let idx = row.index() as usize;
            if let Some(&act) = current_actions.borrow().get(idx) {
                dlg.close();
                dispatch(act);
            }
        });
    }

    // Esc closes command palette
    {
        let kc = gtk::EventControllerKey::new();
        let dlg_esc = dlg.clone();
        kc.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                dlg_esc.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        dlg.add_controller(kc);
    }

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();

    root.append(&search);
    root.append(&scroll);
    dlg.set_child(Some(&root));
    dlg.present();
    search.grab_focus();
}

fn save_document_to_path(state: &Rc<RefCell<EditorState>>, path: PathBuf) {
    let content = state.borrow().doc().text();
    if fs::write(&path, content).is_ok() {
        let mut st = state.borrow_mut();
        let doc = st.doc_mut();
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            doc.title = name.to_string();
        }
        doc.path = Some(path);
        doc.dirty = false;
        st.save_session();
    }
}

fn open_file_dialog(
    parent: &ApplicationWindow,
    state: Rc<RefCell<EditorState>>,
    refresh: Rc<dyn Fn()>,
) {
    let dialog = FileDialog::builder().title("Open File").build();
    dialog.open(Some(parent), gio::Cancellable::NONE, move |res| {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                let _ = state.borrow_mut().open_file_from_path(path);
                refresh();
            }
        }
    });
}

fn open_folder_dialog(
    parent: &ApplicationWindow,
    state: Rc<RefCell<EditorState>>,
    refresh: Rc<dyn Fn()>,
) {
    let dialog = FileDialog::builder()
        .title("Open Workspace Folder")
        .build();
    dialog.select_folder(Some(parent), gio::Cancellable::NONE, move |res| {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                if path.is_dir() {
                    let mut st = state.borrow_mut();
                    st.workspace_dir = path;
                    st.show_sidebar = true;
                    st.refresh_sidebar();
                    st.save_session();
                    drop(st);
                    refresh();
                }
            }
        }
    });
}

fn save_file_as_dialog(
    parent: &ApplicationWindow,
    state: Rc<RefCell<EditorState>>,
    refresh: Rc<dyn Fn()>,
) {
    let initial_name = state.borrow().doc().title.clone();
    let dialog = FileDialog::builder()
        .title("Save File As")
        .initial_name(&initial_name)
        .build();
    dialog.save(Some(parent), gio::Cancellable::NONE, move |res| {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                save_document_to_path(&state, path);
                refresh();
            }
        }
    });
}

fn save_file_or_prompt(
    parent: &ApplicationWindow,
    state: Rc<RefCell<EditorState>>,
    refresh: Rc<dyn Fn()>,
) {
    let existing = state.borrow().doc().path.clone();
    if let Some(p) = existing {
        save_document_to_path(&state, p);
        refresh();
    } else {
        save_file_as_dialog(parent, state, refresh);
    }
}

fn build_menu_bar(app: &Application) -> PopoverMenuBar {
    let root = gio::Menu::new();

    // File menu
    let file_menu = gio::Menu::new();
    file_menu.append(Some("New Tab (Ctrl+N)"), Some("app.new_tab"));
    file_menu.append(Some("Open File... (Ctrl+O)"), Some("app.open_file"));
    file_menu.append(Some("Open Folder..."), Some("app.open_folder"));
    file_menu.append(Some("Save (Ctrl+S)"), Some("app.save_file"));
    file_menu.append(Some("Save As... (Ctrl+Shift+S)"), Some("app.save_as"));
    file_menu.append(Some("Format Code (Ctrl+Shift+I)"), Some("app.format_code"));
    file_menu.append(Some("Close Tab (Ctrl+W)"), Some("app.close_tab"));
    file_menu.append(Some("Exit (Ctrl+Q)"), Some("app.quit_app"));
    root.append_submenu(Some("File"), &file_menu);

    // Edit menu
    let edit_menu = gio::Menu::new();
    edit_menu.append(Some("Undo (Ctrl+Z)"), Some("app.undo"));
    edit_menu.append(Some("Redo (Ctrl+Y)"), Some("app.redo"));
    edit_menu.append(Some("Cut (Ctrl+X)"), Some("app.cut"));
    edit_menu.append(Some("Copy (Ctrl+C)"), Some("app.copy"));
    edit_menu.append(Some("Paste (Ctrl+V)"), Some("app.paste"));
    edit_menu.append(Some("Select All (Ctrl+A)"), Some("app.select_all"));
    edit_menu.append(Some("Find (Ctrl+F)"), Some("app.open_find"));
    edit_menu.append(Some("Find & Replace (Ctrl+H)"), Some("app.open_replace"));
    edit_menu.append(Some("Go to Line... (Ctrl+G)"), Some("app.open_goto"));
    edit_menu.append(Some("Toggle Comment (Ctrl+/)"), Some("app.toggle_comment"));
    edit_menu.append(Some("Duplicate Line (Ctrl+Shift+D)"), Some("app.duplicate_line"));
    edit_menu.append(Some("Move Line Up (Alt+Up)"), Some("app.move_up"));
    edit_menu.append(Some("Move Line Down (Alt+Down)"), Some("app.move_down"));
    edit_menu.append(Some("Add Cursor Next Match (Ctrl+D)"), Some("app.add_cursor"));
    edit_menu.append(Some("Command Palette (Ctrl+Shift+P)"), Some("app.palette"));
    root.append_submenu(Some("Edit"), &edit_menu);

    // View menu
    let view_menu = gio::Menu::new();
    view_menu.append(Some("Toggle Sidebar (Ctrl+B)"), Some("app.toggle_sidebar"));
    view_menu.append(Some("Toggle Minimap (Ctrl+M)"), Some("app.toggle_minimap"));
    view_menu.append(Some("Toggle Sticky Scroll"), Some("app.toggle_sticky"));
    view_menu.append(Some("Toggle Inline Color Previews"), Some("app.toggle_colors"));
    view_menu.append(Some("Toggle Rainbow Brackets"), Some("app.toggle_rainbow"));
    view_menu.append(Some("Toggle Indent Guides"), Some("app.toggle_guides"));
    view_menu.append(Some("Toggle Whitespace Markers"), Some("app.toggle_ws"));
    view_menu.append(Some("Toggle Vim Mode"), Some("app.toggle_vim"));
    view_menu.append(Some("Toggle Fold at Cursor"), Some("app.toggle_fold"));
    view_menu.append(Some("Unfold All"), Some("app.unfold_all"));
    view_menu.append(Some("Choose Font..."), Some("app.font_picker"));
    view_menu.append(Some("Zoom In (Ctrl++)"), Some("app.zoom_in"));
    view_menu.append(Some("Zoom Out (Ctrl+-)"), Some("app.zoom_out"));
    view_menu.append(Some("Reset Zoom (Ctrl+0)"), Some("app.zoom_reset"));

    // KDE-style Toolbar Style submenu inside View
    let tb_style_menu = gio::Menu::new();
    tb_style_menu.append(Some("Icons Only"), Some("app.tb_icons_only"));
    tb_style_menu.append(Some("Text Alongside Icons"), Some("app.tb_text_beside"));
    tb_style_menu.append(
        Some("Text Under Icons (KDE Style)"),
        Some("app.tb_text_under"),
    );
    view_menu.append_submenu(Some("Toolbar Style"), &tb_style_menu);

    root.append_submenu(Some("View"), &view_menu);

    // Themes menu (all 24 Studio Themes!)
    let themes_menu = gio::Menu::new();
    for (idx, t) in EditorTheme::all().iter().enumerate() {
        let act = format!("app.theme_{idx}");
        themes_menu.append(Some(t.name), Some(&act));
    }
    root.append_submenu(Some("Themes (24)"), &themes_menu);

    // Help menu
    let help_menu = gio::Menu::new();
    help_menu.append(Some("About Studio Code Editor"), Some("app.about"));
    root.append_submenu(Some("Help"), &help_menu);

    let _ = app;
    PopoverMenuBar::from_model(Some(&root))
}

fn add_simple_action(app: &Application, name: &str, cb: impl Fn() + 'static) {
    let act = gio::SimpleAction::new(name, None);
    act.connect_activate(move |_, _| cb());
    app.add_action(&act);
}

fn build_ui(app: &Application, cli_args: &[PathBuf]) {
    let state = Rc::new(RefCell::new(EditorState::new_with_args(cli_args)));

    let css_provider = CssProvider::new();
    apply_theme_css(&css_provider, &state.borrow());
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Studio Code Editor — GTK 4")
        .default_width(1280)
        .default_height(820)
        .build();
    window.add_css_class("studio-window");

    let root = GtkBox::new(Orientation::Vertical, 0);

    // 1. Menu Bar
    let menubar = build_menu_bar(app);
    root.append(&menubar);

    // 2. KDE-Style Studio Toolbar (Symbolic Icons + Icons Only / Text Beside / Text Under)
    let toolbar = GtkBox::new(Orientation::Horizontal, 4);
    toolbar.set_valign(Align::Center);
    toolbar.add_css_class("studio-toolbar");

    // Right-click menu directly on the toolbar (just like KDE Plasma / Kate!)
    let tb_ctx_menu = gio::Menu::new();
    tb_ctx_menu.append(Some("Icons Only"), Some("app.tb_icons_only"));
    tb_ctx_menu.append(Some("Text Alongside Icons"), Some("app.tb_text_beside"));
    tb_ctx_menu.append(
        Some("Text Under Icons (KDE Style)"),
        Some("app.tb_text_under"),
    );
    let tb_popover = PopoverMenu::from_model(Some(&tb_ctx_menu));
    tb_popover.set_parent(&toolbar);
    tb_popover.set_has_arrow(false);
    {
        let pop = tb_popover.clone();
        toolbar.connect_destroy(move |_| pop.unparent());
    }
    {
        let rc = gtk::GestureClick::new();
        rc.set_button(3);
        let pop = tb_popover.clone();
        rc.connect_pressed(move |_, _, x, y| {
            pop.set_pointing_to(Some(&gdk::Rectangle::new(
                x.round() as i32,
                y.round() as i32,
                1,
                1,
            )));
            pop.popup();
        });
        toolbar.add_controller(rc);
    }

    let make_btn = |label: &str, tooltip: &str| {
        let b = Button::with_label(label);
        b.set_valign(Align::Center);
        b.add_css_class("tool-btn");
        b.set_tooltip_text(Some(tooltip));
        b
    };

    let ti_new = ToolbarItem::new(MaterialIcon::NoteAdd, "New", "New Untitled Tab (Ctrl+N)");
    let ti_open = ToolbarItem::new(MaterialIcon::FileOpen, "Open", "Open File (Ctrl+O)");
    let ti_save = ToolbarItem::new(MaterialIcon::Save, "Save", "Save Active File (Ctrl+S)");
    let ti_folder = ToolbarItem::new(
        MaterialIcon::FolderOpen,
        "Folder",
        "Open Workspace Folder",
    );
    let ti_undo = ToolbarItem::new(MaterialIcon::Undo, "Undo", "Undo (Ctrl+Z)");
    let ti_redo = ToolbarItem::new(MaterialIcon::Redo, "Redo", "Redo (Ctrl+Y)");
    let ti_find = ToolbarItem::new(MaterialIcon::Search, "Find", "Find in File (Ctrl+F)");
    let ti_replace = ToolbarItem::new(
        MaterialIcon::FindReplace,
        "Replace",
        "Find & Replace (Ctrl+H)",
    );
    let ti_font = ToolbarItem::new(
        MaterialIcon::FormatSize,
        "Font",
        "Choose Coding Font & Weight",
    );
    let ti_zoom_out = ToolbarItem::new(MaterialIcon::ZoomOut, "Zoom −", "Zoom Out (Ctrl+-)");
    let btn_zoom_reset = make_btn("15px", "Reset Font Size (Ctrl+0)");
    let ti_zoom_in = ToolbarItem::new(MaterialIcon::ZoomIn, "Zoom +", "Zoom In (Ctrl++)");
    let ti_sidebar = ToolbarItem::new(
        MaterialIcon::ViewSidebar,
        "Sidebar",
        "Toggle File Explorer (Ctrl+B)",
    );
    let ti_minimap = ToolbarItem::new(
        MaterialIcon::VerticalSplit,
        "Minimap",
        "Toggle Live Minimap (Ctrl+M)",
    );
    let ti_vim = ToolbarItem::new(
        MaterialIcon::Terminal,
        "Vim",
        "Toggle Vim Mode",
    );
    let ti_theme = ToolbarItem::new(
        MaterialIcon::Palette,
        "Theme",
        "Cycle 24 Studio Themes",
    );

    let btn_new = ti_new.btn.clone();
    let btn_open = ti_open.btn.clone();
    let btn_save = ti_save.btn.clone();
    let lbl_save = ti_save.label.clone();
    let btn_folder = ti_folder.btn.clone();
    let btn_undo = ti_undo.btn.clone();
    let btn_redo = ti_redo.btn.clone();
    let btn_find = ti_find.btn.clone();
    let btn_replace = ti_replace.btn.clone();
    let btn_font = ti_font.btn.clone();
    let lbl_font = ti_font.label.clone();
    let btn_zoom_out = ti_zoom_out.btn.clone();
    let btn_zoom_in = ti_zoom_in.btn.clone();
    let btn_sidebar = ti_sidebar.btn.clone();
    let btn_minimap = ti_minimap.btn.clone();
    let btn_vim = ti_vim.btn.clone();
    let btn_theme = ti_theme.btn.clone();

    let add_sep = |tb: &GtkBox| {
        let sep = Separator::new(Orientation::Vertical);
        sep.add_css_class("tool-sep");
        tb.append(&sep);
    };

    for b in [&btn_new, &btn_open, &btn_save, &btn_folder] {
        toolbar.append(b);
    }
    add_sep(&toolbar);
    toolbar.append(&btn_undo);
    toolbar.append(&btn_redo);
    add_sep(&toolbar);
    toolbar.append(&btn_find);
    toolbar.append(&btn_replace);
    add_sep(&toolbar);
    toolbar.append(&btn_font);
    toolbar.append(&btn_zoom_out);
    toolbar.append(&btn_zoom_reset);
    toolbar.append(&btn_zoom_in);
    add_sep(&toolbar);
    toolbar.append(&btn_sidebar);
    toolbar.append(&btn_minimap);
    toolbar.append(&btn_vim);
    toolbar.append(&btn_theme);

    let toolbar_items = vec![
        ti_new, ti_open, ti_save, ti_folder, ti_undo, ti_redo, ti_find, ti_replace, ti_font,
        ti_zoom_out, ti_zoom_in, ti_sidebar, ti_minimap, ti_vim, ti_theme,
    ];

    let toolbar_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Never)
        .propagate_natural_height(true)
        .child(&toolbar)
        .build();
    root.append(&toolbar_scroll);

    // 3. Interactive Tab Bar
    let tab_bar_outer = GtkBox::new(Orientation::Horizontal, 4);
    tab_bar_outer.add_css_class("tab-bar");
    let tab_box = GtkBox::new(Orientation::Horizontal, 4);
    let tab_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Never)
        .propagate_natural_height(true)
        .hexpand(true)
        .child(&tab_box)
        .build();
    let btn_add_tab = Button::with_label("+");
    btn_add_tab.add_css_class("tool-btn");
    btn_add_tab.set_tooltip_text(Some("New Tab (Ctrl+N)"));
    tab_bar_outer.append(&tab_scroll);
    tab_bar_outer.append(&btn_add_tab);
    root.append(&tab_bar_outer);

    // 4. Slide-down Find & Replace Bar (hidden by default)
    let find_bar = GtkBox::new(Orientation::Vertical, 6);
    find_bar.add_css_class("overlay-bar");
    find_bar.set_visible(false);

    let find_row = GtkBox::new(Orientation::Horizontal, 6);
    let find_lbl = Label::new(Some("Find:"));
    find_lbl.set_width_chars(8);
    find_lbl.set_xalign(1.0);
    let find_entry = Entry::new();
    find_entry.set_hexpand(true);
    find_entry.set_placeholder_text(Some("Search text (Enter = Next, Shift+Enter = Prev, Esc = Close)"));
    find_entry.add_css_class("studio-entry");
    let find_count_lbl = Label::new(Some(""));
    find_count_lbl.add_css_class("muted-label");
    let btn_find_prev = make_btn("↑ Prev", "Previous Match");
    let btn_find_next = make_btn("↓ Next", "Next Match");
    let btn_find_close = make_btn("✕", "Close Find Bar");
    find_row.append(&find_lbl);
    find_row.append(&find_entry);
    find_row.append(&find_count_lbl);
    find_row.append(&btn_find_prev);
    find_row.append(&btn_find_next);
    find_row.append(&btn_find_close);
    find_bar.append(&find_row);

    let replace_row = GtkBox::new(Orientation::Horizontal, 6);
    let repl_lbl = Label::new(Some("Replace:"));
    repl_lbl.set_width_chars(8);
    repl_lbl.set_xalign(1.0);
    let replace_entry = Entry::new();
    replace_entry.set_hexpand(true);
    replace_entry.set_placeholder_text(Some("Replacement text..."));
    replace_entry.add_css_class("studio-entry");
    let btn_repl_one = make_btn("Replace", "Replace Current Match");
    let btn_repl_all = make_btn("Replace All", "Replace All Matches");
    replace_row.append(&repl_lbl);
    replace_row.append(&replace_entry);
    replace_row.append(&btn_repl_one);
    replace_row.append(&btn_repl_all);
    find_bar.append(&replace_row);
    root.append(&find_bar);

    // 5. Slide-down Go To Line Bar (hidden by default)
    let goto_bar = GtkBox::new(Orientation::Horizontal, 6);
    goto_bar.add_css_class("overlay-bar");
    goto_bar.set_visible(false);
    let goto_lbl = Label::new(Some("Go to Line:"));
    let goto_entry = Entry::new();
    goto_entry.set_placeholder_text(Some("Enter line number and press Enter..."));
    goto_entry.set_width_chars(24);
    goto_entry.add_css_class("studio-entry");
    let btn_goto_go = make_btn("Jump", "Jump to Line");
    let btn_goto_close = make_btn("✕", "Close");
    goto_bar.append(&goto_lbl);
    goto_bar.append(&goto_entry);
    goto_bar.append(&btn_goto_go);
    goto_bar.append(&btn_goto_close);
    root.append(&goto_bar);

    // 6. Main Body: Collapsible Workspace Sidebar + Pango/Cairo Code Canvas
    let body = GtkBox::new(Orientation::Horizontal, 0);
    body.set_vexpand(true);

    let sidebar_box = GtkBox::new(Orientation::Vertical, 0);
    sidebar_box.set_width_request(230);
    sidebar_box.add_css_class("sidebar-box");
    let sidebar_header = Label::new(Some("EXPLORER"));
    sidebar_header.set_halign(Align::Fill);
    sidebar_header.set_xalign(0.0);
    sidebar_header.set_ellipsize(pango::EllipsizeMode::Start);
    sidebar_header.add_css_class("sidebar-header");
    let sidebar_list = ListBox::new();
    sidebar_list.set_selection_mode(SelectionMode::Single);
    sidebar_list.add_css_class("sidebar-list");
    let sidebar_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .child(&sidebar_list)
        .build();
    sidebar_box.append(&sidebar_header);
    sidebar_box.append(&sidebar_scroll);
    body.append(&sidebar_box);

    // Deferred signal channel so canvas can trigger UI updates cleanly
    let signal_handler_slot: Rc<RefCell<Option<Rc<dyn Fn(CanvasSignal)>>>> =
        Rc::new(RefCell::new(None));
    let canvas_signal_proxy: Rc<dyn Fn(CanvasSignal)> = {
        let slot = Rc::clone(&signal_handler_slot);
        Rc::new(move |sig| {
            if let Some(ref f) = *slot.borrow() {
                f(sig);
            }
        })
    };

    let canvas = build_code_canvas(Rc::clone(&state), canvas_signal_proxy);
    body.append(&canvas);
    root.append(&body);

    // 7. Studio Status Bar
    let status_bar = GtkBox::new(Orientation::Horizontal, 10);
    status_bar.add_css_class("status-bar");

    let status_vim = Label::new(None);
    status_vim.add_css_class("status-vim");
    status_vim.set_visible(false);

    let status_pos = Label::new(Some("Ln 1, Col 1"));
    let status_spacer = GtkBox::new(Orientation::Horizontal, 0);
    status_spacer.set_hexpand(true);

    let status_lang = Label::new(Some("Rust"));
    let status_indent = Button::with_label("Spaces: 4");
    status_indent.add_css_class("status-pill");
    status_indent.set_tooltip_text(Some("Click to cycle Indentation (4 Spaces / 2 Spaces / Tabs)"));
    let status_enc = Label::new(Some("UTF-8"));
    let status_theme = Button::with_label("Tokyo Night");
    status_theme.add_css_class("status-pill");
    status_theme.set_tooltip_text(Some("Click to cycle Studio Theme"));
    let status_zoom = Label::new(Some("100%"));

    status_bar.append(&status_vim);
    status_bar.append(&status_pos);
    status_bar.append(&status_spacer);
    status_bar.append(&status_lang);
    status_bar.append(&status_indent);
    status_bar.append(&status_enc);
    status_bar.append(&status_theme);
    status_bar.append(&status_zoom);
    root.append(&status_bar);

    window.set_child(Some(&root));

    let ui = Rc::new(UiHandles {
        window: window.clone(),
        css_provider,
        canvas: canvas.clone(),
        tab_box,
        sidebar_box,
        sidebar_header,
        sidebar_list: sidebar_list.clone(),
        toolbar_items,
        btn_save: btn_save.clone(),
        lbl_save,
        btn_font: btn_font.clone(),
        lbl_font,
        btn_zoom_reset: btn_zoom_reset.clone(),
        btn_sidebar: btn_sidebar.clone(),
        btn_minimap: btn_minimap.clone(),
        btn_vim: btn_vim.clone(),
        find_bar: find_bar.clone(),
        find_entry: find_entry.clone(),
        replace_row: replace_row.clone(),
        find_count_lbl,
        goto_bar: goto_bar.clone(),
        goto_entry: goto_entry.clone(),
        status_vim,
        status_pos,
        status_lang,
        status_indent: status_indent.clone(),
        status_theme: status_theme.clone(),
        status_zoom,
    });

    // Central UI refresh function (syncs CSS, tabs, sidebar, toolbar, status bar & canvas)
    let refresh_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let refresh_all: Rc<dyn Fn()> = {
        let state = Rc::clone(&state);
        let ui = Rc::clone(&ui);
        let refresh_slot = Rc::clone(&refresh_slot);
        Rc::new(move || {
            let st = state.borrow();
            let doc = st.doc();
            let theme = st.theme();

            // Window title
            let dirty_mark = if doc.dirty { "● " } else { "" };
            ui.window.set_title(Some(&format!(
                "{dirty_mark}{} — Studio Code Editor (GTK 4)",
                doc.title
            )));

            // Toolbar style, Google Material Symbols Sharp icons (theme-colored!) & active toggles
            for item in &ui.toolbar_items {
                let is_accent = match item.icon_kind {
                    MaterialIcon::Save => doc.dirty,
                    MaterialIcon::ViewSidebar => st.show_sidebar,
                    MaterialIcon::VerticalSplit => st.show_minimap,
                    MaterialIcon::Terminal => st.vim_mode != VimMode::Off,
                    MaterialIcon::Palette => true,
                    _ => false,
                };
                let icon_col = if is_accent {
                    theme.accent
                } else {
                    theme.text_fg
                };
                item.apply_style(st.toolbar_style, icon_col);
            }
            ui.lbl_save
                .set_text(if doc.dirty { "Save ●" } else { "Save" });
            set_toggle_class(&ui.btn_save, doc.dirty);
            let mut fn_disp = st.font_name.clone();
            if fn_disp.chars().count() > 14 {
                fn_disp = format!("{}…", fn_disp.chars().take(13).collect::<String>());
            }
            ui.lbl_font.set_text(&fn_disp);
            ui.btn_font
                .set_tooltip_text(Some(&format!("Font: {} (Click to change)", st.font_name)));
            ui.btn_zoom_reset.set_label(&format!("{}px", st.font_size));
            set_toggle_class(&ui.btn_sidebar, st.show_sidebar);
            set_toggle_class(&ui.btn_minimap, st.show_minimap);
            set_toggle_class(&ui.btn_vim, st.vim_mode != VimMode::Off);
            ui.sidebar_box.set_visible(st.show_sidebar);

            // Rebuild Tabs
            while let Some(c) = ui.tab_box.first_child() {
                ui.tab_box.remove(&c);
            }
            for (idx, d) in st.docs.iter().enumerate() {
                let is_active = idx == st.active_doc;
                let tab_container = GtkBox::new(Orientation::Horizontal, 4);
                tab_container.add_css_class(if is_active {
                    "tab-btn-active"
                } else {
                    "tab-btn"
                });

                let title_str = if d.dirty {
                    format!("● {}", d.title)
                } else {
                    d.title.clone()
                };
                let title_btn = Button::with_label(&title_str);
                title_btn.add_css_class("tab-close-btn");
                {
                    let state = Rc::clone(&state);
                    let rslot = Rc::clone(&refresh_slot);
                    let canvas = ui.canvas.clone();
                    title_btn.connect_clicked(move |_| {
                        {
                            let mut st = state.borrow_mut();
                            if idx < st.docs.len() {
                                st.active_doc = idx;
                                st.save_session();
                            }
                        }
                        if let Some(ref f) = *rslot.borrow() {
                            f();
                        }
                        canvas.grab_focus();
                    });
                }

                let close_btn = Button::with_label("×");
                close_btn.add_css_class("tab-close-btn");
                {
                    let state = Rc::clone(&state);
                    let rslot = Rc::clone(&refresh_slot);
                    let win = ui.window.clone();
                    close_btn.connect_clicked(move |_| {
                        if let Some(rf) = rslot.borrow().as_ref().cloned() {
                            request_close_tab(&win, Rc::clone(&state), idx, rf);
                        }
                    });
                }

                tab_container.append(&title_btn);
                tab_container.append(&close_btn);
                ui.tab_box.append(&tab_container);
            }

            // Rebuild Sidebar Explorer
            let dir_name = st
                .workspace_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("WORKSPACE");
            ui.sidebar_header
                .set_text(&format!("EXPLORER — {}", dir_name.to_uppercase()));
            while let Some(c) = ui.sidebar_list.first_child() {
                ui.sidebar_list.remove(&c);
            }
            for entry in &st.sidebar_entries {
                let icon = if entry.is_parent {
                    "↩"
                } else if entry.is_dir {
                    "📁"
                } else {
                    "📄"
                };
                let lbl = Label::new(Some(&format!("{icon}  {}", entry.name)));
                lbl.set_halign(Align::Start);
                lbl.set_ellipsize(pango::EllipsizeMode::End);
                lbl.add_css_class(if entry.is_dir {
                    "sidebar-dir"
                } else {
                    "sidebar-file"
                });
                let row = ListBoxRow::new();
                row.set_child(Some(&lbl));
                ui.sidebar_list.append(&row);
            }

            // Find match counter
            if ui.find_bar.is_visible() && !st.find_query.is_empty() {
                let n = doc.find_all_matches(&st.find_query).len();
                ui.find_count_lbl.set_text(&format!("{n} matches"));
            } else {
                ui.find_count_lbl.set_text("");
            }

            // Status bar
            if st.vim_mode != VimMode::Off {
                ui.status_vim.set_text(st.vim_mode.badge());
                ui.status_vim.set_visible(true);
            } else {
                ui.status_vim.set_visible(false);
            }

            let extra = doc.extra_cursors.len();
            let pos_str = if extra > 0 {
                format!(
                    "Ln {}, Col {}  ({} cursors)",
                    doc.cursor.line + 1,
                    doc.cursor.col + 1,
                    extra + 1
                )
            } else {
                format!("Ln {}, Col {}", doc.cursor.line + 1, doc.cursor.col + 1)
            };
            ui.status_pos.set_text(&pos_str);

            let lang = st
                .syntax
                .detect_language_name(doc.path.as_deref(), &doc.title);
            ui.status_lang.set_text(&lang);
            ui.status_indent.set_label(&doc.indent_style.label());
            ui.status_theme.set_label(theme.name);
            let zoom_pct = ((st.font_size as f32 / 15.0) * 100.0).round() as i32;
            ui.status_zoom.set_text(&format!("{zoom_pct}%"));

            drop(st);
            ui.canvas.queue_draw();
        })
    };
    *refresh_slot.borrow_mut() = Some(Rc::clone(&refresh_all));

    // Helper: open Find / Replace overlay
    let open_find_bar: Rc<dyn Fn(bool)> = {
        let state = Rc::clone(&state);
        let ui = Rc::clone(&ui);
        let refresh = Rc::clone(&refresh_all);
        Rc::new(move |with_replace| {
            ui.goto_bar.set_visible(false);
            ui.find_bar.set_visible(true);
            ui.replace_row.set_visible(with_replace);
            if let Some(sel) = state.borrow().doc().selected_text() {
                if !sel.contains('\n') && !sel.is_empty() {
                    ui.find_entry.set_text(&sel);
                    state.borrow_mut().find_query = sel;
                }
            }
            refresh();
            ui.find_entry.grab_focus();
            ui.find_entry.select_region(0, -1);
        })
    };

    // Helper: open Go To Line overlay
    let open_goto_bar: Rc<dyn Fn()> = {
        let ui = Rc::clone(&ui);
        Rc::new(move || {
            ui.find_bar.set_visible(false);
            ui.goto_bar.set_visible(true);
            ui.goto_entry.set_text("");
            ui.goto_entry.grab_focus();
        })
    };

    // Unified Command Palette / Action Dispatcher
    let dispatch_action: Rc<dyn Fn(PaletteAction)> = {
        let state = Rc::clone(&state);
        let ui = Rc::clone(&ui);
        let refresh = Rc::clone(&refresh_all);
        let open_find = Rc::clone(&open_find_bar);
        let open_goto = Rc::clone(&open_goto_bar);
        Rc::new(move |action| {
            match action {
                PaletteAction::FormatDocument => state.borrow_mut().doc_mut().format_document(),
                PaletteAction::ToggleComment => {
                    let mut st = state.borrow_mut();
                    let lang = st
                        .syntax
                        .detect_language_name(st.doc().path.as_deref(), &st.doc().title);
                    let pfx = SyntaxEngine::comment_prefix(&lang);
                    st.doc_mut().toggle_comment(pfx);
                }
                PaletteAction::DuplicateLine => state.borrow_mut().doc_mut().duplicate_lines(),
                PaletteAction::MoveLineUp => state.borrow_mut().doc_mut().move_lines_up(),
                PaletteAction::MoveLineDown => state.borrow_mut().doc_mut().move_lines_down(),
                PaletteAction::AddCursorNextOccurrence => {
                    state.borrow_mut().doc_mut().add_cursor_next_occurrence();
                }
                PaletteAction::ToggleSidebar => {
                    let mut st = state.borrow_mut();
                    st.show_sidebar = !st.show_sidebar;
                    st.save_session();
                }
                PaletteAction::ToggleMinimap => {
                    let mut st = state.borrow_mut();
                    st.show_minimap = !st.show_minimap;
                    st.save_session();
                }
                PaletteAction::ToggleBookmark => state.borrow_mut().doc_mut().toggle_bookmark(),
                PaletteAction::NextBookmark => {
                    state.borrow_mut().doc_mut().next_bookmark();
                    state.borrow_mut().ensure_cursor_visible();
                }
                PaletteAction::GoToLine => {
                    open_goto();
                    return;
                }
                PaletteAction::ZoomIn => state.borrow_mut().zoom_in(),
                PaletteAction::ZoomOut => state.borrow_mut().zoom_out(),
                PaletteAction::ZoomReset => state.borrow_mut().zoom_reset(),
                PaletteAction::ToggleWordWrap => {
                    let cur = state.borrow().doc().wrap_lines;
                    state.borrow_mut().doc_mut().wrap_lines = !cur;
                }
                PaletteAction::ToggleStickyScroll => {
                    let mut st = state.borrow_mut();
                    st.sticky_scroll = !st.sticky_scroll;
                    st.save_session();
                }
                PaletteAction::ToggleColorPreviews => {
                    let mut st = state.borrow_mut();
                    st.inline_color_previews = !st.inline_color_previews;
                    st.save_session();
                }
                PaletteAction::ToggleRainbowBrackets => {
                    let mut st = state.borrow_mut();
                    st.rainbow_brackets = !st.rainbow_brackets;
                    st.save_session();
                }
                PaletteAction::ToggleWhitespace => {
                    let mut st = state.borrow_mut();
                    st.show_whitespace = !st.show_whitespace;
                    st.save_session();
                }
                PaletteAction::ToggleIndentGuides => {
                    let mut st = state.borrow_mut();
                    st.show_indent_guides = !st.show_indent_guides;
                    st.save_session();
                }
                PaletteAction::ToggleFoldAtCursor => {
                    let line = state.borrow().doc().cursor.line;
                    state.borrow_mut().doc_mut().toggle_fold_at(line);
                }
                PaletteAction::UnfoldAll => state.borrow_mut().doc_mut().unfold_all(),
                PaletteAction::ToggleVimMode => state.borrow_mut().toggle_vim_mode(),
                PaletteAction::OpenFind => {
                    open_find(false);
                    return;
                }
                PaletteAction::OpenReplace => {
                    open_find(true);
                    return;
                }
                PaletteAction::NewTab => state.borrow_mut().new_tab(),
                PaletteAction::CloseFile => {
                    let cur = state.borrow().active_doc;
                    request_close_tab(&ui.window, Rc::clone(&state), cur, Rc::clone(&refresh));
                    return;
                }
                PaletteAction::IndentSpaces4 => {
                    state.borrow_mut().doc_mut().indent_style = IndentStyle::Spaces(4);
                }
                PaletteAction::IndentSpaces2 => {
                    state.borrow_mut().doc_mut().indent_style = IndentStyle::Spaces(2);
                }
                PaletteAction::IndentTabs => {
                    state.borrow_mut().doc_mut().indent_style = IndentStyle::Tabs;
                }
                PaletteAction::AboutDeveloper => {
                    show_about_dialog(&ui.window);
                    return;
                }
                PaletteAction::SetTheme(idx) => {
                    let mut st = state.borrow_mut();
                    st.theme_idx = idx % EditorTheme::all().len();
                    st.save_session();
                    apply_theme_css(&ui.css_provider, &st);
                }
            }
            refresh();
            ui.canvas.grab_focus();
        })
    };

    let force_close = Rc::new(Cell::new(false));

    // Wire Canvas -> Shell signals
    {
        let state = Rc::clone(&state);
        let ui = Rc::clone(&ui);
        let refresh = Rc::clone(&refresh_all);
        let open_find = Rc::clone(&open_find_bar);
        let open_goto = Rc::clone(&open_goto_bar);
        let dispatch = Rc::clone(&dispatch_action);
        let fc = Rc::clone(&force_close);
        *signal_handler_slot.borrow_mut() = Some(Rc::new(move |sig| match sig {
            CanvasSignal::RedrawAll => refresh(),
            CanvasSignal::OpenFind(r) => open_find(r),
            CanvasSignal::OpenGoToLine => open_goto(),
            CanvasSignal::OpenCommandPalette => {
                show_command_palette(&ui.window, Rc::clone(&state), Rc::clone(&dispatch));
            }
            CanvasSignal::NewTab => {
                state.borrow_mut().new_tab();
                refresh();
            }
            CanvasSignal::OpenFile => {
                open_file_dialog(&ui.window, Rc::clone(&state), Rc::clone(&refresh));
            }
            CanvasSignal::SaveFile => {
                save_file_or_prompt(&ui.window, Rc::clone(&state), Rc::clone(&refresh));
            }
            CanvasSignal::SaveFileAs => {
                save_file_as_dialog(&ui.window, Rc::clone(&state), Rc::clone(&refresh));
            }
            CanvasSignal::CloseTab => {
                let cur = state.borrow().active_doc;
                request_close_tab(&ui.window, Rc::clone(&state), cur, Rc::clone(&refresh));
            }
            CanvasSignal::ToggleSidebar => dispatch(PaletteAction::ToggleSidebar),
            CanvasSignal::Quit => {
                request_close_app(
                    &ui.window,
                    Rc::clone(&state),
                    Rc::clone(&fc),
                    Rc::clone(&refresh),
                );
            }
        }));
    }

    // Wire Sidebar click (navigate folder or open file)
    {
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        let canvas = ui.canvas.clone();
        sidebar_list.connect_row_activated(move |_, row| {
            let idx = row.index() as usize;
            let entry_opt = state.borrow().sidebar_entries.get(idx).cloned();
            if let Some(entry) = entry_opt {
                if entry.is_dir {
                    let mut st = state.borrow_mut();
                    st.workspace_dir = entry.path;
                    st.refresh_sidebar();
                    st.save_session();
                } else {
                    let _ = state.borrow_mut().open_file_from_path(entry.path);
                }
                refresh();
                canvas.grab_focus();
            }
        });
    }

    // Wire Find & Replace Bar
    {
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        find_entry.connect_changed(move |e| {
            state.borrow_mut().find_query = e.text().to_string();
            refresh();
        });
    }
    {
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        let do_next = Rc::new(move || {
            let mut st = state.borrow_mut();
            let q = st.find_query.clone();
            if !q.is_empty() {
                st.doc_mut().find_next(&q);
                st.ensure_cursor_visible();
            }
            drop(st);
            refresh();
        });
        let do_next_cl = Rc::clone(&do_next);
        find_entry.connect_activate(move |_| do_next_cl());
        btn_find_next.connect_clicked(move |_| do_next());
    }
    {
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        btn_find_prev.connect_clicked(move |_| {
            let mut st = state.borrow_mut();
            let q = st.find_query.clone();
            if !q.is_empty() {
                st.doc_mut().find_prev(&q);
                st.ensure_cursor_visible();
            }
            drop(st);
            refresh();
        });
    }
    {
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        let replace_entry_ref = replace_entry.clone();
        btn_repl_one.connect_clicked(move |_| {
            let repl = replace_entry_ref.text().to_string();
            let mut st = state.borrow_mut();
            let q = st.find_query.clone();
            st.replace_query = repl.clone();
            if !q.is_empty() {
                st.doc_mut().replace_current_or_next(&q, &repl);
                st.ensure_cursor_visible();
            }
            drop(st);
            refresh();
        });
    }
    {
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        let replace_entry_ref = replace_entry.clone();
        btn_repl_all.connect_clicked(move |_| {
            let repl = replace_entry_ref.text().to_string();
            let mut st = state.borrow_mut();
            let q = st.find_query.clone();
            st.replace_query = repl.clone();
            if !q.is_empty() {
                st.doc_mut().replace_all(&q, &repl);
            }
            drop(st);
            refresh();
        });
    }
    {
        let find_bar_hide = find_bar.clone();
        let canvas = ui.canvas.clone();
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        let close_find = Rc::new(move || {
            find_bar_hide.set_visible(false);
            state.borrow_mut().find_query.clear();
            refresh();
            canvas.grab_focus();
        });
        let close_cl = Rc::clone(&close_find);
        btn_find_close.connect_clicked(move |_| close_cl());
        let kc = gtk::EventControllerKey::new();
        kc.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                close_find();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        find_bar.add_controller(kc);
    }

    // Wire Go To Line Bar
    {
        let state = Rc::clone(&state);
        let goto_bar = goto_bar.clone();
        let goto_entry_ref = goto_entry.clone();
        let canvas = ui.canvas.clone();
        let refresh = Rc::clone(&refresh_all);
        let do_jump = Rc::new(move || {
            if let Ok(line) = goto_entry_ref.text().trim().parse::<usize>() {
                if line >= 1 {
                    let mut st = state.borrow_mut();
                    st.doc_mut().go_to_line(line);
                    st.ensure_cursor_visible();
                }
            }
            goto_bar.set_visible(false);
            refresh();
            canvas.grab_focus();
        });
        let do_jump_cl = Rc::clone(&do_jump);
        goto_entry.connect_activate(move |_| do_jump_cl());
        btn_goto_go.connect_clicked(move |_| do_jump());

        let goto_bar2 = ui.goto_bar.clone();
        let canvas2 = ui.canvas.clone();
        btn_goto_close.connect_clicked(move |_| {
            goto_bar2.set_visible(false);
            canvas2.grab_focus();
        });
        let kc = gtk::EventControllerKey::new();
        let goto_bar3 = ui.goto_bar.clone();
        let canvas3 = ui.canvas.clone();
        kc.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                goto_bar3.set_visible(false);
                canvas3.grab_focus();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        ui.goto_bar.add_controller(kc);
    }

    // Wire Toolbar Buttons
    {
        let dispatch = Rc::clone(&dispatch_action);
        btn_new.connect_clicked(move |_| dispatch(PaletteAction::NewTab));
    }
    {
        let dispatch = Rc::clone(&dispatch_action);
        btn_add_tab.connect_clicked(move |_| dispatch(PaletteAction::NewTab));
    }
    {
        let win = ui.window.clone();
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        btn_open.connect_clicked(move |_| {
            open_file_dialog(&win, Rc::clone(&state), Rc::clone(&refresh));
        });
    }
    {
        let win = ui.window.clone();
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        btn_save.connect_clicked(move |_| {
            save_file_or_prompt(&win, Rc::clone(&state), Rc::clone(&refresh));
        });
    }
    {
        let win = ui.window.clone();
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        btn_folder.connect_clicked(move |_| {
            open_folder_dialog(&win, Rc::clone(&state), Rc::clone(&refresh));
        });
    }
    {
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        let canvas = ui.canvas.clone();
        btn_undo.connect_clicked(move |_| {
            state.borrow_mut().doc_mut().undo();
            state.borrow_mut().ensure_cursor_visible();
            refresh();
            canvas.grab_focus();
        });
    }
    {
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        let canvas = ui.canvas.clone();
        btn_redo.connect_clicked(move |_| {
            state.borrow_mut().doc_mut().redo();
            state.borrow_mut().ensure_cursor_visible();
            refresh();
            canvas.grab_focus();
        });
    }
    {
        let open_find = Rc::clone(&open_find_bar);
        btn_find.connect_clicked(move |_| open_find(false));
    }
    {
        let open_find = Rc::clone(&open_find_bar);
        btn_replace.connect_clicked(move |_| open_find(true));
    }
    {
        let win = ui.window.clone();
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        btn_font.connect_clicked(move |_| {
            show_font_picker(&win, Rc::clone(&state), Rc::clone(&refresh));
        });
    }
    {
        let dispatch = Rc::clone(&dispatch_action);
        btn_zoom_out.connect_clicked(move |_| dispatch(PaletteAction::ZoomOut));
    }
    {
        let dispatch = Rc::clone(&dispatch_action);
        btn_zoom_reset.connect_clicked(move |_| dispatch(PaletteAction::ZoomReset));
    }
    {
        let dispatch = Rc::clone(&dispatch_action);
        btn_zoom_in.connect_clicked(move |_| dispatch(PaletteAction::ZoomIn));
    }
    {
        let dispatch = Rc::clone(&dispatch_action);
        btn_sidebar.connect_clicked(move |_| dispatch(PaletteAction::ToggleSidebar));
    }
    {
        let dispatch = Rc::clone(&dispatch_action);
        btn_minimap.connect_clicked(move |_| dispatch(PaletteAction::ToggleMinimap));
    }
    {
        let dispatch = Rc::clone(&dispatch_action);
        btn_vim.connect_clicked(move |_| dispatch(PaletteAction::ToggleVimMode));
    }
    {
        let state = Rc::clone(&state);
        let dispatch = Rc::clone(&dispatch_action);
        let cycle = Rc::new(move || {
            let next = (state.borrow().theme_idx + 1) % EditorTheme::all().len();
            dispatch(PaletteAction::SetTheme(next));
        });
        let cycle2 = Rc::clone(&cycle);
        btn_theme.connect_clicked(move |_| cycle());
        status_theme.connect_clicked(move |_| cycle2());
    }
    {
        let state = Rc::clone(&state);
        let refresh = Rc::clone(&refresh_all);
        status_indent.connect_clicked(move |_| {
            state.borrow_mut().cycle_indent_style();
            refresh();
        });
    }

    // Wire GApplication Menu Actions
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "new_tab", move || d(PaletteAction::NewTab));
    }
    {
        let win = ui.window.clone();
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        add_simple_action(app, "open_file", move || {
            open_file_dialog(&win, Rc::clone(&st), Rc::clone(&rf));
        });
    }
    {
        let win = ui.window.clone();
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        add_simple_action(app, "open_folder", move || {
            open_folder_dialog(&win, Rc::clone(&st), Rc::clone(&rf));
        });
    }
    {
        let win = ui.window.clone();
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        add_simple_action(app, "save_file", move || {
            save_file_or_prompt(&win, Rc::clone(&st), Rc::clone(&rf));
        });
    }
    {
        let win = ui.window.clone();
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        add_simple_action(app, "save_as", move || {
            save_file_as_dialog(&win, Rc::clone(&st), Rc::clone(&rf));
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "format_code", move || d(PaletteAction::FormatDocument));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "close_tab", move || d(PaletteAction::CloseFile));
    }
    {
        let win = ui.window.clone();
        let st = Rc::clone(&state);
        let fc = Rc::clone(&force_close);
        let rf = Rc::clone(&refresh_all);
        add_simple_action(app, "quit_app", move || {
            request_close_app(&win, Rc::clone(&st), Rc::clone(&fc), Rc::clone(&rf));
        });
    }
    {
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        add_simple_action(app, "undo", move || {
            st.borrow_mut().doc_mut().undo();
            st.borrow_mut().ensure_cursor_visible();
            rf();
        });
    }
    {
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        add_simple_action(app, "redo", move || {
            st.borrow_mut().doc_mut().redo();
            st.borrow_mut().ensure_cursor_visible();
            rf();
        });
    }
    {
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        let cv = ui.canvas.clone();
        add_simple_action(app, "cut", move || {
            let text = {
                let mut s = st.borrow_mut();
                let t = if let Some(sel) = s.doc().selected_text() {
                    s.doc_mut().push_undo(EditKind::BlockOp);
                    s.doc_mut().delete_selection_internal();
                    s.doc_mut().dirty = true;
                    sel
                } else {
                    s.doc_mut().delete_current_line()
                };
                s.ensure_cursor_visible();
                t
            };
            if let Some(disp) = gdk::Display::default() {
                disp.clipboard().set_text(&text);
            }
            rf();
            cv.grab_focus();
        });
    }
    {
        let st = Rc::clone(&state);
        let cv = ui.canvas.clone();
        add_simple_action(app, "copy", move || {
            let text = {
                let s = st.borrow();
                s.doc().selected_text().unwrap_or_else(|| {
                    format!("{}\n", s.doc().lines[s.doc().cursor.line])
                })
            };
            if let Some(disp) = gdk::Display::default() {
                disp.clipboard().set_text(&text);
            }
            cv.grab_focus();
        });
    }
    {
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        let cv = ui.canvas.clone();
        add_simple_action(app, "paste", move || {
            if let Some(disp) = gdk::Display::default() {
                let st_cl = Rc::clone(&st);
                let rf_cl = Rc::clone(&rf);
                let cv_cl = cv.clone();
                disp.clipboard()
                    .read_text_async(gio::Cancellable::NONE, move |res| {
                        if let Ok(Some(txt)) = res {
                            {
                                let mut s = st_cl.borrow_mut();
                                s.doc_mut().insert_str(&txt);
                                s.ensure_cursor_visible();
                            }
                            rf_cl();
                            cv_cl.grab_focus();
                        }
                    });
            }
        });
    }
    {
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        let cv = ui.canvas.clone();
        add_simple_action(app, "select_all", move || {
            st.borrow_mut().doc_mut().select_all();
            rf();
            cv.grab_focus();
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_bookmark", move || {
            d(PaletteAction::ToggleBookmark)
        });
    }
    {
        let f = Rc::clone(&open_find_bar);
        add_simple_action(app, "open_find", move || f(false));
    }
    {
        let f = Rc::clone(&open_find_bar);
        add_simple_action(app, "open_replace", move || f(true));
    }
    {
        let g = Rc::clone(&open_goto_bar);
        add_simple_action(app, "open_goto", move || g());
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_comment", move || d(PaletteAction::ToggleComment));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "duplicate_line", move || d(PaletteAction::DuplicateLine));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "move_up", move || d(PaletteAction::MoveLineUp));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "move_down", move || d(PaletteAction::MoveLineDown));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "add_cursor", move || {
            d(PaletteAction::AddCursorNextOccurrence)
        });
    }
    {
        let win = ui.window.clone();
        let st = Rc::clone(&state);
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "palette", move || {
            show_command_palette(&win, Rc::clone(&st), Rc::clone(&d));
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_sidebar", move || d(PaletteAction::ToggleSidebar));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_minimap", move || d(PaletteAction::ToggleMinimap));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_sticky", move || {
            d(PaletteAction::ToggleStickyScroll)
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_colors", move || {
            d(PaletteAction::ToggleColorPreviews)
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_rainbow", move || {
            d(PaletteAction::ToggleRainbowBrackets)
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_guides", move || {
            d(PaletteAction::ToggleIndentGuides)
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_ws", move || d(PaletteAction::ToggleWhitespace));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_vim", move || d(PaletteAction::ToggleVimMode));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "toggle_fold", move || {
            d(PaletteAction::ToggleFoldAtCursor)
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "unfold_all", move || d(PaletteAction::UnfoldAll));
    }
    {
        let win = ui.window.clone();
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        add_simple_action(app, "font_picker", move || {
            show_font_picker(&win, Rc::clone(&st), Rc::clone(&rf));
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "zoom_in", move || d(PaletteAction::ZoomIn));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "zoom_out", move || d(PaletteAction::ZoomOut));
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "zoom_reset", move || d(PaletteAction::ZoomReset));
    }
    for (act_name, style) in [
        ("tb_icons_only", ToolbarStyle::IconsOnly),
        ("tb_text_beside", ToolbarStyle::TextBesideIcons),
        ("tb_text_under", ToolbarStyle::TextUnderIcons),
    ] {
        let st = Rc::clone(&state);
        let rf = Rc::clone(&refresh_all);
        add_simple_action(app, act_name, move || {
            {
                let mut s = st.borrow_mut();
                s.toolbar_style = style;
                s.save_session();
            }
            rf();
        });
    }
    {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, "about", move || d(PaletteAction::AboutDeveloper));
    }
    for idx in 0..EditorTheme::all().len() {
        let d = Rc::clone(&dispatch_action);
        add_simple_action(app, &format!("theme_{idx}"), move || {
            d(PaletteAction::SetTheme(idx));
        });
    }

    // Guard window close against losing unsaved changes!
    {
        let st = Rc::clone(&state);
        let fc = Rc::clone(&force_close);
        let rf = Rc::clone(&refresh_all);
        window.connect_close_request(move |win| {
            if fc.get() {
                st.borrow().save_session();
                return glib::Propagation::Proceed;
            }
            let any_dirty = st.borrow().docs.iter().any(|d| d.dirty);
            if !any_dirty {
                st.borrow().save_session();
                return glib::Propagation::Proceed;
            }
            request_close_app(win, Rc::clone(&st), Rc::clone(&fc), Rc::clone(&rf));
            glib::Propagation::Stop
        });
    }

    refresh_all();
    window.present();
    canvas.grab_focus();
}

fn main() {
    let cli_args: Vec<PathBuf> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .collect();

    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        build_ui(app, &cli_args);
    });

    // Pass only argv[0] to GTK so CLI file paths are handled cleanly by our own parser
    let empty_args: [&str; 0] = [];
    app.run_with_args(&empty_args);
}
