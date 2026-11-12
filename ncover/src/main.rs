use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use gio::prelude::*;
use gtk::glib::clone;
use gtk::prelude::*;
use gtk::gdk_pixbuf;
use gtk::{
    glib, Application, ApplicationWindow, Box as GBox, Button, DrawingArea, Entry, HeaderBar,
    Label, ListBox, Orientation, Separator, ToggleButton,
};
use gtk4 as gtk;
use serde::{Deserialize, Serialize};

mod batch;
mod color;
mod composition;
mod palette;
mod raster;
mod samples;
mod svg;

use batch::*;
use color::*;
use composition::*;
use palette::*;
use raster::*;
use samples::*;
use svg::*;

const APP_ID: &str = "io.github.xjmzx.NCover";
const HISTORY_LIMIT: usize = 32;

/// How many of the most-recent history colours become the compact "History
/// palette" — a readable strip you scan at a glance, not the whole 32-deep list.
const HISTORY_PALETTE: usize = 10;

/// How many palettes ride along on the Picker view beside the History palette.
const PINNED_MAX: usize = 3;

/// Swatches per row before a palette wraps. Ten reads as a row you can count at
/// a glance; more and it becomes a wall.
const SWATCHES_PER_ROW: usize = 10;

#[derive(Default, Serialize, Deserialize, Debug)]
struct AppData {
    #[serde(default)]
    format: Format,
    #[serde(default)]
    history: Vec<Rgb>,
    #[serde(default)]
    palettes: Vec<Palette>,
    /// The palettes shown compactly on the Picker view, most-recent first, by
    /// NAME. Capped at [`PINNED_MAX`]. It is both "the last few you used" (using a
    /// palette's swatch moves it to the front) and "the ones you chose" (the
    /// Palettes view pins/unpins explicitly) — one list, so the two never fight.
    #[serde(default)]
    pinned: Vec<String>,
    /// Show the getting-started panel. Defaults TRUE — so a fresh install is
    /// onboarded, and an existing data.json (which has no such field) also gets
    /// it once, which is right: the features it describes are new to them too.
    #[serde(default = "yes")]
    tips: bool,
    /// The output canvas size. One of CANVAS_SIZES; anything else is ignored.
    #[serde(default = "default_canvas")]
    canvas: i32,

    /// Collapsed state per section, so the window comes back the way you left it.
    #[serde(default = "yes")]
    open_history: bool,
    #[serde(default = "yes")]
    open_palettes: bool,
    /// Have the sample files been written? Set ONCE, on the first run that seeds
    /// them.
    ///
    /// This is what makes "delete a sample and it stays deleted" true. Seeding on
    /// every launch would mean the user could never actually get rid of them —
    /// a demo file that keeps coming back is not a demo, it is litter.
    #[serde(default)]
    seeded: bool,
}

fn default_canvas() -> i32 {
    CANVAS_DEFAULT
}

fn yes() -> bool {
    true
}

fn data_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ncover/data.json")
}

fn load_data() -> AppData {
    let path = data_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_data(data: &AppData) -> Result<()> {
    let path = data_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("creating config dir")?;
    }
    let s = serde_json::to_string_pretty(data)?;
    fs::write(&path, s).context("writing data.json")?;
    Ok(())
}

// ---------- picker subprocess ----------

