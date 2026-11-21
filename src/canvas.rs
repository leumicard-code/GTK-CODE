// ============================================================================
// canvas.rs — High-Performance GTK 4 + Pango/Cairo Code Canvas
// Pixel-Perfect Pango Layout Metrics (Proportional + Monospace + Hebrew/BiDi),
// Mouse-Hover & Caret Bracket Matching, Right-Click Context Menu, Minimap & Vim
// ============================================================================

use std::cell::RefCell;
use std::rc::Rc;

use gtk::cairo::Context;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use gtk::{DrawingArea, PopoverMenu};

use crate::document::{EditKind, Pos};
use crate::state::{EditorState, VimMode};
use crate::syntax::SyntaxEngine;
use crate::themes::{EditorTheme, Rgb};

pub const GUTTER_WIDTH: f64 = 74.0;
pub const MINIMAP_WIDTH: f64 = 76.0;
pub const SCROLLBAR_WIDTH: f64 = 12.0;

#[derive(Clone, Copy, Debug)]
pub enum CanvasSignal {
    RedrawAll,
    OpenFind(bool),
    OpenGoToLine,
    OpenCommandPalette,
    NewTab,
    OpenFile,
    SaveFile,
    SaveFileAs,
    CloseTab,
    ToggleSidebar,
    Quit,
}

/// Normalizes physical keys including Hebrew keyboard layout so Ctrl+ש/ד/ב/כ work as Ctrl+A/S/C/F.
pub fn normalize_hotkey_char(key: gdk::Key) -> Option<char> {
    match key {
        gdk::Key::KP_Add | gdk::Key::plus | gdk::Key::equal => return Some('+'),
        gdk::Key::KP_Subtract | gdk::Key::minus => return Some('-'),
        gdk::Key::KP_0 | gdk::Key::_0 => return Some('0'),
        gdk::Key::KP_Divide | gdk::Key::slash => return Some('/'),
        gdk::Key::bracketleft | gdk::Key::braceleft => return Some('['),
        gdk::Key::bracketright | gdk::Key::braceright => return Some(']'),
        _ => {}
    }

    if let Some(ch) = key.to_unicode() {
        let mapped = match ch {
            'ש' => 'a',
            'נ' => 'b',
            'ב' => 'c',
            'ג' => 'd',
            'ק' => 'e',
            'כ' => 'f',
            'ע' => 'g',
            'י' => 'h',
            'ן' => 'i',
            'ח' => 'j',
            'ל' => 'k',
            'ך' => 'l',
            'צ' => 'm',
            'מ' => 'n',
            'ם' => 'o',
            'פ' => 'p',
            '/' => '/',
            'ר' => 'r',
            'ד' => 's',
            'א' => 't',
            'ו' => 'u',
            'ה' => 'v',
            '\'' => 'w',
            'ס' => 'x',
            'ט' => 'y',
            'ז' => 'z',
            c => c.to_ascii_lowercase(),
        };
        return Some(mapped);
    }
    None
}

/// Expands tabs & optional whitespace markers and returns:
/// - `display_text`: string passed directly to Pango
/// - `orig_to_byte`: byte offset in `display_text` for each original char index `0..=orig_len`
fn expand_tabs_for_display(line: &str, tab_w: usize, show_ws: bool) -> (String, Vec<usize>) {
    let char_count = line.chars().count();
    let mut out = String::with_capacity(line.len() + 8);
    let mut orig_to_byte = Vec::with_capacity(char_count + 1);
    let mut vis_col = 0usize;

    for ch in line.chars() {
        orig_to_byte.push(out.len());
        if ch == '\t' {
            let spaces = tab_w - (vis_col % tab_w);
            for s in 0..spaces {
                if show_ws && s == 0 {
                    out.push('→');
                } else {
                    out.push(' ');
                }
                vis_col += 1;
            }
        } else if ch == ' ' && show_ws {
            out.push('·');
            vis_col += 1;
        } else {
            out.push(ch);
            vis_col += 1;
        }
    }
    orig_to_byte.push(out.len());
    (out, orig_to_byte)
}

/// Exact X coordinate (in pixels relative to layout origin) for an original character column.
/// Works with 100% subpixel accuracy for proportional fonts (Segoe UI), monospace fonts,
/// ligatures (Cascadia Code), and Hebrew/RTL text!
fn col_x_in_layout(layout: &pango::Layout, orig_to_byte: &[usize], orig_col: usize) -> f64 {
    if orig_to_byte.is_empty() {
        return 0.0;
    }
    let idx = orig_col.min(orig_to_byte.len() - 1);
    let byte_idx = orig_to_byte[idx];
    let rect = layout.index_to_pos(byte_idx as i32);
    rect.x() as f64 / pango::SCALE as f64
}

/// Exact `(x, width)` bounding span (in pixels relative to layout origin) of a single character.
fn char_box_in_layout(
    layout: &pango::Layout,
    orig_to_byte: &[usize],
    orig_col: usize,
    fallback_w: f64,
) -> (f64, f64) {
    if orig_to_byte.is_empty() {
        return (0.0, fallback_w);
    }
    let idx = orig_col.min(orig_to_byte.len() - 1);
    let byte_idx = orig_to_byte[idx];
    let rect = layout.index_to_pos(byte_idx as i32);
    let x = rect.x() as f64 / pango::SCALE as f64;
    let mut w = (rect.width().abs() as f64) / pango::SCALE as f64;
    if w < 2.0 {
        w = fallback_w;
    }
    (x, w)
}

fn find_inline_hex_colors(line: &str) -> Vec<(usize, Rgb)> {
    let mut found = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i + 6 < chars.len() {
        if chars[i] == '#' {
            let hex: String = chars[i + 1..i + 7].iter().collect();
            let boundary_ok = i + 7 == chars.len() || !chars[i + 7].is_ascii_hexdigit();
            if boundary_ok && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&hex[0..2], 16),
                    u8::from_str_radix(&hex[2..4], 16),
                    u8::from_str_radix(&hex[4..6], 16),
                ) {
                    found.push((i, Rgb(r, g, b)));
                    i += 7;
                    continue;
                }
            }
        }
        i += 1;
    }
    found
}

fn set_rgb(cr: &Context, c: Rgb) {
    let (r, g, b) = c.cairo_f64();
    cr.set_source_rgb(r, g, b);
}

fn set_rgba(cr: &Context, c: Rgb, a: f64) {
    let (r, g, b) = c.cairo_f64();
    cr.set_source_rgba(r, g, b, a);
}

fn build_font_desc(font_name: &str, size_px: i32) -> pango::FontDescription {
    let spec = if font_name.trim().is_empty() {
        "Cascadia Code, JetBrains Mono, Monospace"
    } else {
        font_name.trim()
    };
    let mut fd = pango::FontDescription::from_string(spec);
    fd.set_absolute_size((size_px.max(8) as f64) * (pango::SCALE as f64));
    fd
}

fn draw_text_simple(
    cr: &Context,
    x: f64,
    y: f64,
    text: &str,
    fd: &pango::FontDescription,
    fallback_fg: Rgb,
) {
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_font_description(Some(fd));
    layout.set_text(text);
    set_rgb(cr, fallback_fg);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
}