fn find_xcolor() -> Option<PathBuf> {
    // 1. Adjacent to the running binary (for `cargo run` and Makefile install)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let adj = dir.join("xcolor");
            if adj.is_file() {
                return Some(adj);
            }
        }
    }
    // 2. PATH lookup
    if let Ok(path_var) = std::env::var("PATH") {
        for p in path_var.split(':') {
            let cand = Path::new(p).join("xcolor");
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Whether GDK is on Wayland. There the `xcolor` CLI cannot work at all — a
/// rootless XWayland refuses GetImage on the root window (BadMatch) — so the
/// pick has to be the compositor's own, through the Screenshot portal.
#[cfg(target_os = "linux")]
fn on_wayland() -> bool {
    gtk::gdk::Display::default().is_some_and(|d| d.type_().name() == "GdkWaylandDisplay")
}

#[cfg(target_os = "linux")]
fn pick_color_portal<F>(parent: &ApplicationWindow, on_picked: F)
where
    F: Fn(Rgb) + 'static,
{
    use ashpd::desktop::{Color, ResponseError};
    let parent = parent.clone();
    glib::spawn_future_local(async move {
        let picked = async { Color::pick().send().await?.response() }.await;
        match picked {
            Ok(c) => {
                let ch = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                on_picked(Rgb { r: ch(c.red()), g: ch(c.green()), b: ch(c.blue()) });
            }
            // Esc in the compositor's picker: not an error worth a dialog.
            Err(ashpd::Error::Response(ResponseError::Cancelled)) => {}
            Err(e) => show_error(&parent, &format!("Screen pick failed: {e}")),
        }
    });
}

fn pick_color<F>(parent: &ApplicationWindow, on_picked: F)
where
    F: Fn(Rgb) + 'static,
{
    #[cfg(target_os = "linux")]
    if on_wayland() {
        return pick_color_portal(parent, on_picked);
    }
    let Some(bin) = find_xcolor() else {
        show_error(parent, "xcolor binary not found in PATH");
        return;
    };
    let proc = match gio::Subprocess::newv(
        &[
            bin.as_os_str(),
            std::ffi::OsStr::new("-f"),
            std::ffi::OsStr::new("hex"),
        ],
        gio::SubprocessFlags::STDOUT_PIPE,
    ) {
        Ok(p) => p,
        Err(e) => {
            show_error(parent, &format!("Failed to launch xcolor: {e}"));
            return;
        }
    };
    let parent = parent.clone();
    proc.communicate_utf8_async(
        None::<String>,
        gio::Cancellable::NONE,
        move |res| match res {
            Ok((Some(stdout), _)) => match Rgb::from_hex(&stdout) {
                Some(c) => on_picked(c),
                None => show_error(
                    &parent,
                    &format!("Could not parse picker output: {}", stdout.trim()),
                ),
            },
            Ok(_) => show_error(&parent, "Picker returned no output"),
            Err(e) => show_error(&parent, &format!("Picker failed: {e}")),
        },
    );
}

fn show_error(parent: &ApplicationWindow, msg: &str) {
    let dlg = gtk::AlertDialog::builder()
        .message("n.cover")
        .detail(msg)
        .modal(true)
        .build();
    dlg.show(Some(parent));
}

// ---------- state ----------

struct State {
    data: AppData,
    current: Option<Rgb>,
    swatch: DrawingArea,
    code_label: Label,
    fmt_hex: ToggleButton,
    fmt_rgb: ToggleButton,
    fmt_hsl: ToggleButton,
    palettes_list: ListBox,
    /// The compact strips on the Picker view: History palette + pinned palettes.
    pinned_box: GBox,
}

type SharedState = Rc<RefCell<State>>;

fn copy_to_clipboard(window: &ApplicationWindow, text: &str) {
    let display = gtk::prelude::WidgetExt::display(window);
    let clip: gtk::gdk::Clipboard = display.clipboard();
    clip.set_text(text);
}

fn refresh_swatch(state: &State) {
    state.swatch.queue_draw();
}

fn refresh_code(state: &State) {
    let txt = match state.current {
        Some(c) => c.format(state.data.format),
        None => "(no color picked)".to_string(),
    };
    state.code_label.set_text(&txt);
}

fn refresh_format_toggles(state: &State) {
    state.fmt_hex.set_active(state.data.format == Format::Hex);
    state.fmt_rgb.set_active(state.data.format == Format::Rgb);
    state.fmt_hsl.set_active(state.data.format == Format::Hsl);
}

fn refresh_history_ui(state: &State, window: &ApplicationWindow, shared: &SharedState) {
    // History no longer has a section of its own — it lives as the compact
    // History palette in the Palettes area. "Refreshing history" is rebuilding
    // that strip. The name stays so every caller reads the same.
    refresh_pinned_ui(state, window, shared);
}

fn refresh_palettes_ui(state: &State, window: &ApplicationWindow, shared: &SharedState) {
    while let Some(child) = state.palettes_list.first_child() {
        state.palettes_list.remove(&child);
    }
    for (idx, pal) in state.data.palettes.iter().enumerate() {
        let row = build_palette_row(pal.clone(), idx, window, shared);
        state.palettes_list.append(&row);
    }
}

/// Both palette surfaces at once. Anything that changes the palettes OR the pins
/// must call this, or the Palettes view and the Picker's compact strips drift.
fn refresh_palettes_all(state: &State, window: &ApplicationWindow, shared: &SharedState) {
    refresh_palettes_ui(state, window, shared);
    refresh_pinned_ui(state, window, shared);
}

/// Is this palette currently pinned to the Picker view?
fn is_pinned(data: &AppData, name: &str) -> bool {
    data.pinned.iter().any(|n| n == name)
}

/// Move a palette to the front of the pinned list (most-recent), capped. Used
/// both by an explicit pin and by "you just used this palette".
fn pin_to_front(data: &mut AppData, name: &str) {
    data.pinned.retain(|n| n != name);
    data.pinned.insert(0, name.to_string());
    data.pinned.truncate(PINNED_MAX);
}

/// Toggle a pin. Returns the new state. Pinning caps at [`PINNED_MAX`], dropping
/// the oldest — the Picker view has room for a few, not a library.
fn toggle_pin(data: &mut AppData, name: &str) -> bool {
    if let Some(pos) = data.pinned.iter().position(|n| n == name) {
        data.pinned.remove(pos);
        false
    } else {
        pin_to_front(data, name);
        true
    }
}

/// Rebuild the Picker view's compact strips: the History palette, then the
/// pinned palettes (resolved by name, newest first). This is the small,
/// scan-at-a-glance surface; the full editable list lives in the Palettes view.
fn refresh_pinned_ui(state: &State, window: &ApplicationWindow, shared: &SharedState) {
    while let Some(child) = state.pinned_box.first_child() {
        state.pinned_box.remove(&child);
    }

    let hist: Vec<Rgb> = state
        .data
        .history
        .iter()
        .take(HISTORY_PALETTE)
        .copied()
        .collect();
    if hist.is_empty() {
        let empty = Label::new(Some("Pick a colour — your recent colours collect here as a palette."));
        empty.add_css_class("dim-label");
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        state.pinned_box.append(&empty);
    } else {
        // The History strip is the compact strip plus a Clear — the only edit it
        // gets, and the home for the clear that used to live in the old History
        // section's header.
        let strip = GBox::new(Orientation::Vertical, 3);
        strip.set_margin_top(4);
        strip.set_margin_bottom(4);
        let head = GBox::new(Orientation::Horizontal, 6);
        let lbl = Label::new(Some(&format!("History  ·  {}", hist.len())));
        lbl.add_css_class("dim-label");
        lbl.set_xalign(0.0);
        lbl.set_hexpand(true);
        head.append(&lbl);
        let clear = Button::from_icon_name("edit-clear-symbolic");
        clear.add_css_class("flat");
        clear.set_tooltip_text(Some("Clear the colour history"));
        clear.connect_clicked(clone!(
            #[strong]
            shared,
            #[weak]
            window,
            move |_| {
                {
                    let mut s = shared.borrow_mut();
                    s.data.history.clear();
                    let _ = save_data(&s.data);
                }
                let s = shared.borrow();
                refresh_history_ui(&s, &window, &shared);
            }
        ));
        head.append(&clear);
        strip.append(&head);
        strip.append(&build_swatch_flow(&hist, state.data.format, window, shared));
        state.pinned_box.append(&strip);
    }

    let mut shown = 0;
    for name in &state.data.pinned {
        if shown >= PINNED_MAX {
            break;
        }
        if let Some(p) = state.data.palettes.iter().find(|p| &p.name == name) {
            let cols: Vec<Rgb> = p.colors.iter().map(|s| s.rgb).collect();
            let title = if p.name.is_empty() {
                "(unnamed)".to_string()
            } else {
                p.name.clone()
            };
            state.pinned_box.append(&build_compact_strip(
                &title,
                &cols,
                state.data.format,
                window,
                shared,
            ));
            shown += 1;
        }
    }

    if shown == 0 && !hist.is_empty() {
        let hint = Label::new(Some("Pin palettes in the Palettes view to keep them here."));
        hint.add_css_class("dim-label");
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        state.pinned_box.append(&hint);
    }
}

/// A wrapping row of small square swatches. Clicking a square selects and copies
/// that colour — the same act as clicking a swatch anywhere else.
fn build_swatch_flow(
    colors: &[Rgb],
    fmt: Format,
    window: &ApplicationWindow,
    shared: &SharedState,
) -> gtk::FlowBox {
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    // Ten to a row, then wrap; squares touch. NOT homogeneous — homogeneous
    // stretches each cell to fill the width, which turns the squares into
    // gap-separated rectangles. Non-homogeneous cells hug the 22px chip, so with
    // zero spacing and zero child padding the squares sit edge to edge.
    flow.set_max_children_per_line(SWATCHES_PER_ROW as u32);
    flow.set_column_spacing(0);
    flow.set_row_spacing(0);
    flow.set_homogeneous(false);
    // Fill, NOT Start: a Start-aligned FlowBox shrink-wraps to its minimum width
    // (one column), which stacks every swatch vertically. Fill gives it the
    // column's width so it lays out left-to-right and wraps at ten. Non-
    // homogeneous keeps the squares square; the leftover width sits to the right.
    flow.set_halign(gtk::Align::Fill);
    flow.add_css_class("swatch-flow");
    for color in colors {
        let c = *color;
        let chip = DrawingArea::new();
        chip.set_size_request(22, 22);
        chip.set_draw_func(move |_, cr, w, h| {
            cr.set_source_rgb(c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0);
            cr.rectangle(0.0, 0.0, w as f64, h as f64);
            let _ = cr.fill();
        });
        chip.set_tooltip_text(Some(&format!("{} — click to select + copy", c.format(fmt))));
        let click = gtk::GestureClick::new();
        click.connect_pressed(clone!(
            #[strong]
            shared,
            #[weak]
            window,
            move |_, _, _, _| {
                let mut s = shared.borrow_mut();
                s.current = Some(c);
                refresh_swatch(&s);
                refresh_code(&s);
                copy_to_clipboard(&window, &c.format(s.data.format));
            }
        ));
        chip.add_controller(click);
        flow.insert(&chip, -1);
    }
    flow
}

/// A compact palette: a title over the swatch flow. Read-mostly — no edit
/// affordances, that is the Palettes view's job.
fn build_compact_strip(
    title: &str,
    colors: &[Rgb],
    fmt: Format,
    window: &ApplicationWindow,
    shared: &SharedState,
) -> GBox {
    let strip = GBox::new(Orientation::Vertical, 3);
    strip.set_margin_top(4);
    strip.set_margin_bottom(4);

    let head = Label::new(Some(&format!("{title}  ·  {}", colors.len())));
    head.add_css_class("dim-label");
    head.set_xalign(0.0);
    strip.append(&head);
    strip.append(&build_swatch_flow(colors, fmt, window, shared));
    strip
}

fn build_palette_row(
    pal: Palette,
    idx: usize,
    window: &ApplicationWindow,
    shared: &SharedState,
) -> GBox {
    let row = GBox::new(Orientation::Vertical, 4);
    row.set_margin_top(4);
    row.set_margin_bottom(4);
    row.set_margin_start(6);
    row.set_margin_end(6);

    let header = GBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some(&format!(
        "{} ({})",
        if pal.name.is_empty() {
            "(unnamed)"
        } else {
            &pal.name
        },
        pal.colors.len()
    )));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("heading");
    header.append(&title);

    // Pin toggle — decides whether this palette rides along on the Picker view.
    // Reflects the current state so it is a status as much as a control.
    let pin_name = pal.name.clone();
    let pin_btn = ToggleButton::new();
    pin_btn.set_icon_name("view-pin-symbolic");
    pin_btn.set_active(is_pinned(&shared.borrow().data, &pin_name));
    pin_btn.set_tooltip_text(Some(&format!(
        "Pin to the Picker view (keeps up to {PINNED_MAX} beside History)"
    )));
    pin_btn.connect_toggled(clone!(
        #[strong]
        shared,
        #[weak]
        window,
        move |_| {
            {
                let mut s = shared.borrow_mut();
                toggle_pin(&mut s.data, &pin_name);
                let _ = save_data(&s.data);
            }
            // Rebuilds the rows (so every pin toggle reflects the new cap) and
            // the compact strips together.
            let s = shared.borrow();
            refresh_palettes_all(&s, &window, &shared);
        }
    ));
    header.append(&pin_btn);

    let add_current = Button::from_icon_name("list-add-symbolic");
    add_current.set_tooltip_text(Some("Add current color"));
    add_current.connect_clicked(clone!(
        #[strong]
        shared,
        #[weak]
        window,
        move |_| {
            {
                let mut s = shared.borrow_mut();
                let Some(c) = s.current else {
                    return;
                };
                if let Some(p) = s.data.palettes.get_mut(idx) {
                    if !p.colors.iter().any(|s| s.rgb == c) {
                        p.colors.push(Swatch::new(c));
                    }
                }
                let _ = save_data(&s.data);
            }
            let s = shared.borrow();
            refresh_palettes_all(&s, &window, &shared);
        }
    ));
    header.append(&add_current);

    let export_btn = Button::from_icon_name("document-save-symbolic");
    export_btn.set_tooltip_text(Some("Export palette"));
    export_btn.connect_clicked(clone!(
        #[strong]
        shared,
        #[weak]
        window,
        move |_| {
            export_palette(&window, &shared, idx);
        }
    ));
    header.append(&export_btn);

    let del_btn = Button::from_icon_name("user-trash-symbolic");
    del_btn.set_tooltip_text(Some("Delete palette"));
    del_btn.connect_clicked(clone!(
        #[strong]
        shared,
        #[weak]
        window,
        move |_| {
            {
                let mut s = shared.borrow_mut();
                if idx < s.data.palettes.len() {
                    let removed = s.data.palettes.remove(idx);
                    // A deleted palette can no longer be pinned — drop it, or the
                    // Picker view would try to resolve a name that is gone.
                    s.data.pinned.retain(|n| n != &removed.name);
                }
                let _ = save_data(&s.data);
            }
            let s = shared.borrow();
            refresh_palettes_all(&s, &window, &shared);
        }
    ));
    header.append(&del_btn);

    row.append(&header);

    let pal_name = pal.name.clone();
    // Same layout as the compact strips: ten per row, no gap, wrap. A palette
    // should read the same on both surfaces.
    let chips = gtk::FlowBox::new();
    chips.set_selection_mode(gtk::SelectionMode::None);
    chips.set_max_children_per_line(SWATCHES_PER_ROW as u32);
    chips.set_column_spacing(0);
    chips.set_row_spacing(0);
    chips.set_homogeneous(false);
    // Fill so it wraps at ten across the width; Start would collapse it to a
    // single vertical column. See build_swatch_flow.
    chips.set_halign(gtk::Align::Fill);
    chips.add_css_class("swatch-flow");
    for (cidx, swatch) in pal.colors.iter().enumerate() {
        let color = &swatch.rgb;
        let chip = DrawingArea::new();
        chip.set_size_request(22, 22);
        let c = *color;
        chip.set_draw_func(move |_, cr, w, h| {
            cr.set_source_rgb(c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0);
            cr.rectangle(0.0, 0.0, w as f64, h as f64);
            let _ = cr.fill();
        });
        let click = gtk::GestureClick::new();
        click.set_button(0); // any button
        let pal_name = pal_name.clone();
        click.connect_pressed(clone!(
            #[strong]
            shared,
            #[weak]
            window,
            move |g, _, _, _| {
                let btn = g.current_button();
                if btn == 3 {
                    // right-click removes
                    {
                        let mut s = shared.borrow_mut();
                        if let Some(p) = s.data.palettes.get_mut(idx) {
                            if cidx < p.colors.len() {
                                p.colors.remove(cidx);
                            }
                        }
                        let _ = save_data(&s.data);
                    }
                    let s = shared.borrow();
                    refresh_palettes_all(&s, &window, &shared);
                } else {
                    // Using an UNPINNED palette brings it onto the Picker view —
                    // "the last few you used". Already-pinned ones do not reorder,
                    // to keep the strip from shuffling under every click.
                    let newly_pinned = {
                        let mut s = shared.borrow_mut();
                        s.current = Some(c);
                        refresh_swatch(&s);
                        refresh_code(&s);
                        copy_to_clipboard(&window, &c.format(s.data.format));
                        if !pal_name.is_empty() && !is_pinned(&s.data, &pal_name) {
                            pin_to_front(&mut s.data, &pal_name);
                            let _ = save_data(&s.data);
                            true
                        } else {
                            false
                        }
                    };
                    if newly_pinned {
                        let s = shared.borrow();
                        refresh_palettes_all(&s, &window, &shared);
                    }
                }
            }
        ));
        chip.add_controller(click);
        chip.set_tooltip_text(Some(&match &swatch.name {
            Some(n) => format!("{n} — {} (left: select+copy, right: remove)", c.hex()),
            None => format!("{} (left: select+copy, right: remove)", c.hex()),
        }));
        chips.insert(&chip, -1);
    }
    row.append(&chips);
    row.append(&Separator::new(Orientation::Horizontal));
    row
}

/// Read a palette off disk and add it. The app could always *export* and never
/// *import*, so a palette that left the app could never come back — the reason
/// the suite's theme palettes had to be hand-written into data.json.
fn import_palette(window: &ApplicationWindow, shared: &SharedState) {
    let dlg = gtk::FileDialog::builder()
        .title("Import palette (.gpl / .json)")
        .build();
    dlg.open(
        Some(window),
        gio::Cancellable::NONE,
        clone!(
            #[weak]
            window,
            #[strong]
            shared,
            move |res| {
                let Ok(file) = res else { return };
                let Some(path) = file.path() else { return };
                match read_palette(&path) {
                    Ok(mut pal) => {
                        {
                            let mut s = shared.borrow_mut();
                            // Don't silently merge into a same-named palette —
                            // suffix instead, so an import never mutates
                            // something the user already had.
                            let taken: Vec<String> =
                                s.data.palettes.iter().map(|p| p.name.clone()).collect();
                            if taken.contains(&pal.name) {
                                let base = pal.name.clone();
                                let mut n = 2;
                                while taken.contains(&format!("{base} ({n})")) {
                                    n += 1;
                                }
                                pal.name = format!("{base} ({n})");
                            }
                            s.data.palettes.push(pal);
                            let _ = save_data(&s.data);
                        }
                        let s = shared.borrow();
                        refresh_palettes_all(&s, &window, &shared);
                    }
                    Err(e) => show_error(&window, &format!("Import failed: {e}")),
                }
            }
        ),
    );
}

/// The image section: open, view, pick a pixel, extract a palette.
/// The getting-started panel. Each line names a FEATURE and the sample file that
/// demonstrates it — a tour with nothing to click through is just a wall of text.
fn build_tips(window: &ApplicationWindow, toggle: &ToggleButton) -> GBox {
    let b = GBox::new(Orientation::Vertical, 6);
    b.add_css_class("tips-card");

    let head = GBox::new(Orientation::Horizontal, 8);
    let t = Label::new(Some("Quick start"));
    t.add_css_class("heading");
    t.set_xalign(0.0);
    t.set_hexpand(true);
    head.append(&t);
    // Just a close [x] — the panel always comes back from the [!] toggle, so
    // "Don't show again" was overstating it.
    let close = Button::from_icon_name("window-close-symbolic");
    close.add_css_class("flat");
    close.set_tooltip_text(Some("Hide — reopen any time with the [!] button"));
    head.append(&close);
    b.append(&head);

    // A bullet, and a small sub-heading, built the same terse way.
    let bullet = |b: &GBox, text: &str| {
        let l = Label::new(Some(&format!("•  {text}")));
        l.set_xalign(0.0);
        l.set_wrap(true);
        l.set_max_width_chars(48);
        l.add_css_class("dim-label");
        b.append(&l);
    };
    let subhead = |b: &GBox, text: &str| {
        let l = Label::new(Some(text));
        l.set_xalign(0.0);
        l.set_margin_top(4);
        l.add_css_class("section-head");
        b.append(&l);
    };

    // What it does — one line each, no prose.
    for line in [
        "Pick any colour on screen → history + clipboard.",
        "Image: Open a file, or Browse a folder (◀ ▶ / arrows).",
        "Click any pixel in an image to pick it.",
        "Build: Blank, Invert, stamp Squares, mask to a Disc — steps stack.",
        "Palettes: recent colours form a History palette; pin up to 3.",
        "Batch: one recipe over a folder or ndisc's published releases.",
    ] {
        bullet(&b, line);
    }

    // Formats & limits — the compatibility list, and the things it will NOT do.
    subhead(&b, "Formats & limits");
    for line in [
        "Opens: PNG · SVG · JPEG · WebP.",
        "Saves: PNG only.",
        "SVG: reads its declared colours, fonts, text.",
        "Canvas: square, 200–1000 px.",
        "Batch never writes your source — Preview first.",
    ] {
        bullet(&b, line);
    }

    let row = GBox::new(Orientation::Horizontal, 6);
    let open_dir = Button::with_label("Open samples folder");
    let restore = Button::with_label("Restore samples");
    restore.set_tooltip_text(Some(
        "Rewrite any sample you have deleted. Existing files are left alone.",
    ));
    row.append(&open_dir);
    row.append(&restore);
    b.append(&row);

    open_dir.connect_clicked(clone!(
        #[weak]
        window,
        move |_| {
            let dir = samples_dir();
            if let Err(e) = fs::create_dir_all(&dir) {
                show_error(&window, &format!("Could not create {}: {e}", dir.display()));
                return;
            }
            let f = gio::File::for_path(&dir);
            let launcher = gtk::FileLauncher::new(Some(&f));
            launcher.launch(Some(&window), gio::Cancellable::NONE, |_| {});
        }
    ));
    restore.connect_clicked(clone!(
        #[weak]
        window,
        move |_| match write_samples() {
            Ok(0) => show_error(&window, "All samples are already there."),
            Ok(n) => show_error(&window, &format!("Restored {n} sample file(s).")),
            Err(e) => show_error(&window, &format!("Could not write samples: {e}")),
        }
    ));
    // "Don't show again" un-presses the header toggle rather than hiding the
    // panel behind its back — one piece of state, so the button in the header
    // can never disagree with what is on screen.
    close.connect_clicked(clone!(
        #[weak]
        toggle,
        move |_| toggle.set_active(false)
    ));

    b
}

/// The batch section: a recipe, a folder, and a run you can watch and stop.
fn build_batch(
    window: &ApplicationWindow,
    shared: &SharedState,
    canvas: Rc<Cell<i32>>,
    size_dd: &gtk::DropDown,
) -> gtk::Expander {
    let src: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let dst: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    // ndisc's manifest, loaded when you switch to that scope and reused at run,
    // so the count you saw and the set you process are the same read.
    let manifest: Rc<RefCell<Option<Manifest>>> = Rc::new(RefCell::new(None));

    let body = GBox::new(Orientation::Vertical, 8);
    body.set_margin_top(6);

    // --- scope: a folder, or ndisc's published discography ---
    // The published option is only offered when ndisc has actually exported a
    // manifest. A scope that can only ever match nothing is worse than no scope.
    let has_manifest = published_manifest_path().is_some();
    let scope_names: &[&str] = if has_manifest {
        &["Folder", "Published discography"]
    } else {
        &["Folder"]
    };
    let row_scope = GBox::new(Orientation::Horizontal, 6);
    let scope_lbl = Label::new(Some("Scope"));
    scope_lbl.add_css_class("dim-label");
    row_scope.append(&scope_lbl);
    let dd_scope = gtk::DropDown::from_strings(scope_names);
    dd_scope.set_tooltip_text(Some(
        "Folder — any tree of artwork.\n\nPublished discography — the exact releases ndisc has published to Nostr, read from the suite's published.json. Its covers, mirrored to your output folder.",
    ));
    row_scope.append(&dd_scope);
    row_scope.set_hexpand(true);
    // Only worth a row when there is a choice to make.
    row_scope.set_visible(has_manifest);
    body.append(&row_scope);

    // --- folders ---
    let row_src = GBox::new(Orientation::Horizontal, 6);
    let b_src = Button::with_label("Source…");
    b_src.set_tooltip_text(Some("A folder of artwork. Searched recursively."));
    let l_src = Label::new(Some("no source folder"));
    l_src.add_css_class("dim-label");
    l_src.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    l_src.set_xalign(0.0);
    l_src.set_hexpand(true);
    row_src.append(&b_src);
    row_src.append(&l_src);
    body.append(&row_src);

    let row_dst = GBox::new(Orientation::Horizontal, 6);
    let b_dst = Button::with_label("Output…");
    b_dst.set_tooltip_text(Some(
        "Where the results go. Never the source folder — your originals are not touched.",
    ));
    let l_dst = Label::new(Some("no output folder"));
    l_dst.add_css_class("dim-label");
    l_dst.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    l_dst.set_xalign(0.0);
    l_dst.set_hexpand(true);
    row_dst.append(&b_dst);
    row_dst.append(&l_dst);
    body.append(&row_dst);

    // --- recipe ---
    let row_r = GBox::new(Orientation::Horizontal, 6);

    let e_match = gtk::Entry::new();
    e_match.set_text("cover");
    e_match.set_width_chars(10);
    e_match.set_placeholder_text(Some("name filter"));
    e_match.set_tooltip_text(Some(
        "Only files whose name contains this. \u{201c}cover\u{201d} catches cover.png / cover.jpg across a music library. Empty = every image in the tree.",
    ));
    row_r.append(&e_match);

    let dd_frame = gtk::DropDown::from_strings(&["Cover", "Fit"]);
    dd_frame.set_tooltip_text(Some(
        "Cover fills the square (edges cropped); Fit puts the whole image inside it.\n\nThese are the only framings that mean anything across a folder — a dragged placement is about ONE image's dimensions.",
    ));
    row_r.append(&dd_frame);

    let dd_disc = gtk::DropDown::from_strings(&["No disc", "Alpha", "White", "Colour", "Gradient"]);
    dd_disc.set_tooltip_text(Some(
        "Disc mask + corner fill. Colour/Gradient take the colours you have picked, exactly as the single-image Disc buttons do.",
    ));
    row_r.append(&dd_disc);

    let size_note = Label::new(None);
    size_note.add_css_class("dim-label");
    size_note.set_hexpand(true);
    size_note.set_xalign(1.0);
    row_r.append(&size_note);
    body.append(&row_r);

    // --- run ---
    let row_go = GBox::new(Orientation::Horizontal, 6);
    let b_preview = Button::with_label("Preview");
    b_preview.set_tooltip_text(Some(
        "Render the first few outputs and show them as thumbnails — SEE what the recipe does before it writes anything.",
    ));
    let b_dry = Button::with_label("Dry run");
    b_dry.set_tooltip_text(Some(
        "List what WOULD be written, writing nothing. Every image is still opened and framed, so a file that would fail, fails here.",
    ));
    let b_run = Button::with_label("Run");
    b_run.add_css_class("suggested-action");
    let b_stop = Button::with_label("Stop");
    b_stop.set_sensitive(false);
    row_go.append(&b_preview);
    row_go.append(&b_dry);
    row_go.append(&b_run);
    row_go.append(&b_stop);
    let l_prog = Label::new(None);
    l_prog.add_css_class("dim-label");
    l_prog.set_hexpand(true);
    l_prog.set_xalign(1.0);
    row_go.append(&l_prog);
    body.append(&row_go);

    // The preview contact sheet — hidden until you ask for one.
    let preview_flow = gtk::FlowBox::new();
    preview_flow.set_selection_mode(gtk::SelectionMode::None);
    preview_flow.set_max_children_per_line(6);
    preview_flow.set_row_spacing(6);
    preview_flow.set_column_spacing(6);
    let preview_scroll = gtk::ScrolledWindow::new();
    preview_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    preview_scroll.set_min_content_height(160);
    preview_scroll.set_max_content_height(340);
    preview_scroll.set_child(Some(&preview_flow));
    preview_scroll.set_visible(false);
    body.append(&preview_scroll);

    let results = ListBox::new();
    results.set_selection_mode(gtk::SelectionMode::None);
    results.add_css_class("boxed-list");
    let res_scroll = gtk::ScrolledWindow::new();
    res_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    res_scroll.set_min_content_height(90);
    res_scroll.set_max_content_height(240);
    res_scroll.set_child(Some(&results));
    body.append(&res_scroll);

    // Folder pickers.
    for (btn, slot, lbl, title) in [
        (&b_src, &src, &l_src, "Source folder"),
        (&b_dst, &dst, &l_dst, "Output folder"),
    ] {
        btn.connect_clicked(clone!(
            #[weak]
            window,
            #[strong(rename_to = slot)]
            slot.clone(),
            #[weak(rename_to = lbl)]
            lbl.clone(),
            #[strong(rename_to = title)]
            title.to_string(),
            move |_| {
                let dlg = gtk::FileDialog::builder().title(&title).build();
                dlg.select_folder(
                    Some(&window),
                    gio::Cancellable::NONE,
                    clone!(
                        #[strong]
                        slot,
                        #[weak]
                        lbl,
                        move |res| {
                            let Ok(f) = res else { return };
                            let Some(p) = f.path() else { return };
                            lbl.set_text(&p.display().to_string());
                            *slot.borrow_mut() = Some(p);
                        }
                    ),
                );
            }
        ));
    }

    // Switching scope swaps what the source row means. Folder → the picker is
    // live. Published → it is replaced by a read-only readout of the manifest
    // (count + library root), because there is nothing to pick: the set is
    // ndisc's, not yours. Loaded HERE so a broken manifest is a plain message
    // now, not a surprise at run.
    if has_manifest {
        dd_scope.connect_selected_notify(clone!(
            #[strong]
            manifest,
            #[strong]
            src,
            #[weak]
            b_src,
            #[weak]
            l_src,
            move |dd| {
                if dd.selected() == 1 {
                    let loaded = published_manifest_path()
                        .ok_or_else(|| anyhow::anyhow!("published.json is gone"))
                        .and_then(|p| load_manifest(&p));
                    match loaded {
                        Ok(m) => {
                            l_src.set_text(&format!(
                                "{} releases · {}",
                                m.releases.len(),
                                m.library_root.display()
                            ));
                            b_src.set_sensitive(false);
                            *manifest.borrow_mut() = Some(m);
                        }
                        Err(e) => {
                            l_src.set_text(&format!("{e:#}"));
                            b_src.set_sensitive(false);
                            *manifest.borrow_mut() = None;
                        }
                    }
                } else {
                    *manifest.borrow_mut() = None;
                    // Clear the folder too, so the readout and the state agree —
                    // a "no source folder" label over a still-set path is a lie.
                    *src.borrow_mut() = None;
                    b_src.set_sensitive(true);
                    l_src.set_text("no source folder");
                }
            }
        ));
    }

    // The recipe's size is the canvas size — one control, not a second copy of
    // it that can disagree with what you are looking at.
    // Watching the SAME dropdown, rather than holding a copy of the size that
    // could drift out of step with the canvas you are looking at.
    //
    // The size is read from the DROPDOWN, not from `canvas` — GTK runs handlers
    // in connection order, and this one is connected before the handler that
    // writes `canvas`, so reading the cell here would render the size you just
    // moved away from.
    let note_from = |sel: u32| {
        let c = sane_canvas(
            CANVAS_SIZES
                .get(sel as usize)
                .copied()
                .unwrap_or(CANVAS_DEFAULT),
        );
        format!("\u{2192} {c}\u{d7}{c} PNG")
    };
    size_note.set_text(&note_from(size_dd.selected()));
    size_dd.connect_selected_notify(clone!(
        #[weak(rename_to = note)]
        size_note,
        move |dd| note.set_text(&note_from(dd.selected()))
    ));

    // Where the images come from — folder or manifest — with the errors shown.
    // Shared by Run and Preview so they can never disagree about the set.
    let resolve_scope: Rc<dyn Fn() -> Option<Scope>> = {
        let window = window.clone();
        let dd_scope = dd_scope.clone();
        let manifest = manifest.clone();
        let src = src.clone();
        Rc::new(move || {
            if dd_scope.selected() == 1 {
                match manifest.borrow().as_ref() {
                    Some(m) => Some(Scope::published(m)),
                    None => {
                        show_error(&window, "The published discography could not be read. Re-select the scope, or point ndisc at Export.");
                        None
                    }
                }
            } else {
                match src.borrow().clone() {
                    Some(s) => Some(Scope::folder(s)),
                    None => {
                        show_error(&window, "Choose a source folder.");
                        None
                    }
                }
            }
        })
    };

    // The recipe as currently dialled in. The disc fill resolves from the picked
    // colours ONCE here, so every file in a run gets the same fill even if you
    // pick while it runs — and a Preview shows exactly that fill.
    let resolve_recipe: Rc<dyn Fn() -> Option<Recipe>> = {
        let window = window.clone();
        let shared = shared.clone();
        let canvas = canvas.clone();
        let dd_frame = dd_frame.clone();
        let dd_disc = dd_disc.clone();
        Rc::new(move || {
            let (cur, prev) = {
                let st = shared.borrow();
                (
                    st.current.or_else(|| st.data.history.first().copied()),
                    st.data.history.get(1).copied().or(st.current),
                )
            };
            let disc = match dd_disc.selected() {
                0 => None,
                1 => Some(OuterFill::Alpha),
                2 => Some(OuterFill::White),
                3 => match cur {
                    Some(c) => Some(OuterFill::Solid(c)),
                    None => {
                        show_error(&window, "Pick a colour first — that is the fill.");
                        return None;
                    }
                },
                _ => match (cur, prev) {
                    (Some(i), Some(o)) => Some(OuterFill::Gradient { inner: i, outer: o }),
                    _ => {
                        show_error(&window, "A gradient needs two colours — pick a second one.");
                        return None;
                    }
                },
            };
            Some(Recipe {
                canvas: canvas.get(),
                framing: if dd_frame.selected() == 1 {
                    Framing::Fit
                } else {
                    Framing::Cover
                },
                disc,
            })
        })
    };

    let run = {
        let window = window.clone();
        let dst = dst.clone();
        let stop = stop.clone();
        let e_match = e_match.clone();
        let resolve_scope = resolve_scope.clone();
        let resolve_recipe = resolve_recipe.clone();
        let results = results.clone();
        let l_prog = l_prog.clone();
        let b_dry = b_dry.clone();
        let b_run = b_run.clone();
        let b_stop = b_stop.clone();
        Rc::new(move |dry: bool| {
            let Some(o_root) = dst.borrow().clone() else {
                show_error(&window, "Choose an output folder.");
                return;
            };
            let Some(scope) = resolve_scope() else { return };
            if let Err(e) = guard_batch(&scope.guard_root, &o_root) {
                show_error(&window, &format!("{e}"));
                return;
            }
            let Some(recipe) = resolve_recipe() else { return };
            let needle = e_match.text().to_string();

            while let Some(c) = results.first_child() {
                results.remove(&c);
            }
            l_prog.set_text("scanning…");
            stop.store(false, Ordering::Relaxed);
            b_dry.set_sensitive(false);
            b_run.set_sensitive(false);
            b_stop.set_sensitive(true);

            let (tx, rx) = async_channel::unbounded::<BatchMsg>();
            let worker_stop = stop.clone();
            let Scope { roots, strip, .. } = scope;
            std::thread::spawn(move || {
                // Discovery is on the worker too: walking a music library is
                // thousands of directories (one per release, under published
                // scope), and a frozen window during the "scanning…" step is
                // still a frozen window.
                let mut files = Vec::new();
                for root in &roots {
                    if let Err(e) = find_images(root, &needle, &mut files) {
                        // One unreadable release dir is not fatal to the run.
                        let _ = tx.send_blocking(BatchMsg::One {
                            rel: root.display().to_string(),
                            result: Err(format!("{e}")),
                        });
                    }
                }
                // Release dirs are disjoint and a folder is walked once, so this
                // is belt-and-braces — but a duplicate would be a wasted write.
                files.sort();
                files.dedup();
                let _ = tx.send_blocking(BatchMsg::Total(files.len()));

                let (mut ok, mut failed) = (0usize, 0usize);
                let mut stopped = false;
                for f in files {
                    if worker_stop.load(Ordering::Relaxed) {
                        stopped = true;
                        break;
                    }
                    let rel = f.strip_prefix(&strip).unwrap_or(&f).to_path_buf();
                    let result = batch_one(&f, &rel, &o_root, recipe, dry)
                        .map_err(|e| format!("{e:#}")); // {:#} = the whole context chain
                    match &result {
                        Ok(_) => ok += 1,
                        Err(_) => failed += 1,
                    }
                    let _ = tx.send_blocking(BatchMsg::One {
                        rel: rel.display().to_string(),
                        result,
                    });
                }
                let _ = tx.send_blocking(BatchMsg::Done { ok, failed, stopped });
            });

            glib::spawn_future_local(clone!(
                #[weak]
                results,
                #[weak]
                l_prog,
                #[weak]
                b_dry,
                #[weak]
                b_run,
                #[weak]
                b_stop,
                async move {
                    let mut total = 0usize;
                    let mut seen = 0usize;
                    let mut failed = 0usize;
                    let mut rows = 0usize;
                    while let Ok(msg) = rx.recv().await {
                        match msg {
                            BatchMsg::Total(n) => {
                                total = n;
                                if n == 0 {
                                    l_prog.set_text("nothing matched");
                                }
                            }
                            BatchMsg::One { rel, result } => {
                                seen += 1;
                                let bad = result.is_err();
                                if bad {
                                    failed += 1;
                                }
                                // Failures ALWAYS get a row. Successes get one
                                // until the list is full — you need to see what a
                                // dry run would write, but you do not need to read
                                // 12,000 of them.
                                if bad || rows < RESULT_ROWS {
                                    let l = Label::new(Some(&match &result {
                                        Ok(d) => format!(
                                            "{}  →  {}",
                                            rel,
                                            d.file_name()
                                                .and_then(|s| s.to_str())
                                                .unwrap_or("?")
                                        ),
                                        Err(e) => format!("{rel}  —  {e}"),
                                    }));
                                    l.set_xalign(0.0);
                                    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                                    l.set_margin_start(6);
                                    l.set_margin_end(6);
                                    if bad {
                                        l.add_css_class("error");
                                    } else {
                                        l.add_css_class("dim-label");
                                    }
                                    results.append(&l);
                                    rows += 1;
                                }
                                if seen % 16 == 0 || seen == total {
                                    l_prog.set_text(&format!(
                                        "{seen} / {total}{}",
                                        if failed > 0 {
                                            format!(" · {failed} failed")
                                        } else {
                                            String::new()
                                        }
                                    ));
                                }
                            }
                            BatchMsg::Done { ok, failed, stopped } => {
                                let verb = if stopped { "stopped" } else { "done" };
                                l_prog.set_text(&format!(
                                    "{verb} — {ok} {}{}",
                                    if dry { "would be written" } else { "written" },
                                    if failed > 0 {
                                        format!(" · {failed} failed")
                                    } else {
                                        String::new()
                                    }
                                ));
                                // Say what the list is NOT showing. A quiet cap
                                // reads as "that was everything".
                                if ok > rows.saturating_sub(failed) {
                                    let l = Label::new(Some(&format!(
                                        "… and {} more (list caps at {RESULT_ROWS}; every failure is shown)",
                                        ok - rows.saturating_sub(failed)
                                    )));
                                    l.set_xalign(0.0);
                                    l.add_css_class("dim-label");
                                    l.set_margin_start(6);
                                    results.append(&l);
                                }
                                b_dry.set_sensitive(true);
                                b_run.set_sensitive(true);
                                b_stop.set_sensitive(false);
                                break;
                            }
                        }
                    }
                }
            ));
        })
    };

    b_dry.connect_clicked(clone!(
        #[strong]
        run,
        move |_| run(true)
    ));
    b_run.connect_clicked(clone!(
        #[strong]
        run,
        move |_| run(false)
    ));
    b_stop.connect_clicked(clone!(
        #[strong]
        stop,
        move |_| stop.store(true, Ordering::Relaxed)
    ));

    // Preview: render the first PREVIEW_N outputs and show them as a contact
    // sheet. Nothing is written — this is the "see it before it can mess anything
    // up" pass, and for a big scope it is the difference between catching a wrong
    // recipe and discovering it in 500 files.
    let preview = {
        let e_match = e_match.clone();
        let resolve_scope = resolve_scope.clone();
        let resolve_recipe = resolve_recipe.clone();
        let preview_flow = preview_flow.clone();
        let preview_scroll = preview_scroll.clone();
        let l_prog = l_prog.clone();
        let b_preview = b_preview.clone();
        Rc::new(move || {
            let Some(scope) = resolve_scope() else { return };
            let Some(recipe) = resolve_recipe() else { return };
            let needle = e_match.text().to_string();

            while let Some(c) = preview_flow.first_child() {
                preview_flow.remove(&c);
            }
            preview_scroll.set_visible(true);
            l_prog.set_text("rendering preview…");
            b_preview.set_sensitive(false);

            let (tx, rx) = async_channel::unbounded::<PreviewMsg>();
            let Scope { roots, strip, .. } = scope;
            std::thread::spawn(move || {
                let mut files = Vec::new();
                for root in &roots {
                    let _ = find_images(root, &needle, &mut files);
                }
                files.sort();
                files.dedup();
                let _ = tx.send_blocking(PreviewMsg::Total(files.len()));
                // Render only the first N. Pixbuf work off the main thread is
                // fine — it is image data, never GTK — and only the RGBA bytes
                // (Send) cross back.
                for f in files.iter().take(PREVIEW_N) {
                    let label = f.strip_prefix(&strip).unwrap_or(f).display().to_string();
                    let msg = match batch_render(f, recipe)
                        .and_then(|pb| {
                            pb.scale_simple(PREVIEW_PX, PREVIEW_PX, gdk_pixbuf::InterpType::Bilinear)
                                .context("could not scale the thumbnail")
                        }) {
                        Ok(t) => PreviewMsg::Thumb {
                            label,
                            thumb: Some((
                                unsafe { t.pixels() }.to_vec(),
                                t.width(),
                                t.height(),
                                t.rowstride(),
                                t.has_alpha(),
                            )),
                            err: None,
                        },
                        Err(e) => PreviewMsg::Thumb {
                            label,
                            thumb: None,
                            err: Some(format!("{e:#}")),
                        },
                    };
                    let _ = tx.send_blocking(msg);
                }
                let _ = tx.send_blocking(PreviewMsg::Done);
            });

            glib::spawn_future_local(clone!(
                #[weak]
                preview_flow,
                #[weak]
                l_prog,
                #[weak]
                b_preview,
                async move {
                    let mut total = 0usize;
                    let mut shown = 0usize;
                    let mut failed = 0usize;
                    while let Ok(msg) = rx.recv().await {
                        match msg {
                            PreviewMsg::Total(n) => {
                                total = n;
                                if n == 0 {
                                    l_prog.set_text("nothing matched");
                                }
                            }
                            PreviewMsg::Thumb { label, thumb, err } => {
                                shown += 1;
                                let cell = GBox::new(Orientation::Vertical, 2);
                                match thumb {
                                    Some((bytes, w, h, rowstride, has_alpha)) => {
                                        let pb = gdk_pixbuf::Pixbuf::from_bytes(
                                            &glib::Bytes::from_owned(bytes),
                                            gdk_pixbuf::Colorspace::Rgb,
                                            has_alpha,
                                            8,
                                            w,
                                            h,
                                            rowstride,
                                        );
                                        let tex = gtk::gdk::Texture::for_pixbuf(&pb);
                                        let pic = gtk::Picture::for_paintable(&tex);
                                        pic.set_size_request(PREVIEW_PX, PREVIEW_PX);
                                        pic.add_css_class("frame");
                                        cell.append(&pic);
                                    }
                                    None => {
                                        failed += 1;
                                        let x = Label::new(Some("render failed"));
                                        x.add_css_class("error");
                                        x.set_size_request(PREVIEW_PX, PREVIEW_PX);
                                        x.set_wrap(true);
                                        x.set_tooltip_text(err.as_deref());
                                        cell.append(&x);
                                    }
                                }
                                let cap = Label::new(Some(&label));
                                cap.add_css_class("dim-label");
                                cap.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                                cap.set_max_width_chars(18);
                                cap.set_tooltip_text(Some(&label));
                                cell.append(&cap);
                                preview_flow.insert(&cell, -1);
                            }
                            PreviewMsg::Done => {
                                let more = total.saturating_sub(shown);
                                l_prog.set_text(&format!(
                                    "preview: {shown} of {total}{}{}",
                                    if failed > 0 {
                                        format!(" · {failed} failed")
                                    } else {
                                        String::new()
                                    },
                                    if more > 0 {
                                        format!(" · {more} more not shown")
                                    } else {
                                        String::new()
                                    }
                                ));
                                b_preview.set_sensitive(true);
                                break;
                            }
                        }
                    }
                }
            ));
        })
    };
    b_preview.connect_clicked(clone!(
        #[strong]
        preview,
        move |_| preview()
    ));

    let head = GBox::new(Orientation::Horizontal, 8);
    let t = Label::new(Some("Batch"));
    t.add_css_class("section-head");
    t.set_xalign(0.0);
    t.set_hexpand(true);
    head.append(&t);
    let hint = Label::new(Some("apply this recipe to a folder — Preview first"));
    hint.add_css_class("dim-label");
    head.append(&hint);
    head.set_hexpand(true);

    let exp = gtk::Expander::new(None);
    exp.set_label_widget(Some(&head));
    exp.set_child(Some(&body));
    exp.set_expanded(false);
    exp
}

/// Which of the two the big canvas is showing. The right rail flips between the
/// raw Source and the built Result so you can compare what you loaded against
/// what you have made.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Source,
    Result,
}