fn draw_code_canvas(cr: &Context, w: f64, h: f64, st: &mut EditorState) {
    st.last_canvas_w = w;
    st.last_canvas_h = h;

    let theme = st.theme();
    let fd = build_font_desc(&st.font_name, st.font_size);

    // Measure average character width (for horizontal scroll step / indent guides) & exact line height
    let probe = pangocairo::functions::create_layout(cr);
    probe.set_font_description(Some(&fd));
    probe.set_text("0123456789abcdef");
    let (pw, ph) = probe.pixel_size();
    let avg_char_w = ((pw as f64) / 16.0).max(5.5);
    let space_probe = pangocairo::functions::create_layout(cr);
    space_probe.set_font_description(Some(&fd));
    space_probe.set_text("    ");
    let (sp4_w, _) = space_probe.pixel_size();
    let space_w = ((sp4_w as f64) / 4.0).max(3.5);
    let line_h = ((ph as f64) * 1.30).max(16.0).round();
    st.char_width = avg_char_w;
    st.line_height = line_h;

    let minimap_w = if st.show_minimap && w > 420.0 {
        MINIMAP_WIDTH
    } else {
        0.0
    };
    let text_area_x = GUTTER_WIDTH;
    let text_area_w = (w - GUTTER_WIDTH - minimap_w - SCROLLBAR_WIDTH).max(80.0);
    let minimap_x = GUTTER_WIDTH + text_area_w;
    let vscroll_x = w - SCROLLBAR_WIDTH;

    // 1. Editor background
    set_rgb(cr, theme.bg);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();

    // 2. Gutter background
    set_rgb(cr, theme.gutter_bg);
    cr.rectangle(0.0, 0.0, GUTTER_WIDTH, h);
    let _ = cr.fill();
    set_rgb(cr, theme.indent_guide);
    cr.rectangle(GUTTER_WIDTH - 1.0, 0.0, 1.0, h);
    let _ = cr.fill();

    // Collect visible (unfolded) lines
    let doc = st.doc();
    let visible_lines = doc.visible_doc_lines();
    let total_vis = visible_lines.len().max(1);
    let visible_rows = ((h / line_h).ceil() as usize).max(1);
    let scroll_y = doc.scroll_y.min(total_vis.saturating_sub(1));
    let scroll_x = doc.scroll_x;
    let tab_w = doc.indent_style.step_cols();

    // Sticky scroll header lines (enclosing fn/struct/impl/class)
    let top_doc_line = visible_lines.get(scroll_y).copied().unwrap_or(0);
    let sticky_lines = if st.sticky_scroll && scroll_y > 0 {
        let mut s = doc.sticky_headers_for(top_doc_line);
        s.truncate(3);
        s
    } else {
        Vec::new()
    };
    let sticky_h = (sticky_lines.len() as f64) * line_h;

    let start_doc_line = visible_lines.get(scroll_y).copied().unwrap_or(0);
    let end_vis = (scroll_y + visible_rows + 2).min(visible_lines.len());
    let end_doc_line = visible_lines
        .get(end_vis.saturating_sub(1))
        .map(|l| l + 1)
        .unwrap_or(doc.lines.len())
        .min(doc.lines.len());

    // Batch syntax highlighting for visible range
    let highlighted = st.syntax.highlight_lines(
        &doc.lines,
        start_doc_line,
        end_doc_line,
        doc.path.as_deref(),
        &doc.title,
        theme,
    );

    let sel_range = doc.selection_range();
    // Highlight matching brackets for BOTH text caret AND mouse hover!
    let caret_bracket_pair = doc.find_matching_bracket();
    let hover_bracket_pair = st
        .hover_pos
        .and_then(|hp| doc.find_matching_bracket_at(hp));

    let find_matches = if !st.find_query.is_empty() {
        doc.find_all_matches(&st.find_query)
    } else {
        Vec::new()
    };

    // Estimate running bracket depth up to start_doc_line for rainbow brackets
    let mut running_depth: usize = 0;
    if st.rainbow_brackets {
        let scan_from = start_doc_line.saturating_sub(120);
        for line in doc.lines.iter().take(start_doc_line).skip(scan_from) {
            for ch in line.chars() {
                match ch {
                    '(' | '[' | '{' => running_depth = running_depth.saturating_add(1),
                    ')' | ']' | '}' => running_depth = running_depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
    }

    let text_pad_left = 10.0;
    let x_offset = text_area_x + text_pad_left - (scroll_x as f64) * avg_char_w;
    let mut autocomplete_caret_xy: Option<(f64, f64)> = None;

    // 3. Render visible code rows
    for (screen_row, &doc_line) in visible_lines[scroll_y..end_vis].iter().enumerate() {
        let y = (screen_row as f64) * line_h;
        if y > h {
            break;
        }
        let is_active = doc.cursor.line == doc_line;
        let raw_line = &doc.lines[doc_line];

        // Active line background
        if is_active {
            set_rgb(cr, theme.active_line_bg);
            cr.rectangle(text_area_x, y, text_area_w, line_h);
            let _ = cr.fill();
        }

        // Gutter: Bookmark dot + Line number + Folding chevron
        if doc.bookmarks.contains(&doc_line) {
            set_rgb(cr, theme.accent);
            cr.arc(10.0, y + line_h * 0.5, 4.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }

        let num_str = format!("{:>4}", doc_line + 1);
        draw_text_simple(
            cr,
            16.0,
            y + (line_h - ph as f64) * 0.5,
            &num_str,
            &fd,
            if is_active {
                theme.gutter_active_fg
            } else {
                theme.gutter_fg
            },
        );

        if doc.foldable_range(doc_line).is_some() {
            let folded = doc.folded_headers.contains(&doc_line);
            let chevron = if folded { "▸" } else { "▾" };
            draw_text_simple(
                cr,
                GUTTER_WIDTH - 15.0,
                y + (line_h - ph as f64) * 0.5,
                chevron,
                &fd,
                if folded { theme.accent } else { theme.gutter_fg },
            );
        }

        // Clip text area for code, highlights, indent guides & carets
        let _ = cr.save();
        cr.rectangle(text_area_x, y, text_area_w, line_h);
        cr.clip();

        // Build the single Pango Layout FIRST so all geometry (carets, selections, brackets, swatches)
        // uses exact Pango `index_to_pos()` pixel coordinates!
        let (display_text, orig_to_byte) =
            expand_tabs_for_display(raw_line, tab_w, st.show_whitespace);
        let attrs = pango::AttrList::new();
        let spans_opt = doc_line
            .checked_sub(start_doc_line)
            .and_then(|rel| highlighted.get(rel));

        let mut orig_colors: Vec<Rgb> = Vec::with_capacity(raw_line.chars().count());
        if let Some(spans) = spans_opt {
            for sp in spans {
                for _ in sp.text.chars() {
                    orig_colors.push(sp.fg);
                }
            }
        }

        for (orig_char_idx, ch) in raw_line.chars().enumerate() {
            let base_fg = orig_colors.get(orig_char_idx).copied().unwrap_or(theme.text_fg);
            let final_fg = if ch == '\t' || (ch == ' ' && st.show_whitespace) {
                theme.indent_guide
            } else if st.rainbow_brackets && matches!(ch, '(' | '[' | '{') {
                let c = theme.rainbow[running_depth % theme.rainbow.len()];
                running_depth = running_depth.saturating_add(1);
                c
            } else if st.rainbow_brackets && matches!(ch, ')' | ']' | '}') {
                running_depth = running_depth.saturating_sub(1);
                theme.rainbow[running_depth % theme.rainbow.len()]
            } else {
                base_fg
            };

            let start_b = orig_to_byte[orig_char_idx];
            let end_b = orig_to_byte[orig_char_idx + 1];
            if start_b < end_b {
                let mut attr = pango::AttrColor::new_foreground(
                    (final_fg.0 as u16) * 257,
                    (final_fg.1 as u16) * 257,
                    (final_fg.2 as u16) * 257,
                );
                attr.set_start_index(start_b as u32);
                attr.set_end_index(end_b as u32);
                attrs.insert(attr);
            }
        }

        let line_layout = pangocairo::functions::create_layout(cr);
        line_layout.set_font_description(Some(&fd));
        line_layout.set_text(&display_text);
        line_layout.set_attributes(Some(&attrs));
        let (line_pixel_w, _) = line_layout.pixel_size();

        // Indent Guides (aligned to exact space width of active font)
        if st.show_indent_guides {
            let indent_cols = raw_line
                .chars()
                .take_while(|&c| c == ' ' || c == '\t')
                .fold(0usize, |acc, c| {
                    if c == '\t' {
                        acc + (tab_w - (acc % tab_w))
                    } else {
                        acc + 1
                    }
                });
            let mut col = tab_w;
            set_rgb(cr, theme.indent_guide);
            while col <= indent_cols {
                let gx = x_offset + (col as f64) * space_w;
                if gx >= text_area_x && gx <= text_area_x + text_area_w {
                    cr.rectangle(gx, y, 1.0, line_h);
                    let _ = cr.fill();
                }
                col += tab_w;
            }
        }

        // Selection Highlight (pixel-exact!)
        if let Some((s, e)) = sel_range {
            if doc_line >= s.line && doc_line <= e.line {
                let c0 = if doc_line == s.line { s.col } else { 0 };
                let x0 = x_offset + col_x_in_layout(&line_layout, &orig_to_byte, c0);
                let x1 = if doc_line == e.line {
                    x_offset + col_x_in_layout(&line_layout, &orig_to_byte, e.col)
                } else {
                    x_offset + (line_pixel_w as f64) + avg_char_w
                };
                let sw = (x1 - x0).max(avg_char_w * 0.6);
                set_rgb(cr, theme.selection_bg);
                cr.rectangle(x0, y, sw, line_h);
                let _ = cr.fill();
            }
        }

        // Find Match Highlights (pixel-exact!)
        for &(ms, me) in &find_matches {
            if ms.line == doc_line {
                let mx0 = x_offset + col_x_in_layout(&line_layout, &orig_to_byte, ms.col);
                let mx1 = x_offset + col_x_in_layout(&line_layout, &orig_to_byte, me.col);
                let mw = (mx1 - mx0).max(avg_char_w);
                set_rgba(cr, theme.accent, 0.28);
                cr.rectangle(mx0, y + 1.0, mw, line_h - 2.0);
                let _ = cr.fill();
                set_rgba(cr, theme.accent, 0.85);
                cr.set_line_width(1.0);
                cr.rectangle(mx0 + 0.5, y + 1.5, mw - 1.0, line_h - 3.0);
                let _ = cr.stroke();
            }
        }

        // Matching Bracket Highlight (BOTH caret brackets AND mouse-hover brackets with crisp accent box!)
        for pair_opt in [caret_bracket_pair, hover_bracket_pair] {
            if let Some((bp1, bp2)) = pair_opt {
                for bp in [bp1, bp2] {
                    if bp.line == doc_line {
                        let (rel_x, bw) =
                            char_box_in_layout(&line_layout, &orig_to_byte, bp.col, avg_char_w);
                        let bx = x_offset + rel_x;
                        set_rgba(cr, theme.accent, 0.26);
                        cr.rectangle(bx - 1.0, y + 1.0, bw + 2.0, line_h - 2.0);
                        let _ = cr.fill();
                        set_rgba(cr, theme.accent, 0.92);
                        cr.set_line_width(1.4);
                        cr.rectangle(bx - 0.5, y + 1.5, bw + 1.0, line_h - 3.0);
                        let _ = cr.stroke();
                    }
                }
            }
        }

        // Render the syntax-colored Pango text line
        set_rgb(cr, theme.text_fg);
        cr.move_to(x_offset, y + (line_h - ph as f64) * 0.5);
        pangocairo::functions::show_layout(cr, &line_layout);

        // Inline Hex Color Swatches (#RRGGBB) placed RIGHT NEXT to the exact hex token!
        if st.inline_color_previews && raw_line.contains('#') {
            for (hex_col, color) in find_inline_hex_colors(raw_line) {
                let rel_end_x = col_x_in_layout(&line_layout, &orig_to_byte, hex_col + 7);
                let sx = x_offset + rel_end_x + 4.0;
                let sz = (line_h * 0.52).clamp(9.0, 14.0);
                let sy = y + (line_h - sz) * 0.5;
                if sx + sz < text_area_x + text_area_w {
                    set_rgb(cr, color);
                    cr.rectangle(sx, sy, sz, sz);
                    let _ = cr.fill();
                    set_rgb(cr, theme.text_fg);
                    cr.set_line_width(1.0);
                    cr.rectangle(sx + 0.5, sy + 0.5, sz - 1.0, sz - 1.0);
                    let _ = cr.stroke();
                }
            }
        }

        // Folded badge (`⋯ +N lines`)
        if doc.folded_headers.contains(&doc_line) {
            if let Some(end_l) = doc.foldable_range(doc_line) {
                let hidden = end_l.saturating_sub(doc_line);
                let bx = x_offset + (line_pixel_w as f64) + 10.0;
                let badge = format!(" ⋯ +{hidden} lines ");
                let b_layout = pangocairo::functions::create_layout(cr);
                b_layout.set_font_description(Some(&fd));
                b_layout.set_text(&badge);
                let (bw, _) = b_layout.pixel_size();
                set_rgba(cr, theme.accent, 0.22);
                cr.rectangle(bx, y + 2.0, bw as f64, line_h - 4.0);
                let _ = cr.fill();
                set_rgb(cr, theme.accent);
                cr.move_to(bx, y + (line_h - ph as f64) * 0.5);
                pangocairo::functions::show_layout(cr, &b_layout);
            }
        }

        // Pixel-Exact Multi-Cursor Carets
        let block_cursor = matches!(
            st.vim_mode,
            VimMode::Normal | VimMode::Visual | VimMode::VisualLine
        );
        for cur in doc.all_cursors() {
            if cur.line == doc_line {
                let (rel_x, cw) =
                    char_box_in_layout(&line_layout, &orig_to_byte, cur.col, avg_char_w);
                let cx = x_offset + rel_x;
                if cur == doc.cursor {
                    autocomplete_caret_xy = Some((cx, y + line_h + 2.0));
                }
                if st.caret_visible {
                    if block_cursor {
                        set_rgba(cr, theme.cursor, 0.55);
                        cr.rectangle(cx, y + 1.0, cw, line_h - 2.0);
                        let _ = cr.fill();
                    } else {
                        set_rgb(cr, theme.cursor);
                        cr.rectangle(cx, y + 1.0, 2.2, line_h - 2.0);
                        let _ = cr.fill();
                    }
                }
            }
        }

        let _ = cr.restore();
    }

    // 4. Sticky Scroll Enclosing Scope Header Bar
    if !sticky_lines.is_empty() {
        let _ = cr.save();
        cr.rectangle(0.0, 0.0, text_area_x + text_area_w, sticky_h + 2.0);
        cr.clip();

        set_rgb(cr, theme.gutter_bg);
        cr.rectangle(0.0, 0.0, text_area_x + text_area_w, sticky_h);
        let _ = cr.fill();

        for (i, &sline) in sticky_lines.iter().enumerate() {
            let y = (i as f64) * line_h;
            let num_str = format!("{:>4}", sline + 1);
            draw_text_simple(
                cr,
                16.0,
                y + (line_h - ph as f64) * 0.5,
                &num_str,
                &fd,
                theme.gutter_active_fg,
            );
            if let Some(txt) = doc.lines.get(sline) {
                let (disp, _) = expand_tabs_for_display(txt, tab_w, false);
                draw_text_simple(
                    cr,
                    x_offset,
                    y + (line_h - ph as f64) * 0.5,
                    &disp,
                    &fd,
                    theme.text_fg,
                );
            }
        }

        set_rgb(cr, theme.accent);
        cr.rectangle(0.0, sticky_h - 1.0, text_area_x + text_area_w, 1.2);
        let _ = cr.fill();
        let _ = cr.restore();
    }

    // 5. Live Syntax Minimap (Right Side)
    if minimap_w > 0.0 {
        draw_minimap(cr, minimap_x, 0.0, minimap_w, h, doc, theme, visible_rows);
    }

    // 6. Custom Smooth Vertical Scrollbar
    draw_vscrollbar(
        cr,
        vscroll_x,
        0.0,
        SCROLLBAR_WIDTH,
        h,
        scroll_y,
        total_vis,
        visible_rows,
        st.hover_vscroll || st.dragging_vscroll.is_some(),
        theme,
    );

    // 7. Floating Autocomplete Popup (anchored right under the exact Pango caret!)
    if st.autocomplete_open {
        let candidates = doc.completion_candidates();
        if !candidates.is_empty() {
            if let Some((cx, mut py)) = autocomplete_caret_xy {
                let item_h = (line_h + 2.0).max(22.0);
                let shown = candidates.len().min(8);
                let pop_w = 220.0;
                let pop_h = (shown as f64) * item_h + 8.0;
                let mut px = cx.clamp(text_area_x + 4.0, (w - pop_w - 16.0).max(text_area_x + 4.0));
                if py + pop_h > h {
                    py = (py - line_h - pop_h - 4.0).max(4.0);
                }
                if px + pop_w > minimap_x {
                    px = (minimap_x - pop_w - 6.0).max(text_area_x + 4.0);
                }

                set_rgb(cr, theme.gutter_bg);
                cr.rectangle(px, py, pop_w, pop_h);
                let _ = cr.fill();
                set_rgb(cr, theme.accent);
                cr.set_line_width(1.2);
                cr.rectangle(px + 0.5, py + 0.5, pop_w - 1.0, pop_h - 1.0);
                let _ = cr.stroke();

                let sel = st.autocomplete_selected % shown;
                for (idx, item) in candidates.iter().take(shown).enumerate() {
                    let iy = py + 4.0 + (idx as f64) * item_h;
                    if idx == sel {
                        set_rgb(cr, theme.selection_bg);
                        cr.rectangle(px + 3.0, iy, pop_w - 6.0, item_h);
                        let _ = cr.fill();
                    }
                    draw_text_simple(
                        cr,
                        px + 10.0,
                        iy + (item_h - ph as f64) * 0.5,
                        item,
                        &fd,
                        if idx == sel {
                            theme.accent
                        } else {
                            theme.text_fg
                        },
                    );
                }
            }
        }
    }
}

fn draw_minimap(
    cr: &Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    doc: &crate::document::Document,
    theme: &EditorTheme,
    visible_rows: usize,
) {
    set_rgb(cr, theme.gutter_bg);
    cr.rectangle(x, y, w, h);
    let _ = cr.fill();
    set_rgb(cr, theme.indent_guide);
    cr.rectangle(x, y, 1.0, h);
    let _ = cr.fill();

    let total = doc.lines.len().max(1);
    let row_step = 2.6_f64;
    let max_rows = (h / row_step).floor() as usize;
    let stride = if total > max_rows.max(1) {
        (total as f64) / (max_rows.max(1) as f64)
    } else {
        1.0
    };

    let vp_top = ((doc.scroll_y as f64) / stride) * row_step;
    let vp_h = ((visible_rows as f64) / stride * row_step).clamp(18.0, h);
    set_rgba(cr, theme.accent, 0.18);
    cr.rectangle(x + 1.0, y + vp_top.clamp(0.0, (h - vp_h).max(0.0)), w - 2.0, vp_h);
    let _ = cr.fill();

    let mut row = 0usize;
    while (row as f64) * row_step < h {
        let line_idx = ((row as f64) * stride).floor() as usize;
        if line_idx >= doc.lines.len() {
            break;
        }
        let line = &doc.lines[line_idx];
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            let leading = line.chars().take_while(|c| c.is_whitespace()).count();
            let bar_x = x + 5.0 + (leading.min(20) as f64) * 1.1;
            let bar_w = ((trimmed.chars().count() as f64) * 0.95)
                .clamp(4.0, (w - (bar_x - x) - 4.0).max(4.0));
            let col = if trimmed.starts_with("//") || trimmed.starts_with('#') {
                theme.gutter_fg
            } else if trimmed.starts_with("fn ")
                || trimmed.starts_with("pub ")
                || trimmed.starts_with("struct ")
                || trimmed.starts_with("impl ")
                || trimmed.starts_with("class ")
                || trimmed.starts_with("def ")
            {
                theme.accent
            } else {
                theme.text_fg
            };
            set_rgba(cr, col, 0.52);
            cr.rectangle(bar_x, y + (row as f64) * row_step, bar_w, 1.5);
            let _ = cr.fill();
        }
        row += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_vscrollbar(
    cr: &Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    scroll_y: usize,
    total_lines: usize,
    visible_rows: usize,
    hovered: bool,
    theme: &EditorTheme,
) {
    set_rgb(cr, theme.scrollbar_track);
    cr.rectangle(x, y, w, h);
    let _ = cr.fill();

    let total = total_lines.max(1) as f64;
    let vis = visible_rows.max(1) as f64;
    let thumb_h = ((vis / total) * h).clamp(28.0, h);
    let max_scroll = total_lines.saturating_sub(1).max(1) as f64;
    let ratio = (scroll_y as f64 / max_scroll).clamp(0.0, 1.0);
    let thumb_y = y + ratio * (h - thumb_h).max(0.0);

    set_rgb(
        cr,
        if hovered {
            theme.scrollbar_hover
        } else {
            theme.scrollbar_thumb
        },
    );
    cr.rectangle(x + 2.0, thumb_y + 1.0, w - 4.0, (thumb_h - 2.0).max(8.0));
    let _ = cr.fill();
}

/// Exact Pango hit-testing: maps any pixel `(px, py)` to the exact document `(line, col)`
/// for BOTH monospace and proportional fonts (Segoe UI, Cascadia Code, Hebrew, etc.).
fn pixel_to_doc_pos(area: &DrawingArea, st: &EditorState, px: f64, py: f64) -> Pos {
    let doc = st.doc();
    let visible = doc.visible_doc_lines();
    let scroll_y = doc.scroll_y.min(visible.len().saturating_sub(1));
    let row = (py / st.line_height.max(12.0)).floor().max(0.0) as usize;
    let vis_idx = (scroll_y + row).min(visible.len().saturating_sub(1));
    let doc_line = visible.get(vis_idx).copied().unwrap_or(0);

    let raw_line = &doc.lines[doc_line];
    let tab_w = doc.indent_style.step_cols();
    let text_x = GUTTER_WIDTH + 10.0 - (doc.scroll_x as f64) * st.char_width;
    let rel_x = px - text_x;
    if rel_x <= 0.0 || raw_line.is_empty() {
        return Pos::new(doc_line, 0);
    }

    let (display_text, orig_to_byte) =
        expand_tabs_for_display(raw_line, tab_w, st.show_whitespace);
    let fd = build_font_desc(&st.font_name, st.font_size);
    let layout = area.create_pango_layout(Some(&display_text));
    layout.set_font_description(Some(&fd));

    let (inside, byte_idx, trailing) =
        layout.xy_to_index((rel_x * pango::SCALE as f64).round() as i32, 0);

    if !inside && rel_x > 0.0 {
        let (pw, _) = layout.pixel_size();
        if rel_x >= pw as f64 {
            return Pos::new(doc_line, raw_line.chars().count());
        }
    }

    let target_byte = (byte_idx.max(0) as usize).min(display_text.len());
    // Binary search / linear scan `orig_to_byte` to find original character column
    let orig_len = raw_line.chars().count();
    let mut orig_col = orig_len;
    for c in 0..orig_len {
        if target_byte < orig_to_byte[c + 1] {
            orig_col = if trailing > 0 {
                (c + 1).min(orig_len)
            } else {
                c
            };
            break;
        }
    }
    Pos::new(doc_line, orig_col)
}

/// Builds the Right-Click Context Menu (`gtk::PopoverMenu`) attached to the code canvas.
fn build_context_menu(area: &DrawingArea) -> PopoverMenu {
    let menu = gio::Menu::new();

    let clip_sec = gio::Menu::new();
    clip_sec.append(Some("Cut\tCtrl+X"), Some("app.cut"));
    clip_sec.append(Some("Copy\tCtrl+C"), Some("app.copy"));
    clip_sec.append(Some("Paste\tCtrl+V"), Some("app.paste"));
    clip_sec.append(Some("Select All\tCtrl+A"), Some("app.select_all"));
    menu.append_section(None, &clip_sec);

    let edit_sec = gio::Menu::new();
    edit_sec.append(Some("Undo\tCtrl+Z"), Some("app.undo"));
    edit_sec.append(Some("Redo\tCtrl+Y"), Some("app.redo"));
    edit_sec.append(Some("Toggle Comment\tCtrl+/"), Some("app.toggle_comment"));
    edit_sec.append(Some("Duplicate Line\tCtrl+Shift+D"), Some("app.duplicate_line"));
    edit_sec.append(Some("Format Code\tCtrl+Shift+I"), Some("app.format_code"));
    menu.append_section(None, &edit_sec);

    let nav_sec = gio::Menu::new();
    nav_sec.append(Some("Find...\tCtrl+F"), Some("app.open_find"));
    nav_sec.append(Some("Replace...\tCtrl+H"), Some("app.open_replace"));
    nav_sec.append(Some("Toggle Bookmark\tF2"), Some("app.toggle_bookmark"));
    nav_sec.append(Some("Command Palette...\tCtrl+Shift+P"), Some("app.palette"));
    menu.append_section(None, &nav_sec);

    let popover = PopoverMenu::from_model(Some(&menu));
    popover.set_parent(area);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover
}

fn handle_vim_normal_key(
    st: &mut EditorState,
    key: gdk::Key,
    ch: Option<char>,
    ctrl: bool,
) -> bool {
    if ctrl {
        return false;
    }
    if key == gdk::Key::Escape {
        st.vim_mode = VimMode::Normal;
        st.vim_pending_key = None;
        st.doc_mut().anchor = None;
        return true;
    }

    let select = matches!(st.vim_mode, VimMode::Visual | VimMode::VisualLine);
    let Some(c) = ch else {
        return false;
    };

    if let Some(pending) = st.vim_pending_key.take() {
        match (pending, c) {
            ('g', 'g') => {
                st.doc_mut().go_to_line(1);
                return true;
            }
            ('d', 'd') => {
                let yanked = st.doc_mut().delete_current_line();
                st.vim_yank_buffer = yanked;
                return true;
            }
            ('y', 'y') => {
                let line = st.doc().lines[st.doc().cursor.line].clone();
                st.vim_yank_buffer = format!("{line}\n");
                return true;
            }
            _ => return true,
        }
    }

    match c {
        'h' => st.doc_mut().move_left(select),
        'l' => st.doc_mut().move_right(select),
        'k' => st.doc_mut().move_up(select),
        'j' => st.doc_mut().move_down(select),
        'w' => st.doc_mut().move_word_forward(select),
        'b' => st.doc_mut().move_word_backward(select),
        '0' => st.doc_mut().move_home(select),
        '$' => st.doc_mut().move_end(select),
        'G' => {
            let last = st.doc().lines.len();
            st.doc_mut().go_to_line(last);
        }
        'g' | 'd' | 'y' => {
            st.vim_pending_key = Some(c);
        }
        'x' => st.doc_mut().delete_forward(),
        'u' => {
            st.doc_mut().undo();
        }
        'p' => {
            let buf = st.vim_yank_buffer.clone();
            if !buf.is_empty() {
                st.doc_mut().insert_str(&buf);
            }
        }
        'i' => {
            st.vim_mode = VimMode::Insert;
            st.doc_mut().anchor = None;
        }
        'a' => {
            st.doc_mut().move_right(false);
            st.vim_mode = VimMode::Insert;
        }
        'o' => {
            st.doc_mut().move_end(false);
            st.doc_mut().insert_newline_auto_indent();
            st.vim_mode = VimMode::Insert;
        }
        'O' => {
            st.doc_mut().move_home(false);
            st.doc_mut().insert_newline_auto_indent();
            st.doc_mut().move_up(false);
            st.vim_mode = VimMode::Insert;
        }
        'v' => {
            if st.vim_mode == VimMode::Visual {
                st.vim_mode = VimMode::Normal;
                st.doc_mut().anchor = None;
            } else {
                st.vim_mode = VimMode::Visual;
                let cur = st.doc().cursor;
                st.doc_mut().anchor = Some(cur);
            }
        }
        'V' => {
            if st.vim_mode == VimMode::VisualLine {
                st.vim_mode = VimMode::Normal;
                st.doc_mut().anchor = None;
            } else {
                st.vim_mode = VimMode::VisualLine;
                st.doc_mut().move_home(false);
                st.doc_mut().move_end(true);
            }
        }
        _ => return false,
    }
    true
}

/// Builds and wires the complete GTK 4 + Pango/Cairo `DrawingArea` code canvas.
pub fn build_code_canvas(
    state: Rc<RefCell<EditorState>>,
    on_signal: Rc<dyn Fn(CanvasSignal)>,
) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_focusable(true);
    area.set_can_focus(true);
    area.set_cursor_from_name(Some("text"));

    // Right-Click Context Menu Popover
    let context_popover = build_context_menu(&area);
    {
        let pop = context_popover.clone();
        area.connect_destroy(move |_| {
            pop.unparent();
        });
    }

    // Draw function
    {
        let state = Rc::clone(&state);
        area.set_draw_func(move |_area, cr, w, h| {
            let mut st = state.borrow_mut();
            draw_code_canvas(cr, w as f64, h as f64, &mut st);
        });
    }

    // Smooth 530ms caret blink timer
    {
        let state = Rc::clone(&state);
        let area_weak = area.downgrade();
        glib::timeout_add_local(std::time::Duration::from_millis(530), move || {
            let Some(a) = area_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if a.has_focus() {
                let mut st = state.borrow_mut();
                st.caret_visible = !st.caret_visible;
                drop(st);
                a.queue_draw();
            } else {
                let mut st = state.borrow_mut();
                if !st.caret_visible {
                    st.caret_visible = true;
                    drop(st);
                    a.queue_draw();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Right-Click Context Menu Gesture (Button 3)
    {
        let right_click = gtk::GestureClick::new();
        right_click.set_button(3);
        let state_rc = Rc::clone(&state);
        let area_rc = area.clone();
        let pop_rc = context_popover.clone();
        let sig_rc = Rc::clone(&on_signal);
        right_click.connect_pressed(move |_, _, x, y| {
            area_rc.grab_focus();
            // If user right-clicks outside an existing selection, place caret at click point
            {
                let mut st = state_rc.borrow_mut();
                if !st.doc().has_selection() && x >= GUTTER_WIDTH {
                    let pos = pixel_to_doc_pos(&area_rc, &st, x, y);
                    st.doc_mut().cursor = pos;
                    st.doc_mut().preferred_col = pos.col;
                }
            }
            sig_rc(CanvasSignal::RedrawAll);
            pop_rc.set_pointing_to(Some(&gdk::Rectangle::new(
                x.round() as i32,
                y.round() as i32,
                1,
                1,
            )));
            pop_rc.popup();
        });
        area.add_controller(right_click);
    }

    // Left-Click Gesture (single / double / triple / Ctrl+Click multi-cursor / gutter fold / minimap / scrollbar)
    {
        let click = gtk::GestureClick::new();
        click.set_button(1);
        let state_press = Rc::clone(&state);
        let area_ref = area.clone();
        let on_signal = Rc::clone(&on_signal);
        click.connect_pressed(move |gesture, n_press, x, y| {
            area_ref.grab_focus();
            let mods = gesture.current_event_state();
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);

            let mut st = state_press.borrow_mut();
            st.caret_visible = true;
            st.autocomplete_open = false;

            let w = st.last_canvas_w;
            let h = st.last_canvas_h.max(1.0);
            let minimap_w = if st.show_minimap && w > 420.0 {
                MINIMAP_WIDTH
            } else {
                0.0
            };
            let vscroll_x = w - SCROLLBAR_WIDTH;
            let minimap_x = vscroll_x - minimap_w;

            // Vertical scrollbar click
            if x >= vscroll_x {
                let total = st.doc().visible_doc_lines().len().max(1);
                let target = ((y / h).clamp(0.0, 1.0) * (total.saturating_sub(1) as f64)).round()
                    as usize;
                st.doc_mut().scroll_y = target;
                st.dragging_vscroll = Some((y, target));
                drop(st);
                area_ref.queue_draw();
                return;
            }

            // Minimap click
            if minimap_w > 0.0 && x >= minimap_x {
                let total = st.doc().visible_doc_lines().len().max(1);
                let target = ((y / h).clamp(0.0, 1.0) * (total.saturating_sub(1) as f64)).round()
                    as usize;
                st.doc_mut().scroll_y = target;
                st.dragging_minimap = true;
                drop(st);
                area_ref.queue_draw();
                return;
            }

            // Gutter click (toggle fold if clicking chevron, otherwise toggle bookmark)
            if x < GUTTER_WIDTH {
                let pos = pixel_to_doc_pos(&area_ref, &st, GUTTER_WIDTH + 10.0, y);
                if x >= GUTTER_WIDTH - 22.0 && st.doc().foldable_range(pos.line).is_some() {
                    st.doc_mut().toggle_fold_at(pos.line);
                } else {
                    st.doc_mut().cursor.line = pos.line;
                    st.doc_mut().toggle_bookmark();
                }
                drop(st);
                on_signal(CanvasSignal::RedrawAll);
                return;
            }

            let pos = pixel_to_doc_pos(&area_ref, &st, x, y);
            if ctrl {
                st.doc_mut().toggle_extra_cursor(pos);
            } else if n_press == 2 {
                st.doc_mut().clear_extra_cursors();
                st.doc_mut().select_word_at(pos);
            } else if n_press >= 3 {
                st.doc_mut().clear_extra_cursors();
                let line_len = st.doc().line_char_len(pos.line);
                let doc = st.doc_mut();
                doc.anchor = Some(Pos::new(pos.line, 0));
                doc.cursor = Pos::new(pos.line, line_len);
                doc.preferred_col = line_len;
            } else {
                st.doc_mut().clear_extra_cursors();
                let doc = st.doc_mut();
                if shift {
                    if doc.anchor.is_none() {
                        doc.anchor = Some(doc.cursor);
                    }
                    doc.cursor = pos;
                } else {
                    doc.cursor = pos;
                    doc.anchor = Some(pos);
                }
                doc.preferred_col = pos.col;
                st.dragging_selection = true;
            }
            drop(st);
            on_signal(CanvasSignal::RedrawAll);
        });

        let state_rel = Rc::clone(&state);
        click.connect_released(move |_, _, _, _| {
            let mut st = state_rel.borrow_mut();
            st.dragging_selection = false;
            st.dragging_minimap = false;
            st.dragging_vscroll = None;
            if let Some(a) = st.doc().anchor {
                if a == st.doc().cursor {
                    st.doc_mut().anchor = None;
                }
            }
        });
        area.add_controller(click);
    }

    // Motion, Hover Bracket Highlighting & Dragging Selection / Minimap / Scrollbar
    {
        let motion = gtk::EventControllerMotion::new();
        let state_m = Rc::clone(&state);
        let area_m = area.clone();
        let on_signal_m = Rc::clone(&on_signal);
        motion.connect_motion(move |_, x, y| {
            let mut st = state_m.borrow_mut();
            let w = st.last_canvas_w;
            let h = st.last_canvas_h.max(1.0);
            let minimap_w = if st.show_minimap && w > 420.0 {
                MINIMAP_WIDTH
            } else {
                0.0
            };
            let vscroll_x = w - SCROLLBAR_WIDTH;
            let minimap_x = vscroll_x - minimap_w;
            let was_hover = st.hover_vscroll;
            st.hover_vscroll = x >= vscroll_x;

            if st.dragging_vscroll.is_some() || st.dragging_minimap {
                let total = st.doc().visible_doc_lines().len().max(1);
                let target = ((y / h).clamp(0.0, 1.0) * (total.saturating_sub(1) as f64)).round()
                    as usize;
                st.doc_mut().scroll_y = target;
                drop(st);
                area_m.queue_draw();
                return;
            }

            if st.dragging_selection {
                let pos = pixel_to_doc_pos(&area_m, &st, x, y);
                st.doc_mut().cursor = pos;
                st.doc_mut().preferred_col = pos.col;
                drop(st);
                on_signal_m(CanvasSignal::RedrawAll);
                return;
            }

            let mut need_redraw = was_hover != st.hover_vscroll;

            // Track hovered bracket under mouse cursor for instant visual bracket pair highlight!
            let prev_pair = st
                .hover_pos
                .and_then(|hp| st.doc().find_matching_bracket_at(hp));
            if x >= GUTTER_WIDTH && x < minimap_x {
                area_m.set_cursor_from_name(Some("text"));
                let hp = pixel_to_doc_pos(&area_m, &st, x, y);
                let new_pair = st.doc().find_matching_bracket_at(hp);
                st.hover_pos = Some(hp);
                if new_pair != prev_pair {
                    need_redraw = true;
                }
            } else {
                area_m.set_cursor_from_name(Some("default"));
                st.hover_pos = None;
                if prev_pair.is_some() {
                    need_redraw = true;
                }
            }

            if need_redraw {
                drop(st);
                area_m.queue_draw();
            }
        });

        let state_l = Rc::clone(&state);
        let area_l = area.clone();
        motion.connect_leave(move |_| {
            let mut st = state_l.borrow_mut();
            let had_hover = st.hover_pos.take().is_some() || st.hover_vscroll;
            st.hover_vscroll = false;
            if had_hover {
                drop(st);
                area_l.queue_draw();
            }
        });
        area.add_controller(motion);
    }

    // Mouse Wheel Scroll (+ Ctrl+Wheel Zoom!)
    {
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let state = Rc::clone(&state);
        let area_ref = area.clone();
        let on_signal = Rc::clone(&on_signal);
        scroll.connect_scroll(move |ctrl_scroll, dx, dy| {
            let mods = ctrl_scroll.current_event_state();
            let mut st = state.borrow_mut();
            if mods.contains(gdk::ModifierType::CONTROL_MASK) {
                if dy < 0.0 {
                    st.zoom_in();
                } else if dy > 0.0 {
                    st.zoom_out();
                }
                drop(st);
                on_signal(CanvasSignal::RedrawAll);
                return glib::Propagation::Stop;
            }

            let max_y = st.doc().visible_doc_lines().len().saturating_sub(1);
            if dy != 0.0 {
                let delta = (dy * 3.0).round() as isize;
                let cur = st.doc().scroll_y as isize;
                st.doc_mut().scroll_y = (cur + delta).clamp(0, max_y as isize) as usize;
            }
            if dx != 0.0 {
                let delta = (dx * 4.0).round() as isize;
                let cur = st.doc().scroll_x as isize;
                st.doc_mut().scroll_x = (cur + delta).max(0) as usize;
            }
            drop(st);
            area_ref.queue_draw();
            glib::Propagation::Stop
        });
        area.add_controller(scroll);
    }

    // Full Keyboard Controller (IDE shortcuts + Hebrew hotkeys + Multi-Cursor + Vim + Clipboard)
    {
        let key_ctrl = gtk::EventControllerKey::new();
        let state = Rc::clone(&state);
        let area_ref = area.clone();
        let on_signal = Rc::clone(&on_signal);

        key_ctrl.connect_key_pressed(move |_, key, _keycode, mods| {
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            let alt = mods.contains(gdk::ModifierType::ALT_MASK);
            let norm = normalize_hotkey_char(key);

            {
                let mut st = state.borrow_mut();
                st.caret_visible = true;

                // ── Autocomplete navigation ──────────────────────────────────
                if st.autocomplete_open {
                    let cands = st.doc().completion_candidates();
                    if cands.is_empty() {
                        st.autocomplete_open = false;
                    } else {
                        match key {
                            gdk::Key::Escape => {
                                st.autocomplete_open = false;
                                drop(st);
                                area_ref.queue_draw();
                                return glib::Propagation::Stop;
                            }
                            gdk::Key::Down => {
                                let max_c = cands.len().min(8);
                                st.autocomplete_selected = (st.autocomplete_selected + 1) % max_c;
                                drop(st);
                                area_ref.queue_draw();
                                return glib::Propagation::Stop;
                            }
                            gdk::Key::Up => {
                                let max_c = cands.len().min(8);
                                st.autocomplete_selected =
                                    (st.autocomplete_selected + max_c - 1) % max_c;
                                drop(st);
                                area_ref.queue_draw();
                                return glib::Propagation::Stop;
                            }
                            gdk::Key::Tab | gdk::Key::Return | gdk::Key::KP_Enter => {
                                let idx = st.autocomplete_selected % cands.len().min(8);
                                let chosen = cands[idx].clone();
                                st.doc_mut().apply_completion(&chosen);
                                st.autocomplete_open = false;
                                st.ensure_cursor_visible();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            _ => {}
                        }
                    }
                }

                // ── Ctrl / Alt Studio Shortcuts ──────────────────────────────
                if ctrl {
                    if let Some(c) = norm {
                        match c {
                            'n' => {
                                drop(st);
                                on_signal(CanvasSignal::NewTab);
                                return glib::Propagation::Stop;
                            }
                            'o' if !shift => {
                                drop(st);
                                on_signal(CanvasSignal::OpenFile);
                                return glib::Propagation::Stop;
                            }
                            's' if shift => {
                                drop(st);
                                on_signal(CanvasSignal::SaveFileAs);
                                return glib::Propagation::Stop;
                            }
                            's' => {
                                drop(st);
                                on_signal(CanvasSignal::SaveFile);
                                return glib::Propagation::Stop;
                            }
                            'w' => {
                                drop(st);
                                on_signal(CanvasSignal::CloseTab);
                                return glib::Propagation::Stop;
                            }
                            'q' => {
                                drop(st);
                                on_signal(CanvasSignal::Quit);
                                return glib::Propagation::Stop;
                            }
                            'f' => {
                                drop(st);
                                on_signal(CanvasSignal::OpenFind(false));
                                return glib::Propagation::Stop;
                            }
                            'h' => {
                                drop(st);
                                on_signal(CanvasSignal::OpenFind(true));
                                return glib::Propagation::Stop;
                            }
                            'g' => {
                                drop(st);
                                on_signal(CanvasSignal::OpenGoToLine);
                                return glib::Propagation::Stop;
                            }
                            'p' => {
                                drop(st);
                                on_signal(CanvasSignal::OpenCommandPalette);
                                return glib::Propagation::Stop;
                            }
                            'b' => {
                                drop(st);
                                on_signal(CanvasSignal::ToggleSidebar);
                                return glib::Propagation::Stop;
                            }
                            'm' => {
                                st.show_minimap = !st.show_minimap;
                                st.save_session();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            'z' if shift => {
                                st.doc_mut().redo();
                                st.ensure_cursor_visible();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            'z' => {
                                st.doc_mut().undo();
                                st.ensure_cursor_visible();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            'y' => {
                                st.doc_mut().redo();
                                st.ensure_cursor_visible();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            'a' => {
                                st.doc_mut().select_all();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            'd' if shift => {
                                st.doc_mut().duplicate_lines();
                                st.ensure_cursor_visible();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            'd' => {
                                st.doc_mut().add_cursor_next_occurrence();
                                st.ensure_cursor_visible();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            '/' => {
                                let lang = st
                                    .syntax
                                    .detect_language_name(st.doc().path.as_deref(), &st.doc().title);
                                let pfx = SyntaxEngine::comment_prefix(&lang);
                                st.doc_mut().toggle_comment(pfx);
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            'i' if shift => {
                                st.doc_mut().format_document();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            '[' if shift => {
                                let l = st.doc().cursor.line;
                                st.doc_mut().toggle_fold_at(l);
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            ']' if shift => {
                                st.doc_mut().unfold_all();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            '+' => {
                                st.zoom_in();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            '-' => {
                                st.zoom_out();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            '0' => {
                                st.zoom_reset();
                                drop(st);
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            'c' => {
                                let text = st.doc().selected_text().unwrap_or_else(|| {
                                    format!("{}\n", st.doc().lines[st.doc().cursor.line])
                                });
                                drop(st);
                                if let Some(disp) = gdk::Display::default() {
                                    disp.clipboard().set_text(&text);
                                }
                                return glib::Propagation::Stop;
                            }
                            'x' => {
                                let text = if let Some(sel) = st.doc().selected_text() {
                                    st.doc_mut().push_undo(EditKind::BlockOp);
                                    st.doc_mut().delete_selection_internal();
                                    st.doc_mut().dirty = true;
                                    sel
                                } else {
                                    st.doc_mut().delete_current_line()
                                };
                                st.ensure_cursor_visible();
                                drop(st);
                                if let Some(disp) = gdk::Display::default() {
                                    disp.clipboard().set_text(&text);
                                }
                                on_signal(CanvasSignal::RedrawAll);
                                return glib::Propagation::Stop;
                            }
                            'v' => {
                                drop(st);
                                if let Some(disp) = gdk::Display::default() {
                                    let st_cl = Rc::clone(&state);
                                    let sig_cl = Rc::clone(&on_signal);
                                    disp.clipboard().read_text_async(
                                        gtk::gio::Cancellable::NONE,
                                        move |res| {
                                            if let Ok(Some(txt)) = res {
                                                let mut st = st_cl.borrow_mut();
                                                st.doc_mut().insert_str(&txt);
                                                st.ensure_cursor_visible();
                                                drop(st);
                                                sig_cl(CanvasSignal::RedrawAll);
                                            }
                                        },
                                    );
                                }
                                return glib::Propagation::Stop;
                            }
                            ' ' => {
                                st.autocomplete_open = true;
                                st.autocomplete_selected = 0;
                                drop(st);
                                area_ref.queue_draw();
                                return glib::Propagation::Stop;
                            }
                            _ => {}
                        }
                    }

                    // Ctrl+Arrow word navigation / Home / End
                    match key {
                        gdk::Key::Left => {
                            st.doc_mut().move_word_backward(shift);
                            st.ensure_cursor_visible();
                            drop(st);
                            on_signal(CanvasSignal::RedrawAll);
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::Right => {
                            st.doc_mut().move_word_forward(shift);
                            st.ensure_cursor_visible();
                            drop(st);
                            on_signal(CanvasSignal::RedrawAll);
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::Home => {
                            st.doc_mut().go_to_line(1);
                            st.ensure_cursor_visible();
                            drop(st);
                            on_signal(CanvasSignal::RedrawAll);
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::End => {
                            let last = st.doc().lines.len();
                            st.doc_mut().go_to_line(last);
                            st.ensure_cursor_visible();
                            drop(st);
                            on_signal(CanvasSignal::RedrawAll);
                            return glib::Propagation::Stop;
                        }
                        _ => {}
                    }
                    return glib::Propagation::Proceed;
                }

                // Alt+Up/Down: move lines; Alt+Shift+Up/Down: add vertical cursor; Alt+Z: word wrap
                if alt {
                    match key {
                        gdk::Key::Up if shift => {
                            st.doc_mut().add_cursor_vertical(-1);
                            st.ensure_cursor_visible();
                            drop(st);
                            on_signal(CanvasSignal::RedrawAll);
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::Down if shift => {
                            st.doc_mut().add_cursor_vertical(1);
                            st.ensure_cursor_visible();
                            drop(st);
                            on_signal(CanvasSignal::RedrawAll);
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::Up => {
                            st.doc_mut().move_lines_up();
                            st.ensure_cursor_visible();
                            drop(st);
                            on_signal(CanvasSignal::RedrawAll);
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::Down => {
                            st.doc_mut().move_lines_down();
                            st.ensure_cursor_visible();
                            drop(st);
                            on_signal(CanvasSignal::RedrawAll);
                            return glib::Propagation::Stop;
                        }
                        _ if norm == Some('z') => {
                            let cur = st.doc().wrap_lines;
                            st.doc_mut().wrap_lines = !cur;
                            drop(st);
                            on_signal(CanvasSignal::RedrawAll);
                            return glib::Propagation::Stop;
                        }
                        _ => return glib::Propagation::Proceed,
                    }
                }

                // Bookmarks (F2 / Shift+F2)
                if key == gdk::Key::F2 {
                    if shift {
                        st.doc_mut().next_bookmark();
                    } else {
                        st.doc_mut().toggle_bookmark();
                    }
                    st.ensure_cursor_visible();
                    drop(st);
                    on_signal(CanvasSignal::RedrawAll);
                    return glib::Propagation::Stop;
                }

                // Vim Normal / Visual Mode
                if matches!(
                    st.vim_mode,
                    VimMode::Normal | VimMode::Visual | VimMode::VisualLine
                ) {
                    if handle_vim_normal_key(&mut st, key, key.to_unicode(), ctrl) {
                        st.ensure_cursor_visible();
                        drop(st);
                        on_signal(CanvasSignal::RedrawAll);
                        return glib::Propagation::Stop;
                    }
                }

                // Vim Insert Escape -> Normal
                if key == gdk::Key::Escape {
                    if st.vim_mode == VimMode::Insert {
                        st.vim_mode = VimMode::Normal;
                    }
                    st.doc_mut().clear_extra_cursors();
                    st.doc_mut().anchor = None;
                    st.autocomplete_open = false;
                    drop(st);
                    on_signal(CanvasSignal::RedrawAll);
                    return glib::Propagation::Stop;
                }

                // Standard Editing & Navigation Keys
                let mut edited = false;
                let mut handled = true;
                match key {
                    gdk::Key::Left => st.doc_mut().move_left(shift),
                    gdk::Key::Right => st.doc_mut().move_right(shift),
                    gdk::Key::Up => st.doc_mut().move_up(shift),
                    gdk::Key::Down => st.doc_mut().move_down(shift),
                    gdk::Key::Home => st.doc_mut().move_home(shift),
                    gdk::Key::End => st.doc_mut().move_end(shift),
                    gdk::Key::Page_Up => {
                        let step = st.visible_rows().saturating_sub(2).max(1);
                        for _ in 0..step {
                            st.doc_mut().move_up(shift);
                        }
                    }
                    gdk::Key::Page_Down => {
                        let step = st.visible_rows().saturating_sub(2).max(1);
                        for _ in 0..step {
                            st.doc_mut().move_down(shift);
                        }
                    }
                    gdk::Key::BackSpace => {
                        st.doc_mut().backspace();
                        edited = true;
                    }
                    gdk::Key::Delete => {
                        st.doc_mut().delete_forward();
                        edited = true;
                    }
                    gdk::Key::Return | gdk::Key::KP_Enter => {
                        st.doc_mut().insert_newline_auto_indent();
                        st.autocomplete_open = false;
                        edited = true;
                    }
                    gdk::Key::Tab => {
                        st.doc_mut().insert_tab();
                        edited = true;
                    }
                    _ => {
                        if let Some(ch) = key.to_unicode() {
                            if !ch.is_control() {
                                st.doc_mut().insert_char(ch);
                                edited = true;
                            } else {
                                handled = false;
                            }
                        } else {
                            handled = false;
                        }
                    }
                }

                if handled {
                    if edited {
                        let has_prefix = st
                            .doc()
                            .current_word_prefix()
                            .map(|p| p.chars().count() >= 2)
                            .unwrap_or(false);
                        st.autocomplete_open =
                            has_prefix && !st.doc().completion_candidates().is_empty();
                        st.autocomplete_selected = 0;
                    }
                    st.ensure_cursor_visible();
                    drop(st);
                    on_signal(CanvasSignal::RedrawAll);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        area.add_controller(key_ctrl);
    }

    area
}