/// Side of a rail thumbnail, px.
const THUMB: i32 = 140;

fn build_image_view(window: &ApplicationWindow, shared: &SharedState) -> GBox {
    // The document: the loaded source, where it sits, and every build step on
    // it (see composition.rs). None until something is opened. It changes
    // ONLY through `mutate`, below.
    let doc: Rc<RefCell<Option<Composition>>> = Rc::new(RefCell::new(None));
    // `render_raster(doc)` while the document has build steps, else None. A
    // cache of the document, written only by `mutate` — never state of its
    // own. The area draws it in preference to the live-placed source, so the
    // disc IS the preview: you are looking at the thing you would save.
    let result: Rc<RefCell<Option<gdk_pixbuf::Pixbuf>>> = Rc::new(RefCell::new(None));
    let name: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    // Set only for SVG. Its presence is what makes the file's DECLARED colours
    // available instead of quantised pixels.
    let svg: Rc<RefCell<Option<SvgInfo>>> = Rc::new(RefCell::new(None));
    let grid_on: Rc<RefCell<bool>> = Rc::new(RefCell::new(true));
    // The output size, live. Everything that draws or exports reads it here
    // rather than from a constant.
    let canvas: Rc<Cell<i32>> = Rc::new(Cell::new(sane_canvas(shared.borrow().data.canvas)));
    // Which guides fired on the last move — drawn so you can SEE why the image
    // stopped. A snap you cannot see is just a bug.
    let snapped: Rc<RefCell<(bool, bool)>> = Rc::new(RefCell::new((false, false)));
    // Which the big canvas is showing. Result by default — with no build applied
    // the result IS the framed source, so it starts honest.
    let view: Rc<Cell<ViewMode>> = Rc::new(Cell::new(ViewMode::Result));
    // The path the current image came from — the target for Overwrite. None for a
    // Blank canvas (nothing to overwrite) or before anything is loaded.
    let src_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    // Pointer position over the canvas (widget coords), or None when it has left.
    // Drives the crosshair guides + the position/colour readout.
    let hover: Rc<Cell<Option<(f64, f64)>>> = Rc::new(Cell::new(None));

    let box_ = GBox::new(Orientation::Vertical, 8);

    let header = GBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some("Image"));
    title.add_css_class("heading");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let file_lbl = Label::new(Some("no image"));
    file_lbl.add_css_class("dim-label");
    file_lbl.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    file_lbl.set_max_width_chars(28);
    header.append(&file_lbl);

    // Undo / redo. Bound to the window actions (defined with the history,
    // below), so they grey out exactly when Ctrl+Z would do nothing.
    let undo_box = GBox::new(Orientation::Horizontal, 0);
    undo_box.add_css_class("linked");
    for (icon, action, tip) in [
        ("edit-undo-symbolic", "win.undo", "Undo (Ctrl+Z)"),
        ("edit-redo-symbolic", "win.redo", "Redo (Ctrl+Shift+Z)"),
    ] {
        let b = Button::from_icon_name(icon);
        b.set_action_name(Some(action));
        b.set_tooltip_text(Some(tip));
        undo_box.append(&b);
    }
    header.append(&undo_box);

    let open_btn = Button::with_label("Open");
    open_btn.set_tooltip_text(Some("Open an image (PNG / SVG / JPEG / WebP)"));
    header.append(&open_btn);

    let blank_btn = Button::with_label("Blank");
    blank_btn.set_tooltip_text(Some(
        "Start from a blank white canvas — build a label from nothing but squares (Invert for a black ground)",
    ));
    header.append(&blank_btn);

    let browse_btn = Button::with_label("Browse…");
    browse_btn.set_tooltip_text(Some(
        "Open a FOLDER and scroll through every image in it (◀ ▶ or the arrow keys) — a cover directory, a release collection.",
    ));
    header.append(&browse_btn);

    let pal_btn = Button::with_label("Palette from image");
    pal_btn.set_tooltip_text(Some(
        "Extract the image's dominant colours as a new palette",
    ));
    pal_btn.set_sensitive(false);
    header.append(&pal_btn);
    box_.append(&header);

    // ---- placement controls -----------------------------------------------
    let ctl = GBox::new(Orientation::Horizontal, 6);
    let sizes: Vec<String> = CANVAS_SIZES.iter().map(|n| format!("{n}×{n}")).collect();
    let size_dd = gtk::DropDown::from_strings(&sizes.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    size_dd.set_selected(
        CANVAS_SIZES
            .iter()
            .position(|n| *n == canvas.get())
            .unwrap_or(1) as u32,
    );
    size_dd.set_tooltip_text(Some(
        "Output size. Changing it keeps your framing — the square around the picture grows, the picture does not move.",
    ));
    ctl.append(&size_dd);

    let b_grid = ToggleButton::with_label("Grid");
    b_grid.set_active(true);
    b_grid.set_tooltip_text(Some("Thirds + centre cross. A guide — never exported."));
    ctl.append(&b_grid);

    // What the transparent parts are seen against. A viewing condition, the
    // same category as the grid: it is drawn by the canvas and must never
    // reach `render_raster`, or Save would write it into the file.
    let backdrop: Rc<Cell<Backdrop>> = Rc::new(Cell::new(Backdrop::Checker));
    let backdrop_dd = gtk::DropDown::from_strings(&["Checker", "Grey", "Black", "White", "Colour"]);
    backdrop_dd.set_tooltip_text(Some(
        "Preview background, so transparency can be judged against what the artwork will sit on. Never written to the file. Colour = the current colour, taken when you choose it.",
    ));
    ctl.append(&backdrop_dd);

    let b_fit = Button::with_label("Fit");
    b_fit.set_tooltip_text(Some("Whole image inside the canvas"));
    let b_cover = Button::with_label("Cover");
    b_cover.set_tooltip_text(Some("Fill the canvas — no gaps"));
    let b_centre = Button::with_label("Centre");
    b_centre.set_tooltip_text(Some("Centre without changing the zoom"));
    let b_11 = Button::with_label("1:1");
    b_11.set_tooltip_text(Some("Actual pixels"));
    for b in [&b_fit, &b_cover, &b_centre, &b_11] {
        b.set_sensitive(false);
        ctl.append(b);
    }
    b_grid.set_sensitive(false);
    ctl.set_visible(false);
    box_.append(&ctl);

    // ---- browse nav: only shown while scrolling a folder --------------------
    let nav = GBox::new(Orientation::Horizontal, 6);
    let b_prev = Button::from_icon_name("go-previous-symbolic");
    b_prev.set_tooltip_text(Some("Previous image (←)"));
    let b_next = Button::from_icon_name("go-next-symbolic");
    b_next.set_tooltip_text(Some("Next image (→)"));
    let nav_lbl = Label::new(None);
    nav_lbl.add_css_class("dim-label");
    nav_lbl.set_hexpand(true);
    nav_lbl.set_xalign(0.0);
    nav_lbl.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    nav.append(&b_prev);
    nav.append(&b_next);
    nav.append(&nav_lbl);
    nav.set_visible(false);
    box_.append(&nav);

    // The browse list and where we are in it. A plain Vec of paths so the SAME
    // nav can later be fed by the published manifest's covers, not only a folder.
    let browse: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
    let browse_idx: Rc<Cell<usize>> = Rc::new(Cell::new(0));

    // 400x400 is the floor, not the size — it expands with the window.
    // The two rail thumbnails, created before the draw func so the canvas can
    // ask them to redraw whenever it does — which means every state change
    // reaches them for free, with no plumbing through each handler.
    let src_thumb = gtk::DrawingArea::new();
    let res_thumb = gtk::DrawingArea::new();
    for t in [&src_thumb, &res_thumb] {
        t.set_content_width(THUMB);
        t.set_content_height(THUMB);
        t.add_css_class("frame");
    }

    let area = gtk::DrawingArea::new();
    area.set_content_width(400);
    area.set_content_height(400);
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.add_css_class("frame");

    // Created here, laid out in the Disc row below: `mutate` keeps them honest.
    let b_save = Button::with_label("Save PNG…");
    b_save.set_sensitive(false);
    // Overwrite writes the result back over the ORIGINAL file — only offered when
    // that file is a PNG (the format we save), so it is never a lossy surprise.
    let b_overwrite = Button::with_label("Overwrite");
    b_overwrite.add_css_class("destructive-action");
    b_overwrite.set_sensitive(false);

    // Undo / redo. Window actions, so the header buttons and Ctrl+Z share one
    // enabled state that cannot disagree with itself.
    let history: Rc<RefCell<History>> = Rc::new(RefCell::new(History::default()));
    let act_undo = gio::SimpleAction::new("undo", None);
    let act_redo = gio::SimpleAction::new("redo", None);
    act_undo.set_enabled(false);
    act_redo.set_enabled(false);
    window.add_action(&act_undo);
    window.add_action(&act_redo);
    // The keys are a BUBBLE-phase controller on the window, not application
    // accels. `set_accels_for_action` fires before the focused widget sees
    // the key, so Ctrl+Z typed into the batch filter undid a canvas step
    // instead of the typing. Bubbling lets a text field handle its own undo
    // first; only a key nothing below wanted reaches the document.
    let keys = gtk::ShortcutController::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Bubble);
    for (trigger, action) in [
        ("<Control>z", "win.undo"),
        ("<Control><Shift>z", "win.redo"),
        ("<Control>y", "win.redo"),
    ] {
        keys.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(trigger),
            Some(gtk::NamedAction::new(action)),
        ));
    }
    window.add_controller(keys);
    let sync_history: Rc<dyn Fn()> = {
        let history = history.clone();
        let act_undo = act_undo.clone();
        let act_redo = act_redo.clone();
        Rc::new(move || {
            let h = history.borrow();
            act_undo.set_enabled(h.can_undo());
            act_redo.set_enabled(h.can_redo());
        })
    };

    // Put a document on screen: render it, and only if that succeeds make it
    // THE document. Records nothing — `mutate` is the edit path, and undo /
    // redo / opening a file call this directly.
    let show_doc: Rc<dyn Fn(Composition) -> bool> = {
        let doc = doc.clone();
        let result = result.clone();
        let view = view.clone();
        let area = area.clone();
        let window = window.clone();
        let src_path = src_path.clone();
        let b_save = b_save.clone();
        let b_overwrite = b_overwrite.clone();
        let canvas = canvas.clone();
        let size_dd = size_dd.clone();
        let shared = shared.clone();
        Rc::new(move |next: Composition| {
            let built = next.has_build();
            let rendered = if built {
                match render_raster(&next) {
                    Ok(pb) => Some(pb),
                    Err(e) => {
                        let step = next.ops.last().map_or("Build", |op| op.label());
                        show_error(&window, &format!("{step} failed: {e}"));
                        return false;
                    }
                }
            } else {
                None
            };
            // The size picker follows the document, so undoing a size change
            // puts the picker back too. `canvas` is set FIRST: the picker's
            // handler then sees no change and does not edit again.
            if next.canvas != canvas.get() {
                canvas.set(next.canvas);
                if let Some(i) = CANVAS_SIZES.iter().position(|n| *n == next.canvas) {
                    size_dd.set_selected(i as u32);
                }
                let mut s = shared.borrow_mut();
                s.data.canvas = next.canvas;
                let _ = save_data(&s.data);
            }
            *doc.borrow_mut() = Some(next);
            *result.borrow_mut() = rendered;
            b_save.set_sensitive(built);
            // Overwrite only when there is a result AND the original is a PNG.
            b_overwrite.set_sensitive(built && src_is_png(&src_path.borrow()));
            if built {
                // You just built something — show it. Editing while looking at
                // the raw source would be invisible.
                view.set(ViewMode::Result);
            }
            area.queue_draw();
            true
        })
    };

    // The ONE way the document is edited. Callers clone the current value,
    // edit the copy, and hand it here with the gesture it belongs to ("drag",
    // "zoom") or None for a discrete edit. Anything that writes `doc` without
    // coming through here is a bug: the preview would not follow it, and it
    // would not be undoable.
    let mutate: Rc<dyn Fn(Composition, Option<&'static str>)> = {
        let doc = doc.clone();
        let history = history.clone();
        let show_doc = show_doc.clone();
        let sync_history = sync_history.clone();
        Rc::new(move |next: Composition, group: Option<&'static str>| {
            let prev = doc.borrow().clone();
            if prev.as_ref() == Some(&next) {
                return; // Centre when already centred is not a step
            }
            if show_doc(next) {
                if let Some(p) = prev {
                    history.borrow_mut().record(p, group, std::time::Instant::now());
                }
                sync_history();
            }
        })
    };

    // Undo and redo swap documents; a failed render (which should not happen,
    // since each of these rendered once already) leaves the stacks as they were.
    for (act, back) in [(&act_undo, true), (&act_redo, false)] {
        act.connect_activate(clone!(
            #[strong]
            doc,
            #[strong]
            history,
            #[strong]
            show_doc,
            #[strong]
            sync_history,
            move |_, _| {
                let Some(cur) = doc.borrow().clone() else { return };
                let step = {
                    let mut h = history.borrow_mut();
                    if back { h.undo(cur.clone()) } else { h.redo(cur.clone()) }
                };
                if let Some(target) = step {
                    if !show_doc(target.clone()) {
                        let mut h = history.borrow_mut();
                        let _ = if back { h.redo(target) } else { h.undo(target) };
                    }
                }
                sync_history();
            }
        ));
    }

    area.set_draw_func(clone!(
        #[strong]
        canvas,
        #[strong]
        doc,
        #[strong]
        result,
        #[strong]
        grid_on,
        #[strong]
        snapped,
        #[strong]
        view,
        #[strong]
        hover,
        #[strong]
        backdrop,
        #[weak]
        src_thumb,
        #[weak]
        res_thumb,
        move |_, cr, w, h| {
            let (w, h) = (w as f64, h as f64);
            // The canvas is the source of truth for what is on screen; whenever it
            // repaints, nudge the thumbnails to repaint too. Queuing from inside a
            // draw just marks the next frame — it does not recurse.
            src_thumb.queue_draw();
            res_thumb.queue_draw();

            // Read the ViewMode and pointer NOW: `view` gets shadowed further down
            // by the fitted() scale factor, so grab what we need up front.
            let vmode = view.get();
            let hov = hover.get();

            // Checkerboard: what is transparent must LOOK transparent, or a
            // white logo on nothing reads as a white logo on white.
            let sq = 12.0;
            cr.set_source_rgb(0.16, 0.16, 0.17);
            let _ = cr.paint();
            cr.set_source_rgb(0.21, 0.21, 0.22);
            let mut y = 0.0;
            let mut row = 0;
            while y < h {
                let mut x = if row % 2 == 0 { 0.0 } else { sq };
                while x < w {
                    cr.rectangle(x, y, sq, sq);
                    x += sq * 2.0;
                }
                y += sq;
                row += 1;
            }
            let _ = cr.fill();

            // Source view: the raw file, fit into the widget at its own aspect —
            // no canvas, no grid. A plain look at what you loaded.
            let src = doc_src(&doc.borrow());
            let place = doc_place(&doc.borrow());
            if vmode == ViewMode::Source {
                if let Some(pb) = src.clone() {
                    let (ox, oy, sc) = fitted(pb.width() as f64, pb.height() as f64, w, h);
                    cr.save().ok();
                    cr.translate(ox, oy);
                    cr.scale(sc, sc);
                    paint_backdrop(cr, backdrop.get(), 0.0, 0.0, pb.width() as f64, pb.height() as f64);
                    cr.set_source_pixbuf(&pb, 0.0, 0.0);
                    if sc > 1.0 {
                        cr.source().set_filter(gtk::cairo::Filter::Nearest);
                    }
                    let _ = cr.paint();
                    cr.restore().ok();
                }
                // Crosshair over the source view too.
                if let Some((px, py)) = hov {
                    let info = pixel_under(
                        px, py, w, h, vmode, canvas.get(),
                        &result.borrow(), &src, place,
                    );
                    draw_crosshair(cr, w, h, px, py, info);
                }
                return;
            }

            // The CANVAS is what is fitted into the widget — not the image. The
            // image lives on the canvas, and may hang off its edges.
            let cv = canvas.get();
            let (ox, oy, view) = fitted(cv as f64, cv as f64, w, h);
            cr.save().ok();
            cr.translate(ox, oy);
            cr.scale(view, view);

            // Everything is clipped to the canvas: what falls outside the 400x400
            // is not in the output, so it must not be in the preview either.
            cr.rectangle(0.0, 0.0, cv as f64, cv as f64);
            cr.clip();
            paint_backdrop(cr, backdrop.get(), 0.0, 0.0, cv as f64, cv as f64);

            if let Some(pb) = result.borrow().clone() {
                // Build steps applied: their render IS the result, drawn as-is.
                cr.set_source_pixbuf(&pb, 0.0, 0.0);
                if view > 1.0 {
                    cr.source().set_filter(gtk::cairo::Filter::Nearest);
                }
                let _ = cr.paint();
            } else if let Some(pb) = src {
                let p = place;
                cr.save().ok();
                cr.translate(p.dx, p.dy);
                cr.scale(p.scale, p.scale);
                cr.set_source_pixbuf(&pb, 0.0, 0.0);
                // Nearest when magnified: a colour tool must not invent colours.
                if p.scale * view > 1.0 {
                    cr.source().set_filter(gtk::cairo::Filter::Nearest);
                }
                let _ = cr.paint();
                cr.restore().ok();
            }

            // Grid — thirds, plus an emphasised centre cross. Drawn OVER the
            // image (it is a guide, not part of the picture) and never exported.
            if *grid_on.borrow() && result.borrow().is_none() {
                let c = cv as f64;
                cr.set_line_width(1.0 / view);
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.18);
                for i in 1..3 {
                    let t = c * i as f64 / 3.0;
                    cr.move_to(t, 0.0);
                    cr.line_to(t, c);
                    cr.move_to(0.0, t);
                    cr.line_to(c, t);
                }
                let _ = cr.stroke();
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
                cr.move_to(c / 2.0, 0.0);
                cr.line_to(c / 2.0, c);
                cr.move_to(0.0, c / 2.0);
                cr.line_to(c, c / 2.0);
                let _ = cr.stroke();
            }

            // Snap guides — cyan, only while a guide is actually holding.
            let (sx, sy) = *snapped.borrow();
            if (sx || sy) && result.borrow().is_none() {
                let c = cv as f64;
                cr.set_source_rgba(0.2, 0.9, 1.0, 0.9);
                cr.set_line_width(1.5 / view);
                if sx {
                    cr.move_to(c / 2.0, 0.0);
                    cr.line_to(c / 2.0, c);
                }
                if sy {
                    cr.move_to(0.0, c / 2.0);
                    cr.line_to(c, c / 2.0);
                }
                let _ = cr.stroke();
            }

            cr.restore().ok();

            // Canvas border — the 400x400 bound, so you can see what you are
            // composing INTO.
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
            cr.set_line_width(1.0);
            cr.rectangle(ox + 0.5, oy + 0.5, cv as f64 * view - 1.0, cv as f64 * view - 1.0);
            let _ = cr.stroke();

            // Crosshair on top of everything — guides + coordinate/colour readout.
            if let Some((px, py)) = hov {
                let info = pixel_under(
                    px, py, w, h, vmode, cv,
                    &result.borrow(), &doc_src(&doc.borrow()), place,
                );
                draw_crosshair(cr, w, h, px, py, info);
            }
        }
    ));

    // A crosshair cursor over the canvas — this is a place for precise work, and
    // an arrow does not say that.
    area.set_cursor_from_name(Some("crosshair"));

    // Track the pointer for the crosshair guides + readout.
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(clone!(
        #[strong]
        hover,
        #[weak]
        area,
        move |_, x, y| {
            hover.set(Some((x, y)));
            area.queue_draw();
        }
    ));
    motion.connect_leave(clone!(
        #[strong]
        hover,
        #[weak]
        area,
        move |_| {
            hover.set(None);
            area.queue_draw();
        }
    ));
    area.add_controller(motion);

    // Click a pixel -> it becomes the current colour, exactly as a screen pick
    // does, so it flows into the history and the palettes. Uses the SAME mapping
    // as the hover readout, so what the crosshair shows is what a click picks.
    let click = gtk::GestureClick::new();
    click.connect_pressed(clone!(
        #[strong]
        doc,
        #[strong]
        result,
        #[strong]
        canvas,
        #[strong]
        view,
        #[strong]
        shared,
        #[weak]
        window,
        #[weak]
        area,
        move |_, _, px, py| {
            let (w, h) = (area.width() as f64, area.height() as f64);
            let Some((_, _, c)) = pixel_under(
                px, py, w, h, view.get(), canvas.get(),
                &result.borrow(), &doc_src(&doc.borrow()), doc_place(&doc.borrow()),
            ) else {
                return; // clicked the checkerboard / outside the canvas
            };
            {
                let mut s = shared.borrow_mut();
                s.current = Some(c);
                // Identical to the screen picker's path — a pick is a pick.
                s.data.history.retain(|x| *x != c);
                s.data.history.insert(0, c);
                s.data.history.truncate(HISTORY_LIMIT);
                let _ = save_data(&s.data);
            }
            let s = shared.borrow();
            refresh_swatch(&s);
            refresh_code(&s);
            refresh_history_ui(&s, &window, &shared);
            copy_to_clipboard(&window, &c.format(s.data.format));
        }
    ));
    area.add_controller(click);

    // Drag to reposition. The click gesture above still picks — GTK routes a
    // press to both, and a drag that never moves is just a click.
    let drag = gtk::GestureDrag::new();
    let start: Rc<RefCell<Placement>> = Rc::new(RefCell::new(Placement {
        dx: 0.0,
        dy: 0.0,
        scale: 1.0,
    }));
    drag.connect_drag_begin(clone!(
        #[strong]
        doc,
        #[strong]
        start,
        #[strong]
        history,
        move |_, _, _| {
            *start.borrow_mut() = doc_place(&doc.borrow());
            // A new drag is a new undo step, even straight after another.
            history.borrow_mut().break_group();
        }
    ));
    drag.connect_drag_update(clone!(
        #[strong]
        canvas,
        #[strong]
        doc,
        #[strong]
        mutate,
        #[strong]
        start,
        #[strong]
        snapped,
        #[strong]
        view,
        #[weak]
        area,
        move |_, ox, oy| {
            let Some(mut d) = doc.borrow().clone() else { return };
            if d.has_build() {
                return; // build steps applied — reset to move it again
            }
            // Framing is a Result-view act; if you were eyeing the Source, drag
            // snaps you back to what you are actually changing.
            view.set(ViewMode::Result);
            let src = d.source.raster.clone();
            // Widget pixels -> canvas pixels. Without dividing by the view scale
            // the image would race ahead of (or lag) the cursor.
            let cv = canvas.get();
            let (_, _, view) = fitted(
                cv as f64,
                cv as f64,
                area.width() as f64,
                area.height() as f64,
            );
            let s0 = *start.borrow();
            let want = Placement {
                dx: s0.dx + ox / view,
                dy: s0.dy + oy / view,
                scale: s0.scale,
            };
            let (p, sx, sy) = snap(want, src.width(), src.height(), cv);
            *snapped.borrow_mut() = (sx, sy);
            d.set_placement(p);
            mutate(d, Some("drag"));
        }
    ));
    drag.connect_drag_end(clone!(
        #[strong]
        snapped,
        #[weak]
        area,
        move |_, _, _| {
            *snapped.borrow_mut() = (false, false);
            area.queue_draw();
        }
    ));
    area.add_controller(drag);

    // Scroll to zoom, about the canvas centre.
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    scroll.connect_scroll(clone!(
        #[strong]
        canvas,
        #[strong]
        doc,
        #[strong]
        mutate,
        #[strong]
        view,
        move |_, _, dy| {
            let Some(mut d) = doc.borrow().clone() else {
                return glib::Propagation::Proceed;
            };
            if d.has_build() {
                return glib::Propagation::Proceed;
            }
            view.set(ViewMode::Result);
            let mut p = d.placement();
            let c = canvas.get() as f64 / 2.0;
            // Keep the canvas centre fixed under the zoom, so the thing you are
            // looking at does not fly off.
            let old = p.scale;
            let new = (old * if dy < 0.0 { 1.1 } else { 1.0 / 1.1 }).clamp(0.02, 20.0);
            p.dx = c - (c - p.dx) * (new / old);
            p.dy = c - (c - p.dy) * (new / old);
            p.scale = new;
            d.set_placement(p);
            mutate(d, Some("zoom"));
            glib::Propagation::Stop
        }
    ));
    area.add_controller(scroll);

    // "Colour" takes the current colour when it is chosen, and keeps it: a
    // backdrop that changed under you every time you picked would be a
    // backdrop you could not compare against. Choose Colour again to update.
    backdrop_dd.connect_selected_notify(clone!(
        #[strong]
        backdrop,
        #[strong]
        shared,
        #[weak]
        window,
        #[weak]
        area,
        move |dd| {
            let b = match dd.selected() {
                1 => Backdrop::Grey,
                2 => Backdrop::Black,
                3 => Backdrop::White,
                4 => {
                    let cur = {
                        let s = shared.borrow();
                        s.current.or_else(|| s.data.history.first().copied())
                    };
                    match cur {
                        Some(c) => Backdrop::Colour(c),
                        None => {
                            show_error(&window, "Pick a colour first — that is the backdrop.");
                            dd.set_selected(0);
                            return;
                        }
                    }
                }
                _ => Backdrop::Checker,
            };
            backdrop.set(b);
            area.queue_draw(); // cascades to the Result thumbnail
        }
    ));

    b_grid.connect_toggled(clone!(
        #[strong]
        grid_on,
        #[weak]
        area,
        move |b| {
            *grid_on.borrow_mut() = b.is_active();
            area.queue_draw();
        }
    ));

    // Fit / Cover / Centre / 1:1 — the four framings you actually reach for.
    let reframe = {
        let doc = doc.clone();
        let mutate = mutate.clone();
        let canvas = canvas.clone();
        let view = view.clone();
        Rc::new(move |mode: u8| {
            let Some(mut d) = doc.borrow().clone() else { return };
            if d.has_build() {
                return;
            }
            view.set(ViewMode::Result);
            let (sw, sh) = (d.source.raster.width(), d.source.raster.height());
            let cv = canvas.get();
            let c = cv as f64;
            let mut p = d.placement();
            match mode {
                0 => p = Placement::fit(sw, sh, cv),
                1 => p = Placement::cover(sw, sh, cv),
                2 => {
                    // Centre, keeping the current zoom.
                    p.dx = (c - p.w(sw)) / 2.0;
                    p.dy = (c - p.h(sh)) / 2.0;
                }
                _ => {
                    p = Placement {
                        scale: 1.0,
                        dx: (c - sw as f64) / 2.0,
                        dy: (c - sh as f64) / 2.0,
                    };
                }
            }
            d.set_placement(p);
            mutate(d, None);
        })
    };
    for (b, mode) in [(&b_fit, 0u8), (&b_cover, 1), (&b_centre, 2), (&b_11, 3)] {
        let reframe = reframe.clone();
        b.connect_clicked(move |_| reframe(mode));
    }

    box_.append(&area);

    // --- rail thumbnails: draw + click-to-select ---------------------------
    // Source thumb: the raw file, fit into the square.
    src_thumb.set_draw_func(clone!(
        #[strong]
        doc,
        #[strong]
        view,
        move |_, cr, w, h| {
            thumb_checker(cr, w, h);
            if let Some(pb) = doc_src(&doc.borrow()) {
                let (ox, oy, sc) = fitted(pb.width() as f64, pb.height() as f64, w as f64, h as f64);
                cr.save().ok();
                cr.translate(ox, oy);
                cr.scale(sc, sc);
                cr.set_source_pixbuf(&pb, 0.0, 0.0);
                let _ = cr.paint();
                cr.restore().ok();
            }
            thumb_border(cr, w, h, view.get() == ViewMode::Source);
        }
    ));
    // Result thumb: the composite, drawn the same way the canvas draws it —
    // the applied out, or the framed source — fit into the square.
    res_thumb.set_draw_func(clone!(
        #[strong]
        canvas,
        #[strong]
        doc,
        #[strong]
        result,
        #[strong]
        view,
        #[strong]
        backdrop,
        move |_, cr, w, h| {
            thumb_checker(cr, w, h);
            let cv = canvas.get();
            let (ox, oy, sc) = fitted(cv as f64, cv as f64, w as f64, h as f64);
            cr.save().ok();
            cr.translate(ox, oy);
            cr.scale(sc, sc);
            cr.rectangle(0.0, 0.0, cv as f64, cv as f64);
            cr.clip();
            paint_backdrop(cr, backdrop.get(), 0.0, 0.0, cv as f64, cv as f64);
            if let Some(pb) = result.borrow().clone() {
                cr.set_source_pixbuf(&pb, 0.0, 0.0);
                let _ = cr.paint();
            } else if let Some(pb) = doc_src(&doc.borrow()) {
                let p = doc_place(&doc.borrow());
                cr.translate(p.dx, p.dy);
                cr.scale(p.scale, p.scale);
                cr.set_source_pixbuf(&pb, 0.0, 0.0);
                let _ = cr.paint();
            }
            cr.restore().ok();
            thumb_border(cr, w, h, view.get() == ViewMode::Result);
        }
    ));

    // Click either thumb to make the canvas show that view.
    for (thumb, mode) in [(&src_thumb, ViewMode::Source), (&res_thumb, ViewMode::Result)] {
        let g = gtk::GestureClick::new();
        g.connect_pressed(clone!(
            #[strong]
            view,
            #[weak]
            area,
            move |_, _, _, _| {
                view.set(mode);
                area.queue_draw(); // cascades to both thumbs (border restyle)
            }
        ));
        thumb.add_controller(g);
    }

    // Inspect panel — SVG only, and empty otherwise. An SVG states its colours,
    // fonts and structure; a raster only implies them, so there is nothing
    // honest to show for a PNG here.
    let inspect = GBox::new(Orientation::Vertical, 6);
    let inspect_scroll = gtk::ScrolledWindow::new();
    inspect_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    inspect_scroll.set_min_content_height(120);
    inspect_scroll.set_max_content_height(260);
    inspect_scroll.set_child(Some(&inspect));
    inspect_scroll.set_visible(false);
    box_.append(&inspect_scroll);

    // ---- Template row ------------------------------------------------------
    // Disc mask with four corner treatments. Solid/Gradient take their colours
    // from what you have already picked — the palettes ARE the input, which is
    // the whole reason this lives in a colour tool rather than an image editor.
    let tpl = GBox::new(Orientation::Horizontal, 6);
    let tpl_lbl = Label::new(Some("Disc"));
    tpl_lbl.add_css_class("dim-label");
    tpl.append(&tpl_lbl);

    let b_alpha = Button::with_label("Alpha");
    b_alpha.set_tooltip_text(Some("Corners transparent — the honest default for artwork on an unknown background"));
    let b_white = Button::with_label("White");
    let b_solid = Button::with_label("Colour");
    b_solid.set_tooltip_text(Some("Corners filled with the CURRENT colour"));
    let b_grad = Button::with_label("Gradient");
    b_grad.set_tooltip_text(Some(
        "Radial from the rim outward: current colour at the rim, the previous pick at the corners",
    ));
    let b_reset = Button::with_label("Reset");
    b_reset.set_tooltip_text(Some("Back to the original image"));

    for b in [&b_alpha, &b_white, &b_solid, &b_grad, &b_reset, &b_save, &b_overwrite] {
        b.set_sensitive(false);
        tpl.append(b);
    }
    tpl.set_visible(false);
    box_.append(&tpl);

    // ---- Build row: invert + composited squares (label-artwork primitives) --
    // These COMPOSE with each other and with the disc, each acting on whatever is
    // already on the canvas — so you can stack: square, square, invert, disc.
    let bld = GBox::new(Orientation::Horizontal, 6);
    let bld_lbl = Label::new(Some("Build"));
    bld_lbl.add_css_class("dim-label");
    bld.append(&bld_lbl);

    let b_invert = Button::with_label("Invert");
    b_invert.set_tooltip_text(Some(
        "Invert the colours (RGB). Transparency is left alone — a colour tool inverts the colour, not the shape.",
    ));
    bld.append(&b_invert);

    let sq_lbl = Label::new(Some("Square"));
    sq_lbl.add_css_class("dim-label");
    bld.append(&sq_lbl);
    // 5–100% of the canvas side. A slider because sizing a square is something
    // you judge by eye, not by typing a number.
    let sq_scale = gtk::Scale::with_range(Orientation::Horizontal, 5.0, 100.0, 1.0);
    sq_scale.set_value(40.0);
    sq_scale.set_hexpand(true);
    sq_scale.set_draw_value(true);
    sq_scale.set_value_pos(gtk::PositionType::Right);
    sq_scale.set_tooltip_text(Some("Square side, as a percent of the canvas"));
    bld.append(&sq_scale);

    let b_square = Button::with_label("+ Square");
    b_square.set_tooltip_text(Some(
        "Stamp a centred black square at this size. Stamp several, at different sizes, to build a design.",
    ));
    bld.append(&b_square);

    for b in [&b_invert, &b_square] {
        b.set_sensitive(false);
    }
    sq_scale.set_sensitive(false);
    bld.set_visible(false);
    box_.append(&bld);

    box_.append(&build_batch(window, shared, canvas.clone(), &size_dd));

    // Changing the output size RESCALES the framing rather than resetting it —
    // you chose where the picture sits, and that decision survives a size change.
    // Build steps already applied are dropped (Composition::set_canvas), as they
    // always have been.
    size_dd.connect_selected_notify(clone!(
        #[strong]
        canvas,
        #[strong]
        doc,
        #[strong]
        mutate,
        #[strong]
        shared,
        #[weak]
        area,
        move |dd| {
            let want = sane_canvas(
                CANVAS_SIZES
                    .get(dd.selected() as usize)
                    .copied()
                    .unwrap_or(CANVAS_DEFAULT),
            );
            let from = canvas.get();
            if want == from {
                return;
            }
            canvas.set(want);
            {
                let mut s = shared.borrow_mut();
                s.data.canvas = want;
                let _ = save_data(&s.data);
            }
            let next = doc.borrow().clone();
            match next {
                Some(mut d) => {
                    d.set_canvas(want);
                    mutate(d, None);
                }
                None => area.queue_draw(),
            }
        }
    ));

    // Add a build step. Steps stack, each acting on whatever the ones before it
    // left on the canvas — invert an image that already has a square,
    // disc-mask something you inverted, and so on.
    let push_op: Rc<dyn Fn(Operation)> = {
        let doc = doc.clone();
        let mutate = mutate.clone();
        Rc::new(move |op: Operation| {
            let Some(mut d) = doc.borrow().clone() else { return };
            d.ops.push(op);
            mutate(d, None);
        })
    };

    let apply = {
        let push_op = push_op.clone();
        let shared = shared.clone();
        let window = window.clone();
        Rc::new(move |fill_kind: u8| {
            let (cur, prev) = {
                let s = shared.borrow();
                (
                    s.current.or_else(|| s.data.history.first().copied()),
                    s.data.history.get(1).copied().or(s.current),
                )
            };
            let fill = match fill_kind {
                0 => OuterFill::Alpha,
                1 => OuterFill::White,
                2 => match cur {
                    Some(c) => OuterFill::Solid(c),
                    None => {
                        show_error(&window, "Pick a colour first — that is the fill.");
                        return;
                    }
                },
                _ => match (cur, prev) {
                    (Some(i), Some(o)) => OuterFill::Gradient { inner: i, outer: o },
                    _ => {
                        show_error(
                            &window,
                            "A gradient needs two colours — pick a second one.",
                        );
                        return;
                    }
                },
            };
            // The disc masks whatever is on the canvas now — the framed source,
            // or a design you have already built up.
            push_op(Operation::Disc(fill));
        })
    };

    b_invert.connect_clicked(clone!(
        #[strong]
        push_op,
        move |_| push_op(Operation::Invert)
    ));

    b_square.connect_clicked(clone!(
        #[strong]
        push_op,
        #[weak]
        sq_scale,
        move |_| {
            push_op(Operation::Square {
                frac: sq_scale.value() / 100.0,
                colour: Rgb { r: 0, g: 0, b: 0 },
            })
        }
    ));

    for (b, kind) in [(&b_alpha, 0u8), (&b_white, 1), (&b_solid, 2), (&b_grad, 3)] {
        let apply = apply.clone();
        b.connect_clicked(move |_| apply(kind));
    }
    b_reset.connect_clicked(clone!(
        #[strong]
        doc,
        #[strong]
        mutate,
        move |_| {
            let Some(mut d) = doc.borrow().clone() else { return };
            d.clear_build();
            mutate(d, None);
        }
    ));
    b_save.connect_clicked(clone!(
        #[weak]
        window,
        #[strong]
        result,
        #[strong]
        name,
        move |_| {
            let Some(pb) = result.borrow().clone() else { return };
            let stem = name
                .borrow()
                .rsplit_once('.')
                .map(|(a, _)| a.to_string())
                .unwrap_or_else(|| name.borrow().clone());
            let dlg = gtk::FileDialog::builder()
                .title("Save image")
                .initial_name(format!("{stem}-out.png"))
                .build();
            dlg.save(
                Some(&window),
                gio::Cancellable::NONE,
                clone!(
                    #[weak]
                    window,
                    move |res| {
                        let Ok(file) = res else { return };
                        let Some(path) = file.path() else { return };
                        // Always PNG: the mask's whole point is alpha, and JPEG
                        // has none.
                        if let Err(e) = pb.savev(&path, "png", &[]) {
                            show_error(&window, &format!("Save failed: {e}"));
                        }
                    }
                ),
            );
        }
    ));
    // Overwrite the ORIGINAL file. Irreversible, and often a file in the music
    // library, so it asks first — the one place this tool writes over something
    // it did not create.
    b_overwrite.connect_clicked(clone!(
        #[weak]
        window,
        #[strong]
        result,
        #[strong]
        src_path,
        move |_| {
            let Some(pb) = result.borrow().clone() else { return };
            let Some(path) = src_path.borrow().clone() else { return };
            if !src_is_png(&Some(path.clone())) {
                return;
            }
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("this file")
                .to_string();
            let dlg = gtk::AlertDialog::builder()
                .modal(true)
                .message(format!("Overwrite {name}?"))
                .detail("This replaces the original PNG with the result. It cannot be undone.")
                .buttons(["Cancel", "Overwrite"])
                .cancel_button(0)
                .default_button(0)
                .build();
            dlg.choose(
                Some(&window),
                gio::Cancellable::NONE,
                clone!(
                    #[weak]
                    window,
                    move |res| {
                        if res.ok() != Some(1) {
                            return; // Cancel, Esc, or error
                        }
                        if let Err(e) = pb.savev(&path, "png", &[]) {
                            show_error(&window, &format!("Overwrite failed: {e}"));
                        }
                    }
                ),
            );
        }
    ));

    let rebuild_inspect = {
        let svg = svg.clone();
        let inspect = inspect.clone();
        let inspect_scroll = inspect_scroll.clone();
        let shared = shared.clone();
        let window = window.clone();
        move || {
            while let Some(c) = inspect.first_child() {
                inspect.remove(&c);
            }
            let Some(info) = svg.borrow().as_ref().map(clone_info) else {
                inspect_scroll.set_visible(false);
                return;
            };
            inspect_scroll.set_visible(true);

            let facts = Label::new(Some(&format!(
                "viewBox {}   ·   {} gradient{}   ·   {}",
                info.view_box.as_deref().unwrap_or("—"),
                info.gradients,
                if info.gradients == 1 { "" } else { "s" },
                info.elements
                    .iter()
                    .take(5)
                    .map(|(t, n)| format!("{n}×{t}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
            facts.add_css_class("dim-label");
            facts.set_xalign(0.0);
            facts.set_wrap(true);
            inspect.append(&facts);

            if !info.fonts.is_empty() {
                let l = Label::new(Some(&format!("fonts: {}", info.fonts.join(", "))));
                l.set_xalign(0.0);
                l.set_wrap(true);
                inspect.append(&l);
            }
            if !info.texts.is_empty() {
                let l = Label::new(Some(&format!("text: “{}”", info.texts.join("” “"))));
                l.set_xalign(0.0);
                l.set_wrap(true);
                l.add_css_class("dim-label");
                inspect.append(&l);
            }

            if !info.colors.is_empty() {
                let head = Label::new(Some(&format!(
                    "{} declared colour{} (click to pick)",
                    info.colors.len(),
                    if info.colors.len() == 1 { "" } else { "s" }
                )));
                head.set_xalign(0.0);
                head.add_css_class("dim-label");
                inspect.append(&head);

                let chips = gtk::FlowBox::new();
                chips.set_selection_mode(gtk::SelectionMode::None);
                chips.set_max_children_per_line(12);
                for (c, n) in info.colors.iter().copied() {
                    let sw = DrawingArea::new();
                    sw.set_content_width(28);
                    sw.set_content_height(28);
                    sw.set_draw_func(move |_, cr, w, h| {
                        cr.set_source_rgb(
                            c.r as f64 / 255.0,
                            c.g as f64 / 255.0,
                            c.b as f64 / 255.0,
                        );
                        cr.rectangle(0.0, 0.0, w as f64, h as f64);
                        let _ = cr.fill();
                    });
                    sw.set_tooltip_text(Some(&format!(
                        "{} — used {n}× (declared in the file, not sampled)",
                        c.hex()
                    )));
                    let g = gtk::GestureClick::new();
                    g.connect_pressed(clone!(
                        #[strong]
                        shared,
                        #[weak]
                        window,
                        move |_, _, _, _| {
                            {
                                let mut s = shared.borrow_mut();
                                s.current = Some(c);
                                s.data.history.retain(|x| *x != c);
                                s.data.history.insert(0, c);
                                s.data.history.truncate(HISTORY_LIMIT);
                                let _ = save_data(&s.data);
                            }
                            let s = shared.borrow();
                            refresh_swatch(&s);
                            refresh_code(&s);
                            refresh_history_ui(&s, &window, &shared);
                            copy_to_clipboard(&window, &c.format(s.data.format));
                        }
                    ));
                    sw.add_controller(g);
                    chips.append(&sw);
                }
                inspect.append(&chips);
            }
        }
    };

    let rebuild_inspect = Rc::new(rebuild_inspect);

    // The ONE place a loaded image becomes what is on the canvas — pixbuf, name,
    // any SVG facts, framing, and every control switched live. Open, Blank and
    // the folder browser all funnel through here, so they cannot drift: whatever
    // one of them enables, they all do. `tip` is the full path for the hover, or
    // None for a synthetic canvas that has no path.
    let set_image: Rc<dyn Fn(gdk_pixbuf::Pixbuf, String, Option<SvgInfo>, Option<String>)> = {
        let canvas = canvas.clone();
        let show_doc = show_doc.clone();
        let history = history.clone();
        let sync_history = sync_history.clone();
        let svg = svg.clone();
        let name = name.clone();
        let rebuild_inspect = rebuild_inspect.clone();
        let ctl = ctl.clone();
        let b_grid = b_grid.clone();
        let b_fit = b_fit.clone();
        let b_cover = b_cover.clone();
        let b_centre = b_centre.clone();
        let b_11 = b_11.clone();
        let pal_btn = pal_btn.clone();
        let tpl = tpl.clone();
        let b_alpha = b_alpha.clone();
        let b_white = b_white.clone();
        let b_solid = b_solid.clone();
        let b_grad = b_grad.clone();
        let b_reset = b_reset.clone();
        let bld = bld.clone();
        let b_invert = b_invert.clone();
        let b_square = b_square.clone();
        let sq_scale = sq_scale.clone();
        let src_path = src_path.clone();
        let file_lbl = file_lbl.clone();
        let view = view.clone();
        Rc::new(move |pb: gdk_pixbuf::Pixbuf, nm: String, info: Option<SvgInfo>, tip: Option<String>| {
            let (pw, ph) = (pb.width(), pb.height());
            let is_svg = info.is_some();
            file_lbl.set_text(&format!("{nm}  \u{b7}  {pw}\u{d7}{ph}"));
            file_lbl.set_tooltip_text(tip.as_deref());
            pal_btn.set_label(if is_svg { "Palette from SVG" } else { "Palette from image" });
            pal_btn.set_tooltip_text(Some(if is_svg {
                "The colours the file DECLARES \u{2014} exact, not sampled"
            } else {
                "The image's dominant colours, quantised from its pixels"
            }));
            *name.borrow_mut() = nm;
            *svg.borrow_mut() = info;
            // The path is the Overwrite target; a Blank canvas (tip None) has none.
            // Set before `mutate`, which reads it for Overwrite's sensitivity.
            *src_path.borrow_mut() = tip.as_deref().map(PathBuf::from);
            view.set(ViewMode::Result); // a fresh image: Result == the framed source
            // A new document: the source covering the canvas, no build steps.
            // It also turns Save and Overwrite off — nothing built, nothing to
            // write. Not an edit, so not undoable: it starts a new history.
            // (Undo back into the previous file would be a surprise, and its
            // Overwrite target is already gone.)
            history.borrow_mut().clear();
            show_doc(Composition::new(pb, canvas.get()));
            sync_history();
            ctl.set_visible(true);
            b_grid.set_sensitive(true);
            for b in [&b_fit, &b_cover, &b_centre, &b_11] {
                b.set_sensitive(true);
            }
            pal_btn.set_sensitive(true);
            tpl.set_visible(true);
            for b in [&b_alpha, &b_white, &b_solid, &b_grad, &b_reset] {
                b.set_sensitive(true);
            }
            bld.set_visible(true);
            b_invert.set_sensitive(true);
            b_square.set_sensitive(true);
            sq_scale.set_sensitive(true);
            rebuild_inspect();
        })
    };

    // Show the browse-list entry at `i` (wrapping is the caller's job) and update
    // the position readout. The list drives it, so the same nav will later serve
    // the published manifest's covers, not only a folder.
    let show_index: Rc<dyn Fn(usize)> = {
        let browse = browse.clone();
        let browse_idx = browse_idx.clone();
        let set_image = set_image.clone();
        let nav_lbl = nav_lbl.clone();
        let window = window.clone();
        Rc::new(move |i: usize| {
            let (path, total) = {
                let list = browse.borrow();
                if list.is_empty() {
                    return;
                }
                let i = i % list.len();
                (list[i].clone(), list.len())
            };
            match load_image_file(&path) {
                Ok((pb, nm, info)) => {
                    let i = i % total;
                    browse_idx.set(i);
                    nav_lbl.set_text(&format!("{} / {total}  \u{b7}  {nm}", i + 1));
                    set_image(pb, nm, info, Some(path.to_string_lossy().into_owned()));
                }
                // A broken file mid-scroll should not stop the scroll; say which.
                Err(e) => show_error(&window, &format!("{e:#}")),
            }
        })
    };

    open_btn.connect_clicked(clone!(
        #[weak]
        window,
        #[strong]
        set_image,
        #[strong]
        browse,
        #[weak]
        nav,
        move |_| {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Images (PNG / SVG / JPEG / WebP)"));
            for p in ["*.png", "*.svg", "*.jpg", "*.jpeg", "*.webp"] {
                filter.add_pattern(p);
            }
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dlg = gtk::FileDialog::builder()
                .title("Open image")
                .filters(&filters)
                .build();
            dlg.open(
                Some(&window),
                gio::Cancellable::NONE,
                clone!(
                    #[weak]
                    window,
                    #[strong]
                    set_image,
                    #[strong]
                    browse,
                    #[weak]
                    nav,
                    move |res| {
                        let Ok(file) = res else { return };
                        let Some(path) = file.path() else { return };
                        match load_image_file(&path) {
                            Ok((pb, nm, info)) => {
                                set_image(pb, nm, info, Some(path.to_string_lossy().into_owned()));
                                // A single Open leaves browse mode.
                                browse.borrow_mut().clear();
                                nav.set_visible(false);
                            }
                            Err(e) => show_error(&window, &format!("{e:#}")),
                        }
                    }
                ),
            );
        }
    ));

    browse_btn.connect_clicked(clone!(
        #[weak]
        window,
        #[strong]
        browse,
        #[strong]
        show_index,
        #[weak]
        nav,
        #[weak]
        area,
        move |_| {
            let dlg = gtk::FileDialog::builder()
                .title("Browse a folder of images")
                .build();
            dlg.select_folder(
                Some(&window),
                gio::Cancellable::NONE,
                clone!(
                    #[weak]
                    window,
                    #[strong]
                    browse,
                    #[strong]
                    show_index,
                    #[weak]
                    nav,
                    #[weak]
                    area,
                    move |res| {
                        let Ok(f) = res else { return };
                        let Some(dir) = f.path() else { return };
                        let mut imgs = Vec::new();
                        let _ = find_images(&dir, "", &mut imgs);
                        imgs.sort();
                        if imgs.is_empty() {
                            show_error(&window, "No images in that folder.");
                            return;
                        }
                        *browse.borrow_mut() = imgs;
                        nav.set_visible(true);
                        show_index(0);
                        // Focus the canvas so the arrow keys drive the scroll.
                        area.grab_focus();
                    }
                ),
            );
        }
    ));

    // Prev/Next wrap — scrolling a collection should not dead-end at the last
    // cover. The buttons are the reliable path; the arrow keys mirror them.
    let step = {
        let browse = browse.clone();
        let browse_idx = browse_idx.clone();
        let show_index = show_index.clone();
        Rc::new(move |forward: bool| {
            let n = browse.borrow().len();
            if n == 0 {
                return;
            }
            let cur = browse_idx.get();
            let i = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
            show_index(i);
        })
    };
    b_prev.connect_clicked(clone!(
        #[strong]
        step,
        move |_| step(false)
    ));
    b_next.connect_clicked(clone!(
        #[strong]
        step,
        move |_| step(true)
    ));

    // Arrow keys on the canvas mirror the buttons while browsing. On the canvas,
    // not the window, so they cannot fight the slider's own left/right or steal
    // keys from the batch filter entry.
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(clone!(
        #[strong]
        step,
        #[strong]
        browse,
        move |_, key, _, _| {
            if browse.borrow().is_empty() {
                return glib::Propagation::Proceed;
            }
            match key {
                gtk::gdk::Key::Left => {
                    step(false);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Right => {
                    step(true);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        }
    ));
    area.set_focusable(true);
    area.add_controller(keys);

    // Blank goes through the SAME set_image path a file does — it just
    // synthesises the pixbuf instead of reading one. The source is a generous
    // flat white; scaling flat white costs nothing and never degrades. No path,
    // so no hover tooltip. A blank also leaves browse mode.
    blank_btn.connect_clicked(clone!(
        #[weak]
        window,
        #[strong]
        set_image,
        #[strong]
        browse,
        #[weak]
        nav,
        move |_| {
            match blank_canvas(1000) {
                Ok(pb) => {
                    set_image(pb, "blank".to_string(), None, None);
                    browse.borrow_mut().clear();
                    nav.set_visible(false);
                }
                Err(e) => show_error(&window, &format!("Could not create a blank canvas: {e}")),
            }
        }
    ));

    pal_btn.connect_clicked(clone!(
        #[weak]
        window,
        #[strong]
        doc,
        #[strong]
        name,
        #[strong]
        svg,
        #[strong]
        shared,
        move |_| {
            let Some(pb) = doc_src(&doc.borrow()) else {
                return;
            };
            // An SVG's colours are STATED. Sampling its pixels instead would be
            // guessing at an answer the file already gives — and would invent
            // anti-aliased in-between tones that the artwork does not use.
            let colors: Vec<Rgb> = match svg.borrow().as_ref() {
                Some(info) => info.colors.iter().map(|(c, _)| *c).collect(),
                None => dominant_colors(&pb, 12),
            };
            if colors.is_empty() {
                show_error(&window, "No colours found.");
                return;
            }
            {
                let mut s = shared.borrow_mut();
                s.data.palettes.push(Palette {
                    name: name.borrow().clone(),
                    colors: colors.into_iter().map(Swatch::new).collect(),
                });
                let _ = save_data(&s.data);
            }
            let s = shared.borrow();
            refresh_palettes_all(&s, &window, &shared);
        }
    ));

    box_.set_hexpand(true);

    // The right rail: a thin full-height strip, Source over Result, each a
    // thumbnail you click to put on the canvas.
    let rail = GBox::new(Orientation::Vertical, 6);
    rail.set_width_request(THUMB + 20);
    let rail_head = Label::new(Some("View"));
    rail_head.add_css_class("section-head");
    rail_head.set_xalign(0.0);
    rail.append(&rail_head);
    for (label, thumb, tip) in [
        ("Source", &src_thumb, "The raw image, as loaded"),
        ("Result", &res_thumb, "What you have built — the composite that would be saved"),
    ] {
        let l = Label::new(Some(label));
        l.add_css_class("dim-label");
        l.set_xalign(0.0);
        thumb.set_tooltip_text(Some(tip));
        rail.append(&l);
        rail.append(thumb);
    }

    let split = GBox::new(Orientation::Horizontal, 12);
    split.append(&box_);
    split.append(&Separator::new(Orientation::Vertical));
    split.append(&rail);
    split
}

fn export_palette(window: &ApplicationWindow, shared: &SharedState, idx: usize) {
    let pal = match shared.borrow().data.palettes.get(idx).cloned() {
        Some(p) => p,
        None => return,
    };
    let dlg = gtk::FileDialog::builder()
        .title(format!("Export palette: {}", pal.name))
        .initial_name(format!("{}.gpl", sanitize(&pal.name)))
        .build();
    dlg.save(
        Some(window),
        gio::Cancellable::NONE,
        clone!(
            #[weak]
            window,
            move |res| {
                let Ok(file) = res else { return };
                let Some(path) = file.path() else { return };
                let result = match path.extension().and_then(|s| s.to_str()) {
                    Some("css") => write_css(&path, &pal),
                    Some("json") => write_json(&path, &pal),
                    _ => write_gpl(&path, &pal),
                };
                if let Err(e) = result {
                    show_error(&window, &format!("Export failed: {e}"));
                }
            }
        ),
    );
}

/// GTK4 has a real CSS engine, so the styling lives here rather than being
/// hand-rolled in draw calls. Selectors, custom classes, spacing, borders — all
/// of it. Rust is not the constraint; GTK is the toolkit.
const APP_CSS: &str = "
  /* A darker ground than the theme's grey — not black — so the small, dim text
     reads against it. Both the named colours (for themed widgets: cards,
     popovers, entries) and the window/headerbar nodes directly, since a theme
     might paint those without the named colour. */
  @define-color window_bg_color #171717;
  @define-color view_bg_color #121212;
  @define-color headerbar_bg_color #1d1d1d;
  @define-color card_bg_color #1f1f1f;
  @define-color popover_bg_color #1d1d1d;
  window, window.background { background-color: #171717; }
  headerbar { background-color: #1d1d1d; }

  /* Lift the dim/secondary text a notch. The theme dims it to ~0.55, which is
     too faint on the darker ground; 0.74 brightens it a fraction without taking
     it to full white. */
  .dim-label { opacity: 0.74; }

  .section-head { font-weight: 700; }
  .tips-card {
    background: alpha(@accent_bg_color, 0.10);
    border: 1px solid alpha(@accent_bg_color, 0.35);
    border-radius: 10px;
    padding: 10px;
  }
  .tips-card label { font-size: 0.86em; }
  .canvas-frame { border-radius: 6px; }
  expander title { padding: 2px 0; }
  /* Swatches must touch: the theme pads flowboxchild, which shows as a gap
     between squares even with the FlowBox's own spacing at zero. */
  .swatch-flow > flowboxchild { padding: 0; margin: 0; min-width: 0; min-height: 0; }
";

fn install_css() {
    let p = gtk::CssProvider::new();
    p.load_from_data(APP_CSS); // load_from_string is 4.12; we target v4_10
    if let Some(d) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &d,
            &p,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Where the image is actually drawn inside the widget: fit to the box,
/// preserve aspect, centre. Returned so a click can be mapped back to a pixel —
/// the draw and the hit-test MUST agree, so they share this one function rather
/// than each doing their own arithmetic.
fn fitted(pw: f64, ph: f64, ww: f64, wh: f64) -> (f64, f64, f64) {
    let scale = (ww / pw).min(wh / ph).min(8.0); // don't magnify past 8x
    let dw = pw * scale;
    let dh = ph * scale;
    ((ww - dw) / 2.0, (wh - dh) / 2.0, scale)
}

/// What is under a widget point `(px, py)` on the canvas area of size `(aw, ah)`:
/// the coordinate (canvas px in Result view, image px in Source view) and the
/// colour there, if a pixel exists. The single mapping used by BOTH the hover
/// crosshair readout and click-to-pick, so what you hover is what you pick.
/// The preview backdrop: what the canvas's transparent pixels are seen
/// against. Preview only — see `paint_backdrop`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Backdrop {
    /// The checkerboard already behind everything: nothing extra to paint.
    Checker,
    Grey,
    Black,
    White,
    Colour(Rgb),
}

/// Fill the rectangle with the backdrop, if it is anything but the checker.
/// Called by the canvas and the Result thumbnail, never by a renderer: the
/// backdrop is a viewing condition, and a file that contained it would be
/// wrong in exactly the way the backdrop exists to reveal.
fn paint_backdrop(cr: &gtk::cairo::Context, b: Backdrop, x: f64, y: f64, w: f64, h: f64) {
    let (r, g, bl) = match b {
        Backdrop::Checker => return,
        Backdrop::Grey => (0.5, 0.5, 0.5),
        Backdrop::Black => (0.0, 0.0, 0.0),
        Backdrop::White => (1.0, 1.0, 1.0),
        Backdrop::Colour(c) => (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0),
    };
    cr.set_source_rgb(r, g, bl);
    cr.rectangle(x, y, w, h);
    let _ = cr.fill();
}

/// The open document's source pixels, if a document is open.
fn doc_src(doc: &Option<Composition>) -> Option<gdk_pixbuf::Pixbuf> {
    doc.as_ref().map(|d| d.source.raster.clone())
}

/// Where the open document's source sits (identity when nothing is open).
fn doc_place(doc: &Option<Composition>) -> Placement {
    doc.as_ref()
        .map(|d| d.placement())
        .unwrap_or(Placement { dx: 0.0, dy: 0.0, scale: 1.0 })
}

fn pixel_under(
    px: f64,
    py: f64,
    aw: f64,
    ah: f64,
    view: ViewMode,
    canvas: i32,
    out: &Option<gdk_pixbuf::Pixbuf>,
    src: &Option<gdk_pixbuf::Pixbuf>,
    place: Placement,
) -> Option<(i32, i32, Rgb)> {
    let rgb_at = |pb: &gdk_pixbuf::Pixbuf, x: i32, y: i32| {
        let bytes = unsafe { pb.pixels() };
        let (r, g, b, _) = sample(pb, bytes, x, y);
        Rgb { r, g, b }
    };
    match view {
        ViewMode::Source => {
            let pb = src.as_ref()?;
            let (ox, oy, sc) = fitted(pb.width() as f64, pb.height() as f64, aw, ah);
            let ix = ((px - ox) / sc).floor() as i32;
            let iy = ((py - oy) / sc).floor() as i32;
            if ix < 0 || iy < 0 || ix >= pb.width() || iy >= pb.height() {
                return None;
            }
            Some((ix, iy, rgb_at(pb, ix, iy)))
        }
        ViewMode::Result => {
            // Widget -> canvas: the canvas square is what is fitted in Result view.
            let (ox, oy, sc) = fitted(canvas as f64, canvas as f64, aw, ah);
            let cx = ((px - ox) / sc).floor() as i32;
            let cy = ((py - oy) / sc).floor() as i32;
            if cx < 0 || cy < 0 || cx >= canvas || cy >= canvas {
                return None;
            }
            if let Some(o) = out {
                // The applied result is canvas-sized: sample it directly.
                return Some((cx, cy, rgb_at(o, cx, cy)));
            }
            // No result yet: the canvas shows the PLACED source, so invert the
            // placement to find the source pixel. Off the source = transparent =
            // no colour, but the coordinate is still valid.
            let pb = src.as_ref()?;
            let sx = ((cx as f64 - place.dx) / place.scale).floor() as i32;
            let sy = ((cy as f64 - place.dy) / place.scale).floor() as i32;
            if sx < 0 || sy < 0 || sx >= pb.width() || sy >= pb.height() {
                return None;
            }
            Some((cx, cy, rgb_at(pb, sx, sy)))
        }
    }
}

/// The TradingView-style crosshair: dashed guide lines tracking the pointer
/// across the whole canvas, plus a small readout of the position and the colour
/// underneath — the precision layer for editing / drawing / selecting.
fn draw_crosshair(
    cr: &gtk::cairo::Context,
    w: f64,
    h: f64,
    px: f64,
    py: f64,
    info: Option<(i32, i32, Rgb)>,
) {
    // Guides — full-width and full-height dashed lines through the pointer.
    cr.save().ok();
    cr.set_line_width(1.0);
    cr.set_dash(&[4.0, 4.0], 0.0);
    cr.set_source_rgba(0.95, 0.95, 1.0, 0.55);
    let (gx, gy) = (px.floor() + 0.5, py.floor() + 0.5);
    cr.move_to(gx, 0.0);
    cr.line_to(gx, h);
    cr.move_to(0.0, gy);
    cr.line_to(w, gy);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
    cr.restore().ok();

    // Readout chip — coordinate + colour, offset off the pointer and clamped so
    // it never runs off the edge.
    let Some((x, y, c)) = info else { return };
    let text = format!("{x},{y}  {}", c.hex());
    cr.select_font_face(
        "monospace",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    cr.set_font_size(12.0);
    let tw = cr.text_extents(&text).map(|e| e.width()).unwrap_or(90.0);
    let (sw, pad) = (14.0, 6.0);
    let (bw, bh) = (pad + sw + 6.0 + tw + pad, 22.0);
    let mut bx = px + 14.0;
    let mut by = py + 14.0;
    if bx + bw > w {
        bx = (px - 14.0 - bw).max(2.0);
    }
    if by + bh > h {
        by = (py - 14.0 - bh).max(2.0);
    }
    cr.set_source_rgba(0.05, 0.05, 0.06, 0.85);
    cr.rectangle(bx, by, bw, bh);
    let _ = cr.fill();
    cr.set_source_rgb(c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0);
    cr.rectangle(bx + pad, by + (bh - sw) / 2.0, sw, sw);
    let _ = cr.fill();
    cr.set_source_rgba(0.95, 0.95, 0.97, 1.0);
    cr.move_to(bx + pad + sw + 6.0, by + bh - 7.0);
    let _ = cr.show_text(&text);
}

/// The transparency checkerboard behind a rail thumbnail.
fn thumb_checker(cr: &gtk::cairo::Context, w: i32, h: i32) {
    let (w, h) = (w as f64, h as f64);
    let sq = 10.0;
    cr.set_source_rgb(0.16, 0.16, 0.17);
    let _ = cr.paint();
    cr.set_source_rgb(0.21, 0.21, 0.22);
    let (mut y, mut row) = (0.0, 0);
    while y < h {
        let mut x = if row % 2 == 0 { 0.0 } else { sq };
        while x < w {
            cr.rectangle(x, y, sq, sq);
            x += sq * 2.0;
        }
        y += sq;
        row += 1;
    }
    let _ = cr.fill();
}

/// A thumbnail's border — bright cyan (the app's "active" colour, same as the
/// snap guides) when this is the selected view, faint otherwise.
fn thumb_border(cr: &gtk::cairo::Context, w: i32, h: i32, selected: bool) {
    let (w, h) = (w as f64, h as f64);
    if selected {
        cr.set_source_rgba(0.2, 0.85, 1.0, 0.95);
        cr.set_line_width(2.0);
        cr.rectangle(1.0, 1.0, w - 2.0, h - 2.0);
    } else {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.2);
        cr.set_line_width(1.0);
        cr.rectangle(0.5, 0.5, w - 1.0, h - 1.0);
    }
    let _ = cr.stroke();
}

// ---------- image view ----------
//
// A viewer for release artwork (PNG / SVG / JPEG), so a cover can be
// interrogated for colour in the same place the palettes live. Two jobs:
//
//   1. CLICK A PIXEL -> that colour becomes the current colour, flowing into the
//      history and palettes exactly as an X11 screen pick does. The app already
//      knew how to pick from the screen; it could not pick from a file.
//   2. EXTRACT A PALETTE -> quantise the image down to its dominant colours and
//      save them as a named palette.
//
// SVG works because librsvg's gdk-pixbuf loader is installed. (The rsvg-convert
// BINARY is not — that is a separate trap, and why `make icons` mangles
// gradients — but the library is what GTK needs.)

// ---------- main UI build ----------

fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("n.cover")
        // Wide enough for controls + a usable image area side by side.
        .default_width(1000)
        .default_height(720)
        .build();

    let header = HeaderBar::new();
    // The view switcher, as the title. Two linked toggles reading Picker /
    // Palettes — the same segmented-control idea the suite's Tauri apps use for
    // their nav, so a second view has an obvious home in the chrome rather than a
    // button buried in a section.
    let view_switch = GBox::new(Orientation::Horizontal, 0);
    view_switch.add_css_class("linked");
    let view_pick = ToggleButton::with_label("Picker");
    let view_pal = ToggleButton::with_label("Palettes");
    view_pal.set_group(Some(&view_pick));
    view_pick.set_active(true);
    view_switch.append(&view_pick);
    view_switch.append(&view_pal);
    header.set_title_widget(Some(&view_switch));

    // Tips toggle. One control, not two: a button that opens the panel and a
    // separate toggle that hides it would be two ways to say the same thing, and
    // they would disagree the moment one of them got out of step. This IS the
    // `tips` flag — pressed means shown, and "Don't show again" un-presses it.
    let tips_btn = ToggleButton::new();
    tips_btn.set_icon_name("help-about-symbolic");
    tips_btn.set_tooltip_text(Some("Getting started — what this app can do"));
    header.pack_end(&tips_btn);

    window.set_titlebar(Some(&header));

    let outer = GBox::new(Orientation::Vertical, 12);
    // No margins here: `columns` (below) owns the window padding now that this
    // is the left column rather than the whole window.
    outer.set_margin_end(6); // breathing room before the scrollbar

    // top: swatch + code
    let top = GBox::new(Orientation::Horizontal, 12);

    let swatch = DrawingArea::new();
    swatch.set_size_request(120, 120);
    swatch.add_css_class("swatch");
    top.append(&swatch);

    let code_col = GBox::new(Orientation::Vertical, 8);
    code_col.set_hexpand(true);

    let fmt_row = GBox::new(Orientation::Horizontal, 0);
    fmt_row.add_css_class("linked");
    let fmt_hex = ToggleButton::with_label("HEX");
    let fmt_rgb = ToggleButton::with_label("RGB");
    let fmt_hsl = ToggleButton::with_label("HSL");
    fmt_rgb.set_group(Some(&fmt_hex));
    fmt_hsl.set_group(Some(&fmt_hex));
    fmt_row.append(&fmt_hex);
    fmt_row.append(&fmt_rgb);
    fmt_row.append(&fmt_hsl);
    code_col.append(&fmt_row);

    let code_label = Label::new(Some("(no color picked)"));
    code_label.set_selectable(true);
    code_label.set_xalign(0.0);
    code_label.add_css_class("code-display");
    code_label.set_wrap(true);
    code_col.append(&code_label);

    let copy_btn = Button::with_label("Copy");
    copy_btn.add_css_class("suggested-action");
    code_col.append(&copy_btn);

    top.append(&code_col);
    outer.append(&top);

    // pick button
    let pick_btn = Button::with_label("Pick Color");
    pick_btn.add_css_class("pill");
    pick_btn.add_css_class("suggested-action");
    pick_btn.set_height_request(44);
    outer.append(&pick_btn);

    // History has no section of its own: it lives as the compact History palette
    // in the Palettes section below (with its own Clear there). The Expander and
    // the 32-deep row list it used to have are gone.

    // palettes section (Picker view) — COMPACT now: the History palette + the
    // pinned palettes as scan-at-a-glance strips. The full editable list moved to
    // the Palettes view, so the left column is not two long lists deep.
    let pal_header = GBox::new(Orientation::Horizontal, 8);
    let pal_title = Label::new(Some("Palettes"));
    pal_title.add_css_class("section-head");
    pal_title.set_xalign(0.0);
    pal_title.set_hexpand(true);
    pal_header.append(&pal_title);
    let manage_btn = Button::with_label("Palettes view →");
    manage_btn.set_tooltip_text(Some("Manage all palettes: import, create, edit, pin"));
    pal_header.append(&manage_btn);

    let pinned_box = GBox::new(Orientation::Vertical, 4);
    let pinned_scroll = gtk::ScrolledWindow::new();
    pinned_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    pinned_scroll.set_min_content_height(120);
    pinned_scroll.set_max_content_height(360);
    pinned_scroll.set_vexpand(true);
    pinned_scroll.set_child(Some(&pinned_box));

    pal_header.set_hexpand(true);
    let pal_exp = gtk::Expander::new(None);
    pal_exp.set_label_widget(Some(&pal_header));
    pal_exp.set_child(Some(&pinned_scroll));
    outer.append(&pal_exp);

    // The Palettes VIEW — the full, editable management surface, its own page.
    let pal_view = GBox::new(Orientation::Vertical, 12);
    pal_view.set_margin_top(16);
    pal_view.set_margin_bottom(16);
    pal_view.set_margin_start(16);
    pal_view.set_margin_end(16);

    let pv_head = GBox::new(Orientation::Horizontal, 8);
    let pv_title = Label::new(Some("Palettes"));
    pv_title.add_css_class("section-head");
    pv_title.set_xalign(0.0);
    pv_title.set_hexpand(true);
    pv_head.append(&pv_title);
    let import_btn = Button::with_label("Import");
    import_btn.set_tooltip_text(Some(
        "Import a palette (.gpl / .json). Swatch names are kept.",
    ));
    pv_head.append(&import_btn);
    let save_hist_btn = Button::with_label("Save History as palette");
    save_hist_btn.set_tooltip_text(Some(
        "Snapshot the current History palette as a named, permanent palette",
    ));
    pv_head.append(&save_hist_btn);
    let new_pal_btn = Button::with_label("New palette");
    new_pal_btn.add_css_class("suggested-action");
    pv_head.append(&new_pal_btn);
    pal_view.append(&pv_head);

    let pv_hint = Label::new(Some(
        "The pin marks a palette to ride along on the Picker view (up to 3 beside History). Using a palette's colour pins it too.",
    ));
    pv_hint.add_css_class("dim-label");
    pv_hint.set_wrap(true);
    pv_hint.set_xalign(0.0);
    pal_view.append(&pv_hint);

    let pal_scroll = gtk::ScrolledWindow::new();
    pal_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    pal_scroll.set_vexpand(true);
    let palettes_list = ListBox::new();
    palettes_list.set_selection_mode(gtk::SelectionMode::None);
    palettes_list.add_css_class("boxed-list");
    pal_scroll.set_child(Some(&palettes_list));
    pal_view.append(&pal_scroll);

    // Two columns. A 400x400 image area stacked under the controls turned the
    // window into a ~1100px-tall strip; side by side, the controls keep their
    // natural width and the image takes everything left over — and grows with
    // the window instead of pushing it taller.
    //
    // The left column scrolls on its own so a long palette list cannot force
    // the window's height either.
    let left_scroll = gtk::ScrolledWindow::new();
    left_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    left_scroll.set_child(Some(&outer));
    left_scroll.set_size_request(380, -1);
    left_scroll.set_hexpand(false);
    left_scroll.set_vexpand(true);

    // `shared` does not exist until the widget tree is done, so the image
    // section is filled in below; this is its slot.
    let image_slot = GBox::new(Orientation::Vertical, 0);
    image_slot.set_hexpand(true);
    image_slot.set_vexpand(true);

    let columns = GBox::new(Orientation::Horizontal, 12);
    columns.set_margin_top(12);
    columns.set_margin_bottom(12);
    columns.set_margin_start(12);
    columns.set_margin_end(12);
    columns.append(&left_scroll);
    columns.append(&Separator::new(Orientation::Vertical));
    columns.append(&image_slot);

    // Two pages behind the header switch: the picker, and the full palettes view.
    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.add_named(&columns, Some("picker"));
    stack.add_named(&pal_view, Some("palettes"));
    window.set_child(Some(&stack));

    view_pick.connect_toggled(clone!(
        #[weak]
        stack,
        move |b| {
            if b.is_active() {
                stack.set_visible_child_name("picker");
            }
        }
    ));
    view_pal.connect_toggled(clone!(
        #[weak]
        stack,
        move |b| {
            if b.is_active() {
                stack.set_visible_child_name("palettes");
            }
        }
    ));
    // "Palettes view →" on the Picker just flips the same switch, so there is one
    // source of truth for which page is showing.
    manage_btn.connect_clicked(clone!(
        #[weak]
        view_pal,
        move |_| view_pal.set_active(true)
    ));

    // load and wire state
    let data = load_data();
    let initial = data.history.first().copied();
    let state = State {
        data,
        current: initial,
        swatch: swatch.clone(),
        code_label: code_label.clone(),
        fmt_hex: fmt_hex.clone(),
        fmt_rgb: fmt_rgb.clone(),
        fmt_hsl: fmt_hsl.clone(),
        palettes_list: palettes_list.clone(),
        pinned_box: pinned_box.clone(),
    };
    let shared: SharedState = Rc::new(RefCell::new(state));

    // First run: write the samples, once. Guarded by `seeded`, not by "are the
    // files there" — so deleting one keeps it deleted, which is the whole point.
    // Tips off means no seeding either: someone who has turned the tour off has
    // not asked for demo files.
    {
        let mut s = shared.borrow_mut();
        if s.data.tips && !s.data.seeded {
            match write_samples() {
                Ok(_) => {
                    s.data.seeded = true;
                    let _ = save_data(&s.data);
                }
                // A read-only data dir must not stop the app opening.
                Err(e) => eprintln!("n.cover: could not write samples: {e}"),
            }
        }
    }

    // The panel is always built; the toggle decides whether it is on screen. It
    // used to only exist when `tips` was true, which is exactly why there was no
    // way back once you dismissed it.
    let tips = build_tips(&window, &tips_btn);
    tips.set_visible(shared.borrow().data.tips);
    outer.prepend(&tips);

    // Palettes collapse state, restored and persisted. (History no longer has a
    // section, so its old `open_history` flag is simply unused now.)
    pal_exp.set_expanded(shared.borrow().data.open_palettes);
    pal_exp.connect_expanded_notify(clone!(
        #[strong]
        shared,
        move |e| {
            let mut s = shared.borrow_mut();
            if s.data.open_palettes != e.is_expanded() {
                s.data.open_palettes = e.is_expanded();
                let _ = save_data(&s.data);
            }
        }
    ));

    tips_btn.set_active(shared.borrow().data.tips);
    tips_btn.connect_toggled(clone!(
        #[strong]
        shared,
        #[weak]
        tips,
        move |b| {
            let on = b.is_active();
            tips.set_visible(on);
            let mut s = shared.borrow_mut();
            if s.data.tips != on {
                s.data.tips = on;
                let _ = save_data(&s.data);
            }
        }
    ));

    image_slot.append(&build_image_view(&window, &shared));

    // swatch draw
    {
        let shared = shared.clone();
        swatch.set_draw_func(move |_, cr, w, h| {
            let s = shared.borrow();
            let (r, g, b) = match s.current {
                Some(c) => (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0),
                None => (0.93, 0.93, 0.93),
            };
            cr.set_source_rgb(r, g, b);
            cr.rectangle(0.0, 0.0, w as f64, h as f64);
            let _ = cr.fill();
            // border
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.15);
            cr.set_line_width(1.0);
            cr.rectangle(0.5, 0.5, w as f64 - 1.0, h as f64 - 1.0);
            let _ = cr.stroke();
        });
    }

    // initial UI sync
    {
        let s = shared.borrow();
        refresh_format_toggles(&s);
        refresh_code(&s);
        refresh_history_ui(&s, &window, &shared);
        refresh_palettes_all(&s, &window, &shared);
    }

    // pick
    {
        let shared = shared.clone();
        let window_ref = window.clone();
        pick_btn.connect_clicked(move |_| {
            let shared_inner = shared.clone();
            let window_inner = window_ref.clone();
            // hide window so the picker overlay isn't obscured by us
            window_ref.set_visible(false);
            pick_color(&window_ref, move |c| {
                {
                    let mut s = shared_inner.borrow_mut();
                    s.current = Some(c);
                    s.data.history.retain(|x| *x != c);
                    s.data.history.insert(0, c);
                    s.data.history.truncate(HISTORY_LIMIT);
                    let _ = save_data(&s.data);
                }
                let s = shared_inner.borrow();
                refresh_swatch(&s);
                refresh_code(&s);
                refresh_history_ui(&s, &window_inner, &shared_inner);
                window_inner.set_visible(true);
                window_inner.present();
            });
        });
    }

    // format toggles
    let connect_fmt = |btn: &ToggleButton, fmt: Format, shared: &SharedState| {
        let shared = shared.clone();
        btn.connect_toggled(move |b| {
            if !b.is_active() {
                return;
            }
            {
                let mut s = shared.borrow_mut();
                if s.data.format == fmt {
                    return;
                }
                s.data.format = fmt;
                let _ = save_data(&s.data);
            }
            let s = shared.borrow();
            refresh_code(&s);
            // history rows display in current format too
            drop(s);
            let s = shared.borrow();
            // can't pass window cheaply here; rebuild via stored ref isn't worth it.
            // We just queue a redraw on existing rows by rebuilding.
            // To avoid plumbing window in, fire a synthetic signal: use widget root.
            if let Some(root) = s.pinned_box.root() {
                if let Some(win) = root.downcast_ref::<ApplicationWindow>() {
                    refresh_history_ui(&s, win, &shared);
                    refresh_palettes_ui(&s, win, &shared);
                }
            }
        });
    };
    connect_fmt(&fmt_hex, Format::Hex, &shared);
    connect_fmt(&fmt_rgb, Format::Rgb, &shared);
    connect_fmt(&fmt_hsl, Format::Hsl, &shared);

    // copy button
    {
        let shared = shared.clone();
        let window = window.clone();
        copy_btn.connect_clicked(move |_| {
            let s = shared.borrow();
            if let Some(c) = s.current {
                copy_to_clipboard(&window, &c.format(s.data.format));
            }
        });
    }

    // (Clear history now lives on the History palette strip in the Palettes
    // section — see refresh_pinned_ui.)

    // import palette
    {
        let shared = shared.clone();
        let window = window.clone();
        import_btn.connect_clicked(move |_| {
            import_palette(&window, &shared);
        });
    }

    // new palette
    {
        let shared = shared.clone();
        let window = window.clone();
        new_pal_btn.connect_clicked(move |_| {
            let dlg = gtk::Window::builder()
                .transient_for(&window)
                .modal(true)
                .title("New palette")
                .default_width(320)
                .build();
            let vbox = GBox::new(Orientation::Vertical, 12);
            vbox.set_margin_top(16);
            vbox.set_margin_bottom(16);
            vbox.set_margin_start(16);
            vbox.set_margin_end(16);
            let entry = Entry::new();
            entry.set_placeholder_text(Some("Palette name"));
            vbox.append(&entry);
            let btnrow = GBox::new(Orientation::Horizontal, 8);
            btnrow.set_halign(gtk::Align::End);
            let cancel = Button::with_label("Cancel");
            let create = Button::with_label("Create");
            create.add_css_class("suggested-action");
            btnrow.append(&cancel);
            btnrow.append(&create);
            vbox.append(&btnrow);
            dlg.set_child(Some(&vbox));
            cancel.connect_clicked(clone!(
                #[weak]
                dlg,
                move |_| dlg.close()
            ));
            create.connect_clicked(clone!(
                #[strong]
                shared,
                #[weak]
                window,
                #[weak]
                entry,
                #[weak]
                dlg,
                move |_| {
                    let name = entry.text().to_string();
                    {
                        let mut s = shared.borrow_mut();
                        s.data.palettes.push(Palette {
                            name,
                            colors: Vec::new(),
                        });
                        let _ = save_data(&s.data);
                    }
                    let s = shared.borrow();
                    refresh_palettes_all(&s, &window, &shared);
                    dlg.close();
                }
            ));
            entry.connect_activate(clone!(
                #[weak]
                create,
                move |_| create.emit_clicked()
            ));
            dlg.present();
        });
    }

    // Save History as a permanent palette. The History palette is derived and
    // rolls over as you pick; this freezes the current one under a name so it
    // stops being a moving target.
    {
        let shared = shared.clone();
        let window = window.clone();
        save_hist_btn.connect_clicked(move |_| {
            let colors: Vec<Rgb> = {
                let s = shared.borrow();
                s.data.history.iter().take(HISTORY_PALETTE).copied().collect()
            };
            if colors.is_empty() {
                show_error(&window, "History is empty — nothing to save yet.");
                return;
            }
            {
                let mut s = shared.borrow_mut();
                // A stable, non-colliding default name; the row is renameable-by
                // -export like any other, and you can edit it after.
                let base = "History";
                let taken: std::collections::HashSet<String> =
                    s.data.palettes.iter().map(|p| p.name.clone()).collect();
                let mut name = base.to_string();
                let mut n = 2;
                while taken.contains(&name) {
                    name = format!("{base} ({n})");
                    n += 1;
                }
                s.data.palettes.push(Palette {
                    name,
                    colors: colors.into_iter().map(Swatch::new).collect(),
                });
                let _ = save_data(&s.data);
            }
            let s = shared.borrow();
            refresh_palettes_all(&s, &window, &shared);
        });
    }

    // CSS
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".code-display { font-family: monospace; font-size: 18px; padding: 4px 8px; }
         .swatch { border-radius: 8px; }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    window.present();
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| install_css());
    app.connect_activate(build_ui);
    app.run()
}

#[cfg(test)]
mod image_tests {
    use super::*;

    #[test]
    fn fit_centres_and_preserves_aspect() {
        // A wide image in a square box: letterboxed, centred vertically.
        let (ox, oy, scale) = fitted(200.0, 100.0, 400.0, 400.0);
        assert_eq!(scale, 2.0);
        assert_eq!(ox, 0.0);
        assert_eq!(oy, 100.0); // (400 - 200) / 2
    }

    #[test]
    fn fit_never_magnifies_past_8x() {
        // A 1px image must not blow up to fill a huge window.
        let (_, _, scale) = fitted(1.0, 1.0, 4000.0, 4000.0);
        assert_eq!(scale, 8.0);
    }

    #[test]
    fn fit_and_hit_test_agree() {
        // The draw and the click MUST use the same mapping, or you pick a
        // different pixel from the one under the cursor. Round-trip the CENTRE
        // of a pixel — a pixel *boundary* is genuinely ambiguous (and, with
        // floating point, lands either side of it), so asserting on one would be
        // testing the arithmetic's rounding rather than the mapping.
        let (pw, ph, ww, wh) = (300.0, 200.0, 400.0, 400.0);
        let (ox, oy, scale) = fitted(pw, ph, ww, wh);
        for (tx, ty) in [(0.0, 0.0), (150.0, 100.0), (299.0, 199.0)] {
            // widget coords of the centre of image pixel (tx, ty)
            let px = ox + (tx + 0.5) * scale;
            let py = oy + (ty + 0.5) * scale;
            let ix = ((px - ox) / scale).floor();
            let iy = ((py - oy) / scale).floor();
            assert_eq!((ix, iy), (tx, ty), "pixel ({tx},{ty}) did not round-trip");
        }
    }
}

#[cfg(test)]
mod pin_tests {
    use super::*;

    #[test]
    fn pin_to_front_is_recency_deduped_and_capped() {
        let mut d = AppData::default();
        for n in ["a", "b", "c"] {
            pin_to_front(&mut d, n);
        }
        assert_eq!(d.pinned, vec!["c", "b", "a"], "newest first");
        // Re-using an existing one moves it to the front, does not duplicate.
        pin_to_front(&mut d, "a");
        assert_eq!(d.pinned, vec!["a", "c", "b"]);
        // A fourth drops the oldest — the Picker view holds a few, not a library.
        pin_to_front(&mut d, "d");
        assert_eq!(d.pinned, vec!["d", "a", "c"]);
        assert!(d.pinned.len() <= PINNED_MAX);
    }

    #[test]
    fn toggle_pin_adds_then_removes() {
        let mut d = AppData::default();
        assert!(toggle_pin(&mut d, "x"), "first toggle pins");
        assert!(is_pinned(&d, "x"));
        assert!(!toggle_pin(&mut d, "x"), "second toggle unpins");
        assert!(!is_pinned(&d, "x"));
        assert!(d.pinned.is_empty());
    }
}

