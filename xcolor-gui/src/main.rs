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

const APP_ID: &str = "io.github.xjmzx.XColorGui";
const HISTORY_LIMIT: usize = 32;

/// How many of the most-recent history colours become the compact "History
/// palette" — a readable strip you scan at a glance, not the whole 32-deep list.
const HISTORY_PALETTE: usize = 10;

/// How many palettes ride along on the Picker view beside the History palette.
const PINNED_MAX: usize = 3;

/// Swatches per row before a palette wraps. Ten reads as a row you can count at
/// a glance; more and it becomes a wall.
const SWATCHES_PER_ROW: usize = 10;

// ---------- color model ----------

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
    fn from_hex(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        Some(Rgb {
            r: u8::from_str_radix(&s[0..2], 16).ok()?,
            g: u8::from_str_radix(&s[2..4], 16).ok()?,
            b: u8::from_str_radix(&s[4..6], 16).ok()?,
        })
    }
    fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
    fn rgb(&self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }
    fn hsl(&self) -> String {
        let (h, s, l) = rgb_to_hsl(self.r, self.g, self.b);
        format!(
            "hsl({}, {}%, {}%)",
            h.round() as i32,
            (s * 100.0).round() as i32,
            (l * 100.0).round() as i32
        )
    }
    fn format(&self, f: Format) -> String {
        match f {
            Format::Hex => self.hex(),
            Format::Rgb => self.rgb(),
            Format::Hsl => self.hsl(),
        }
    }
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d.abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < 1e-6 {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) * 60.0
    } else if (max - g).abs() < 1e-6 {
        ((b - r) / d + 2.0) * 60.0
    } else {
        ((r - g) / d + 4.0) * 60.0
    };
    (h, s, l)
}

// ---------- persistence ----------

#[derive(Default, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Format {
    #[default]
    #[serde(rename = "hex")]
    Hex,
    #[serde(rename = "rgb")]
    Rgb,
    #[serde(rename = "hsl")]
    Hsl,
}

/// A colour in a palette. `Rgb` stays a bare `Copy` triple — it is also the
/// history's element, and a picked colour has no name. A *palette* entry is
/// different: it usually came from somewhere that named it (a design token, a
/// GIMP swatch), and throwing that name away is throwing away the only thing
/// that tells you what the colour is FOR.
///
/// `flatten` keeps the JSON as `{"r":..,"g":..,"b":..,"name":..}`, so a
/// data.json written before names existed still loads — the name is simply
/// absent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Swatch {
    #[serde(flatten)]
    rgb: Rgb,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl Swatch {
    fn new(rgb: Rgb) -> Self {
        Swatch { rgb, name: None }
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
struct Palette {
    name: String,
    colors: Vec<Swatch>,
}

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
        .join("xcolor-gui/data.json")
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

fn pick_color<F>(parent: &ApplicationWindow, on_picked: F)
where
    F: Fn(Rgb) + 'static,
{
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
        .message("xcolor-gui")
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
    flow.set_halign(gtk::Align::Start);
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
    chips.set_halign(gtk::Align::Start);
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
    let t = Label::new(Some("Getting started"));
    t.add_css_class("heading");
    t.set_xalign(0.0);
    t.set_hexpand(true);
    head.append(&t);
    let close = Button::with_label("Don’t show again");
    head.append(&close);
    b.append(&head);

    for line in [
        "Pick — grab any colour on screen. It lands in the history and the clipboard.",
        "Image ▸ Open — artwork (PNG / SVG / JPEG / WebP). Click any pixel to pick from it.",
        "Browse… — point at a FOLDER and scroll every cover in it with ◀ ▶ or the arrow keys.",
        "samples/shapes.svg — an SVG STATES its colours: Inspect lists the exact fills with use counts, not a guess from pixels.",
        "samples/disc-label.svg — Inspect also reads the FONTS and the text. For label art that is most of what you need.",
        "samples/swatches.png — a raster only implies its colours, so “Palette from image” quantises them.",
        "samples/artwork.png — bigger than the canvas and not square, so it must be PLACED: drag to move, scroll to zoom, it snaps to the centre and edges.",
        "Blank + Build — start from a white canvas, Invert to a black one, and stamp centred black squares (scalable) to build label artwork from nothing. The ops stack.",
        "Disc — mask what you framed. Corners can be alpha, white, a colour, or a gradient — taken from the colours you have picked.",
        "Size — 200 to 1000. Changing it keeps your framing: the square grows, the picture stays where you put it.",
        "Batch — the same recipe over a whole folder (a whole discography of cover.png), or over ndisc's PUBLISHED releases straight from the suite. Dry run first; it never writes into your source folder.",
        "Palettes view — a second view (top of the window): your recent colours become a compact History palette, and you pin up to 3 palettes to ride along on the Picker.",
        "Palettes — Import (.gpl/.json), or build one and export to GPL / CSS / JSON. Named swatches keep their names.",
    ] {
        let l = Label::new(Some(&format!("•  {line}")));
        l.set_xalign(0.0);
        l.set_wrap(true);
        l.set_max_width_chars(46);
        l.add_css_class("dim-label");
        b.append(&l);
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

fn build_image_view(window: &ApplicationWindow, shared: &SharedState) -> GBox {
    // The loaded image, shared between the draw handler, the click handler and
    // the palette button.
    let pixbuf: Rc<RefCell<Option<gdk_pixbuf::Pixbuf>>> = Rc::new(RefCell::new(None));
    let name: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    // Set only for SVG. Its presence is what makes the file's DECLARED colours
    // available instead of quantised pixels.
    let svg: Rc<RefCell<Option<SvgInfo>>> = Rc::new(RefCell::new(None));
    // The templated result, when one has been applied. The area draws this in
    // preference to the source, so the disc IS the preview — you are looking at
    // the thing you would save, not an approximation of it.
    let out: Rc<RefCell<Option<gdk_pixbuf::Pixbuf>>> = Rc::new(RefCell::new(None));
    // Where the source sits on the fixed 400x400 canvas.
    let place: Rc<RefCell<Placement>> = Rc::new(RefCell::new(Placement {
        dx: 0.0,
        dy: 0.0,
        scale: 1.0,
    }));
    let grid_on: Rc<RefCell<bool>> = Rc::new(RefCell::new(true));
    // The output size, live. Everything that draws or exports reads it here
    // rather than from a constant.
    let canvas: Rc<Cell<i32>> = Rc::new(Cell::new(sane_canvas(shared.borrow().data.canvas)));
    // Which guides fired on the last move — drawn so you can SEE why the image
    // stopped. A snap you cannot see is just a bug.
    let snapped: Rc<RefCell<(bool, bool)>> = Rc::new(RefCell::new((false, false)));

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
    let area = gtk::DrawingArea::new();
    area.set_content_width(400);
    area.set_content_height(400);
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.add_css_class("frame");

    area.set_draw_func(clone!(
        #[strong]
        canvas,
        #[strong]
        pixbuf,
        #[strong]
        out,
        #[strong]
        place,
        #[strong]
        grid_on,
        #[strong]
        snapped,
        move |_, cr, w, h| {
            let (w, h) = (w as f64, h as f64);

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

            if let Some(pb) = out.borrow().clone() {
                // A template has been applied: THAT is the result, drawn as-is.
                cr.set_source_pixbuf(&pb, 0.0, 0.0);
                if view > 1.0 {
                    cr.source().set_filter(gtk::cairo::Filter::Nearest);
                }
                let _ = cr.paint();
            } else if let Some(pb) = pixbuf.borrow().clone() {
                let p = *place.borrow();
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
            if *grid_on.borrow() && out.borrow().is_none() {
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
            if (sx || sy) && out.borrow().is_none() {
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
        }
    ));

    // Click a pixel -> it becomes the current colour, exactly as a screen pick
    // does, so it flows into the history and the palettes.
    let click = gtk::GestureClick::new();
    click.connect_pressed(clone!(
        #[strong]
        pixbuf,
        #[strong]
        out,
        #[strong]
        shared,
        #[weak]
        window,
        #[weak]
        area,
        move |_, _, px, py| {
            // Pick from what is on screen — if a template is applied, that is
            // the image, and picking the source underneath would be a lie.
            let Some(pb) = out.borrow().clone().or_else(|| pixbuf.borrow().clone()) else {
                return;
            };
            let (w, h) = (area.width() as f64, area.height() as f64);
            let (ox, oy, scale) = fitted(pb.width() as f64, pb.height() as f64, w, h);
            let ix = ((px - ox) / scale).floor();
            let iy = ((py - oy) / scale).floor();
            if ix < 0.0 || iy < 0.0 || ix >= pb.width() as f64 || iy >= pb.height() as f64 {
                return; // clicked the checkerboard, not the image
            }
            let nch = pb.n_channels() as usize;
            let i = iy as usize * pb.rowstride() as usize + ix as usize * nch;
            let bytes = unsafe { pb.pixels() };
            if i + 3 > bytes.len() {
                return;
            }
            let c = Rgb {
                r: bytes[i],
                g: bytes[i + 1],
                b: bytes[i + 2],
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
        place,
        #[strong]
        start,
        move |_, _, _| {
            *start.borrow_mut() = *place.borrow();
        }
    ));
    drag.connect_drag_update(clone!(
        #[strong]
        canvas,
        #[strong]
        place,
        #[strong]
        start,
        #[strong]
        pixbuf,
        #[strong]
        out,
        #[strong]
        snapped,
        #[weak]
        area,
        move |_, ox, oy| {
            if out.borrow().is_some() {
                return; // a template is applied — reset to move it again
            }
            let Some(src) = pixbuf.borrow().clone() else { return };
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
            *place.borrow_mut() = p;
            *snapped.borrow_mut() = (sx, sy);
            area.queue_draw();
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
        place,
        #[strong]
        out,
        #[weak]
        area,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, _, dy| {
            if out.borrow().is_some() {
                return glib::Propagation::Proceed;
            }
            let mut p = place.borrow_mut();
            let c = canvas.get() as f64 / 2.0;
            // Keep the canvas centre fixed under the zoom, so the thing you are
            // looking at does not fly off.
            let old = p.scale;
            let new = (old * if dy < 0.0 { 1.1 } else { 1.0 / 1.1 }).clamp(0.02, 20.0);
            p.dx = c - (c - p.dx) * (new / old);
            p.dy = c - (c - p.dy) * (new / old);
            p.scale = new;
            drop(p);
            area.queue_draw();
            glib::Propagation::Stop
        }
    ));
    area.add_controller(scroll);

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
        let pixbuf = pixbuf.clone();
        let place = place.clone();
        let out = out.clone();
        let area = area.clone();
        let canvas = canvas.clone();
        Rc::new(move |mode: u8| {
            let Some(src) = pixbuf.borrow().clone() else { return };
            if out.borrow().is_some() {
                return;
            }
            let (sw, sh) = (src.width(), src.height());
            let cv = canvas.get();
            let c = cv as f64;
            let mut p = place.borrow_mut();
            match mode {
                0 => *p = Placement::fit(sw, sh, cv),
                1 => *p = Placement::cover(sw, sh, cv),
                2 => {
                    // Centre, keeping the current zoom.
                    p.dx = (c - p.w(sw)) / 2.0;
                    p.dy = (c - p.h(sh)) / 2.0;
                }
                _ => {
                    *p = Placement {
                        scale: 1.0,
                        dx: (c - sw as f64) / 2.0,
                        dy: (c - sh as f64) / 2.0,
                    };
                }
            }
            drop(p);
            area.queue_draw();
        })
    };
    for (b, mode) in [(&b_fit, 0u8), (&b_cover, 1), (&b_centre, 2), (&b_11, 3)] {
        let reframe = reframe.clone();
        b.connect_clicked(move |_| reframe(mode));
    }

    box_.append(&area);

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
    let b_save = Button::with_label("Save PNG…");
    b_save.set_sensitive(false);

    for b in [&b_alpha, &b_white, &b_solid, &b_grad, &b_reset, &b_save] {
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
    // A template already applied is dropped: it was rendered at the old size, and
    // silently keeping it would mean the preview and the size picker disagree.
    size_dd.connect_selected_notify(clone!(
        #[strong]
        canvas,
        #[strong]
        place,
        #[strong]
        out,
        #[strong]
        shared,
        #[weak]
        area,
        #[weak]
        b_save,
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
            *place.borrow_mut() = place.borrow().rescaled(from, want);
            if out.borrow().is_some() {
                *out.borrow_mut() = None;
                b_save.set_sensitive(false);
            }
            {
                let mut s = shared.borrow_mut();
                s.data.canvas = want;
                let _ = save_data(&s.data);
            }
            area.queue_draw();
        }
    ));

    // The current working image: the built-up result if there is one, otherwise
    // the source freshly composed onto the canvas. Every build op reads THIS and
    // writes `out`, which is what lets them stack — invert an image that already
    // has a square, disc-mask something you inverted, and so on.
    let work: Rc<dyn Fn() -> Option<gdk_pixbuf::Pixbuf>> = {
        let pixbuf = pixbuf.clone();
        let place = place.clone();
        let out = out.clone();
        let canvas = canvas.clone();
        Rc::new(move || {
            if let Some(o) = out.borrow().clone() {
                return Some(o);
            }
            let src = pixbuf.borrow().clone()?;
            compose(&src, *place.borrow(), canvas.get()).ok()
        })
    };

    // Commit a built pixbuf as the new working image and show it.
    let commit: Rc<dyn Fn(gdk_pixbuf::Pixbuf)> = {
        let out = out.clone();
        let area = area.clone();
        let b_save = b_save.clone();
        Rc::new(move |pb: gdk_pixbuf::Pixbuf| {
            *out.borrow_mut() = Some(pb);
            b_save.set_sensitive(true);
            area.queue_draw();
        })
    };

    let apply = {
        let work = work.clone();
        let commit = commit.clone();
        let shared = shared.clone();
        let window = window.clone();
        Rc::new(move |fill_kind: u8| {
            let Some(framed) = work() else { return };
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
            match disc_template(&framed, fill) {
                Ok(pb) => commit(pb),
                Err(e) => show_error(&window, &format!("Template failed: {e}")),
            }
        })
    };

    b_invert.connect_clicked(clone!(
        #[strong]
        work,
        #[strong]
        commit,
        #[weak]
        window,
        move |_| {
            let Some(w) = work() else { return };
            match invert_rgb(&w) {
                Ok(pb) => commit(pb),
                Err(e) => show_error(&window, &format!("Invert failed: {e}")),
            }
        }
    ));

    b_square.connect_clicked(clone!(
        #[strong]
        work,
        #[strong]
        commit,
        #[weak]
        sq_scale,
        #[weak]
        window,
        move |_| {
            let Some(w) = work() else { return };
            let frac = sq_scale.value() / 100.0;
            match stamp_square(&w, frac, Rgb { r: 0, g: 0, b: 0 }) {
                Ok(pb) => commit(pb),
                Err(e) => show_error(&window, &format!("Square failed: {e}")),
            }
        }
    ));

    for (b, kind) in [(&b_alpha, 0u8), (&b_white, 1), (&b_solid, 2), (&b_grad, 3)] {
        let apply = apply.clone();
        b.connect_clicked(move |_| apply(kind));
    }
    b_reset.connect_clicked(clone!(
        #[strong]
        out,
        #[weak]
        area,
        #[weak]
        b_save,
        move |_| {
            *out.borrow_mut() = None;
            b_save.set_sensitive(false);
            area.queue_draw();
        }
    ));
    b_save.connect_clicked(clone!(
        #[weak]
        window,
        #[strong]
        out,
        #[strong]
        name,
        move |_| {
            let Some(pb) = out.borrow().clone() else { return };
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
        let pixbuf = pixbuf.clone();
        let place = place.clone();
        let svg = svg.clone();
        let out = out.clone();
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
        let b_save = b_save.clone();
        let area = area.clone();
        let file_lbl = file_lbl.clone();
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
            *pixbuf.borrow_mut() = Some(pb);
            *svg.borrow_mut() = info;
            *out.borrow_mut() = None; // a new image drops any old build/template
            *place.borrow_mut() = Placement::cover(pw, ph, canvas.get());
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
            b_save.set_sensitive(false);
            area.queue_draw();
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
        pixbuf,
        #[strong]
        name,
        #[strong]
        svg,
        #[strong]
        shared,
        move |_| {
            let Some(pb) = pixbuf.borrow().clone() else {
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

    box_
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

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn write_gpl(path: &Path, pal: &Palette) -> Result<()> {
    let mut out = String::new();
    out.push_str("GIMP Palette\n");
    out.push_str(&format!(
        "Name: {}\nColumns: 0\n#\n",
        if pal.name.is_empty() {
            "Unnamed"
        } else {
            &pal.name
        }
    ));
    for s in &pal.colors {
        let c = &s.rgb;
        // GIMP treats everything after the hex as the swatch name, so a named
        // swatch round-trips through .gpl without a custom format.
        match &s.name {
            Some(n) => out.push_str(&format!(
                "{:3} {:3} {:3}\t{}\t{}\n",
                c.r, c.g, c.b, c.hex(), n
            )),
            None => out.push_str(&format!("{:3} {:3} {:3}\t{}\n", c.r, c.g, c.b, c.hex())),
        }
    }
    fs::write(path, out)?;
    Ok(())
}

fn write_css(path: &Path, pal: &Palette) -> Result<()> {
    let mut out = String::from(":root {\n");
    let stem = sanitize(if pal.name.is_empty() {
        "palette"
    } else {
        &pal.name
    });
    for (i, sw) in pal.colors.iter().enumerate() {
        // A named swatch exports as its own name — the whole point of keeping
        // it. Unnamed ones fall back to the positional index as before.
        let key = match &sw.name {
            Some(n) => sanitize(n),
            None => (i + 1).to_string(),
        };
        out.push_str(&format!(
            "  --{}-{}: {};\n",
            stem,
            key,
            sw.rgb.hex().to_lowercase()
        ));
    }
    out.push_str("}\n");
    fs::write(path, out)?;
    Ok(())
}

/// Parse a GIMP palette. Deliberately tolerant: the format is old and every
/// tool emits a slightly different dialect. We need three ints; a hex column
/// and a name are both optional, and anything after the hex is the name (GIMP's
/// own convention, and what `write_gpl` emits).
// ---------- samples ----------
//
// Four demo files, each one there to show a DIFFERENT feature — not decoration.
// Generated at runtime rather than shipped as binary assets: the SVGs are
// strings, the PNGs are a loop, and nothing has to be vendored or kept in step
// with the code.
//
// Seeded ONCE (see AppData::seeded). Delete one and it stays deleted — a demo
// file that keeps coming back is not a demo, it is litter. Only a fresh install
// (no data.json) seeds again, and "Restore samples" is there for a deliberate
// second chance.

/// GTK4 has a real CSS engine, so the styling lives here rather than being
/// hand-rolled in draw calls. Selectors, custom classes, spacing, borders — all
/// of it. Rust is not the constraint; GTK is the toolkit.
const APP_CSS: &str = "
  .section-head { font-weight: 700; }
  .tips-card {
    background: alpha(@accent_bg_color, 0.10);
    border: 1px solid alpha(@accent_bg_color, 0.35);
    border-radius: 10px;
    padding: 10px;
  }
  .tips-card label { font-size: 0.92em; }
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

fn samples_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("xcolor-gui/samples")
}

/// Simple vector shapes with NAMED, declared fills — the file to open to see
/// what SVG introspection actually reads: exact colours with use counts, not a
/// guess from pixels.
const SVG_SHAPES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 400" width="400" height="400">
  <rect width="400" height="400" fill="#131D2A"/>
  <circle cx="120" cy="120" r="70" fill="#7AF0CD"/>
  <rect x="220" y="50" width="140" height="140" rx="12" fill="#A78BFA"/>
  <polygon points="120,360 50,240 190,240" fill="#7DD3FC"/>
  <path d="M290 240 l60 0 l-30 120 z" fill="#FBBF24" stroke="#F87171" stroke-width="4"/>
  <circle cx="200" cy="200" r="26" fill="none" stroke="#4ADE80" stroke-width="8"/>
</svg>
"##;

/// A disc label: the SVG whose FONTS and TEXT the Inspect panel pulls out. For
/// label art that is most of what you want to know before touching anything.
const SVG_LABEL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 400" width="400" height="400">
  <defs>
    <radialGradient id="face" cx="50%" cy="50%" r="50%">
      <stop offset="0%" stop-color="#E0BBFF"/>
      <stop offset="100%" stop-color="#6F00CA"/>
    </radialGradient>
  </defs>
  <circle cx="200" cy="200" r="196" fill="url(#face)"/>
  <circle cx="200" cy="200" r="26" fill="#130023"/>
  <text x="200" y="120" font-family="Helvetica" font-size="34" font-weight="bold"
        text-anchor="middle" fill="#130023">SIDE A</text>
  <text x="200" y="300" font-family="Helvetica" font-size="18"
        text-anchor="middle" fill="#130023">45 RPM  ·  STEREO</text>
</svg>
"##;

/// A raster of flat blocks — open this and hit "Palette from image" to see the
/// quantiser return exactly the colours that are in it.
fn png_swatches() -> Result<gdk_pixbuf::Pixbuf> {
    let cols: [(u8, u8, u8); 8] = [
        (122, 240, 205),
        (125, 211, 252),
        (167, 139, 250),
        (74, 222, 128),
        (251, 191, 36),
        (248, 113, 113),
        (178, 96, 58),
        (19, 29, 42),
    ];
    let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 400, 200)
        .context("alloc")?;
    let row = pb.rowstride() as usize;
    let b = unsafe { pb.pixels() };
    for y in 0..200usize {
        for x in 0..400usize {
            let (r, g, bl) = cols[(x * 8 / 400).min(7)];
            let i = y * row + x * 4;
            b[i] = r;
            b[i + 1] = g;
            b[i + 2] = bl;
            b[i + 3] = 255;
        }
    }
    Ok(pb)
}

/// A deliberately NON-SQUARE, larger-than-canvas raster — the file that makes
/// the point of the 400x400 canvas: it has to be placed, and the disc cuts what
/// you framed rather than what the file happens to contain.
fn png_artwork() -> Result<gdk_pixbuf::Pixbuf> {
    let (w, h) = (900usize, 600usize);
    let pb =
        gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, w as i32, h as i32)
            .context("alloc")?;
    let row = pb.rowstride() as usize;
    let b = unsafe { pb.pixels() };
    for y in 0..h {
        for x in 0..w {
            let fx = x as f64 / w as f64;
            let fy = y as f64 / h as f64;
            // A diagonal sweep with a bright off-centre bloom, so panning and
            // zooming visibly change what lands inside the disc.
            let d = ((fx - 0.32).powi(2) + (fy - 0.4).powi(2)).sqrt();
            let bloom = (1.0 - (d * 2.4)).clamp(0.0, 1.0).powi(2);
            let i = y * row + x * 4;
            b[i] = (30.0 + 200.0 * fx + 25.0 * bloom).min(255.0) as u8;
            b[i + 1] = (20.0 + 90.0 * (1.0 - fy) + 200.0 * bloom).min(255.0) as u8;
            b[i + 2] = (60.0 + 170.0 * fy + 60.0 * bloom).min(255.0) as u8;
            b[i + 3] = 255;
        }
    }
    Ok(pb)
}

/// Write any sample that is not already there. Returns how many were written —
/// existing files are never overwritten, so an edited sample survives.
fn write_samples() -> Result<usize> {
    write_samples_into(&samples_dir())
}

fn write_samples_into(dir: &Path) -> Result<usize> {
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let mut n = 0;

    for (name, body) in [("shapes.svg", SVG_SHAPES), ("disc-label.svg", SVG_LABEL)] {
        let p = dir.join(name);
        if !p.exists() {
            fs::write(&p, body).with_context(|| format!("write {}", p.display()))?;
            n += 1;
        }
    }
    for (name, make) in [
        ("swatches.png", png_swatches as fn() -> Result<gdk_pixbuf::Pixbuf>),
        ("artwork.png", png_artwork),
    ] {
        let p = dir.join(name);
        if !p.exists() {
            make()?
                .savev(&p, "png", &[])
                .with_context(|| format!("write {}", p.display()))?;
            n += 1;
        }
    }
    Ok(n)
}

// ---------- canvas + placement ----------
//
// The output is a SQUARE canvas of a chosen size. A source image of any
// dimensions is PLACED on it — offset and scale — rather than being cropped to
// fit. So the question stops being "how do we squeeze this in" and becomes
// "where on the canvas does this go", which is the one the user is actually
// asking.

/// The offered output sizes. A short list of round numbers, not a free number
/// entry: an arbitrary 437x437 is not a decision anyone means to make, and every
/// size here has to look right at a glance in the picker.
const CANVAS_SIZES: [i32; 5] = [200, 400, 600, 800, 1000];

/// The one you get until you say otherwise.
const CANVAS_DEFAULT: i32 = 400;

/// A stored size that is not one we offer is not honoured — it would be a canvas
/// with no way to get back to it in the UI.
fn sane_canvas(n: i32) -> i32 {
    if CANVAS_SIZES.contains(&n) {
        n
    } else {
        CANVAS_DEFAULT
    }
}

/// How close (in canvas pixels) an edge or centre must come before it snaps.
/// "Medium": firm enough to catch you, loose enough that you can sit 12px off
/// centre on purpose.
const SNAP: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    /// Top-left of the scaled image, in canvas coordinates.
    dx: f64,
    dy: f64,
    scale: f64,
}

impl Placement {
    /// Scale to COVER the canvas, centred — the sane opening position: no gaps,
    /// nothing arbitrary cropped off one side.
    fn cover(sw: i32, sh: i32, c: i32) -> Placement {
        let scale = (c as f64 / sw as f64).max(c as f64 / sh as f64);
        Placement {
            dx: (c as f64 - sw as f64 * scale) / 2.0,
            dy: (c as f64 - sh as f64 * scale) / 2.0,
            scale,
        }
    }

    /// Whole image inside the canvas, centred.
    fn fit(sw: i32, sh: i32, c: i32) -> Placement {
        let scale = (c as f64 / sw as f64).min(c as f64 / sh as f64);
        Placement {
            dx: (c as f64 - sw as f64 * scale) / 2.0,
            dy: (c as f64 - sh as f64 * scale) / 2.0,
            scale,
        }
    }

    /// Rescale a framing from one canvas to another. Changing the output size
    /// must not re-crop what you already framed — the picture stays where you
    /// put it, the square around it just gets bigger.
    fn rescaled(&self, from: i32, to: i32) -> Placement {
        let k = to as f64 / from as f64;
        Placement {
            dx: self.dx * k,
            dy: self.dy * k,
            scale: self.scale * k,
        }
    }
    fn w(&self, sw: i32) -> f64 {
        sw as f64 * self.scale
    }
    fn h(&self, sh: i32) -> f64 {
        sh as f64 * self.scale
    }
}

/// Snap `p` to the canvas's centre and edges. Each axis is considered
/// independently — you can be snapped horizontally while still free vertically,
/// which is what makes it feel like a guide rather than a magnet.
///
/// Returns the snapped placement plus which guides fired, so the view can SHOW
/// why the image stopped moving. A snap you cannot see is just a bug.
fn snap(p: Placement, sw: i32, sh: i32, canvas: i32) -> (Placement, bool, bool) {
    let (mut p, c) = (p, canvas as f64);
    let (iw, ih) = (p.w(sw), p.h(sh));

    // x: left edge, right edge, centre-to-centre.
    let cands_x = [(0.0, 0.0), (c - iw, c), ((c - iw) / 2.0, c / 2.0)];
    let mut sx = None;
    for (target, _) in cands_x {
        if (p.dx - target).abs() <= SNAP {
            p.dx = target;
            sx = Some(target);
            break;
        }
    }
    let cands_y = [(0.0, 0.0), (c - ih, c), ((c - ih) / 2.0, c / 2.0)];
    let mut sy = None;
    for (target, _) in cands_y {
        if (p.dy - target).abs() <= SNAP {
            p.dy = target;
            sy = Some(target);
            break;
        }
    }
    (p, sx.is_some(), sy.is_some())
}

/// Render the placed source onto the fixed 400x400 canvas. Outside the image is
/// transparent — the disc's corner fill decides what happens there, not this.
fn compose(src: &gdk_pixbuf::Pixbuf, p: Placement, canvas: i32) -> Result<gdk_pixbuf::Pixbuf> {
    let dst = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, canvas, canvas)
        .context("could not allocate the canvas")?;
    dst.fill(0x00000000);

    let (sw, sh) = (src.width(), src.height());
    let sbytes = unsafe { src.pixels() };
    let drow = dst.rowstride() as usize;
    let dbytes = unsafe { dst.pixels() };

    for y in 0..canvas {
        for x in 0..canvas {
            // Canvas pixel -> source pixel (nearest: a colour tool must not
            // invent colours that are in neither neighbour).
            let sx = ((x as f64 + 0.5 - p.dx) / p.scale).floor() as i32;
            let sy = ((y as f64 + 0.5 - p.dy) / p.scale).floor() as i32;
            if sx < 0 || sy < 0 || sx >= sw || sy >= sh {
                continue; // left transparent
            }
            let (r, g, b, a) = sample(src, sbytes, sx, sy);
            let i = y as usize * drow + x as usize * 4;
            dbytes[i] = r;
            dbytes[i + 1] = g;
            dbytes[i + 2] = b;
            dbytes[i + 3] = a;
        }
    }
    Ok(dst)
}

// ---------- build primitives ----------
//
// Two operations for BUILDING an image rather than framing one: invert, and a
// centred solid square. Together with a blank canvas they are a minimal kit for
// label artwork — a black square on white, or (invert) a white one on black —
// and they COMPOSE, each one working on what is already on the canvas.

/// A blank opaque-white square, the ground you build a label on. White because a
/// record label is white far more often than not, and Invert reaches black in
/// one click from here.
fn blank_canvas(size: i32) -> Result<gdk_pixbuf::Pixbuf> {
    let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, size, size)
        .context("could not allocate the canvas")?;
    pb.fill(0xFFFF_FFFF); // 0xRRGGBBAA — white, fully opaque
    Ok(pb)
}

/// Invert RGB on a COPY, leaving alpha alone. A colour tool inverts the COLOUR,
/// not the shape: a transparent pixel stays transparent, it does not become an
/// opaque black one.
fn invert_rgb(src: &gdk_pixbuf::Pixbuf) -> Result<gdk_pixbuf::Pixbuf> {
    let pb = src.copy().context("could not copy the image")?;
    let (w, h) = (pb.width(), pb.height());
    let n = pb.n_channels() as usize;
    let stride = pb.rowstride() as usize;
    let bytes = unsafe { pb.pixels() };
    for y in 0..h {
        for x in 0..w {
            let i = y as usize * stride + x as usize * n;
            bytes[i] = 255 - bytes[i];
            bytes[i + 1] = 255 - bytes[i + 1];
            bytes[i + 2] = 255 - bytes[i + 2];
            // channel 3 (alpha), if present, is deliberately untouched
        }
    }
    Ok(pb)
}

/// Composite a filled, centred square onto a COPY of `base`. Side = `frac` of the
/// canvas side (the canvas is square), clamped to [0,1]. Opaque — a label mark is
/// solid, not a tint — so it also stamps over transparent ground, turning it into
/// the square's colour there.
fn stamp_square(base: &gdk_pixbuf::Pixbuf, frac: f64, c: Rgb) -> Result<gdk_pixbuf::Pixbuf> {
    let pb = base.copy().context("could not copy the canvas")?;
    let (w, h) = (pb.width(), pb.height());
    let side = (frac.clamp(0.0, 1.0) * w.min(h) as f64).round() as i32;
    if side <= 0 {
        return Ok(pb); // an empty square is a no-op, not an error
    }
    let (x0, y0) = ((w - side) / 2, (h - side) / 2);
    let n = pb.n_channels() as usize;
    let stride = pb.rowstride() as usize;
    let has_a = pb.has_alpha();
    let bytes = unsafe { pb.pixels() };
    for y in y0..(y0 + side).min(h) {
        for x in x0..(x0 + side).min(w) {
            if x < 0 || y < 0 {
                continue;
            }
            let i = y as usize * stride + x as usize * n;
            bytes[i] = c.r;
            bytes[i + 1] = c.g;
            bytes[i + 2] = c.b;
            if has_a {
                bytes[i + 3] = 255;
            }
        }
    }
    Ok(pb)
}

// ---------- disc template ----------
//
// Mask artwork into a disc — a record label, a CD face — with the corners left
// as one of four things. Done as plain pixel work rather than a cairo round
// trip: the alpha handling is the whole point, and it is easier to be exact
// about it than to un-premultiply someone else's surface.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OuterFill {
    /// The corners are simply not there. The honest default for artwork that
    /// will sit on an unknown background.
    Alpha,
    White,
    Solid(Rgb),
    /// Radial, from the disc's edge outward: `inner` at the rim, `outer` at the
    /// corners.
    Gradient { inner: Rgb, outer: Rgb },
}

fn lerp(a: u8, b: u8, t: f64) -> u8 {
    (a as f64 + (b as f64 - a as f64) * t).round().clamp(0.0, 255.0) as u8
}

/// Nearest-neighbour sample. A colour tool must not invent colours, and bilinear
/// would blend two swatches into a third that is in neither.
fn sample(pb: &gdk_pixbuf::Pixbuf, bytes: &[u8], x: i32, y: i32) -> (u8, u8, u8, u8) {
    let x = x.clamp(0, pb.width() - 1) as usize;
    let y = y.clamp(0, pb.height() - 1) as usize;
    let nch = pb.n_channels() as usize;
    let i = y * pb.rowstride() as usize + x * nch;
    if i + nch > bytes.len() {
        return (0, 0, 0, 0);
    }
    (
        bytes[i],
        bytes[i + 1],
        bytes[i + 2],
        if pb.has_alpha() { bytes[i + 3] } else { 255 },
    )
}

/// Mask `src` into a disc inscribed in a square canvas, filling the corners
/// per `fill`.
///
/// The source is scaled to COVER the square (never letterboxed — a disc with
/// bars through it is not a disc), and the rim is antialiased over one pixel so
/// the edge does not read as a staircase.
fn disc_template(src: &gdk_pixbuf::Pixbuf, fill: OuterFill) -> Result<gdk_pixbuf::Pixbuf> {
    let (sw, sh) = (src.width(), src.height());
    if sw == 0 || sh == 0 {
        anyhow::bail!("empty image");
    }
    let size = sw.max(sh);
    let sbytes = unsafe { src.pixels() };

    let dst = gdk_pixbuf::Pixbuf::new(
        gdk_pixbuf::Colorspace::Rgb,
        true, // always RGBA out: Alpha fill needs it, and the others cost nothing
        8,
        size,
        size,
    )
    .context("could not allocate the output image")?;
    let drow = dst.rowstride() as usize;
    let dbytes = unsafe { dst.pixels() };

    let cx = size as f64 / 2.0;
    let cy = size as f64 / 2.0;
    let radius = size as f64 / 2.0;
    // Cover: the smaller source axis must reach across the square.
    let scale = size as f64 / sw.min(sh) as f64;
    // Gradient normalisation: distance to the furthest PIXEL CENTRE, not to the
    // abstract corner. A pixel's centre is half a pixel inside the corner, so
    // normalising to the corner leaves the corner pixel short of the outer stop
    // (94.7% on a 64px canvas — ask for black->white and the corner comes out
    // #F1F1F1). The furthest thing that actually gets drawn should reach the
    // end of the ramp.
    let corner = ((cx - 0.5).powi(2) + (cy - 0.5).powi(2)).sqrt();

    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            // Coverage: 1 inside, 0 outside, ramped across the last pixel.
            let cov = ((radius - dist) + 0.5).clamp(0.0, 1.0);

            // Source pixel under this destination pixel (cover-scaled, centred).
            let sx = ((x as f64 - cx) / scale + sw as f64 / 2.0).floor() as i32;
            let sy = ((y as f64 - cy) / scale + sh as f64 / 2.0).floor() as i32;
            let (ir, ig, ib, ia) = sample(src, sbytes, sx, sy);

            let (or_, og, ob, oa) = match fill {
                OuterFill::Alpha => (0, 0, 0, 0),
                OuterFill::White => (255, 255, 255, 255),
                OuterFill::Solid(c) => (c.r, c.g, c.b, 255),
                OuterFill::Gradient { inner, outer } => {
                    let t = ((dist - radius) / (corner - radius)).clamp(0.0, 1.0);
                    (
                        lerp(inner.r, outer.r, t),
                        lerp(inner.g, outer.g, t),
                        lerp(inner.b, outer.b, t),
                        255,
                    )
                }
            };

            // Composite the disc over the corner fill by coverage.
            let i = y as usize * drow + x as usize * 4;
            let a_in = ia as f64 / 255.0 * cov;
            let a_out = oa as f64 / 255.0 * (1.0 - cov);
            let a = a_in + a_out;
            if a <= 0.0 {
                dbytes[i] = 0;
                dbytes[i + 1] = 0;
                dbytes[i + 2] = 0;
                dbytes[i + 3] = 0;
                continue;
            }
            let mix = |inside: u8, outside: u8| -> u8 {
                (((inside as f64 * a_in) + (outside as f64 * a_out)) / a)
                    .round()
                    .clamp(0.0, 255.0) as u8
            };
            dbytes[i] = mix(ir, or_);
            dbytes[i + 1] = mix(ig, og);
            dbytes[i + 2] = mix(ib, ob);
            dbytes[i + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    Ok(dst)
}

// ---------- batch ----------
//
// Templating one cover is a demo; templating a discography is the workflow. The
// recipe is what you already dialled in on the canvas — size, framing, disc,
// corner fill — applied across a folder.
//
// What CANNOT be batched is the drag: a hand-placed dx/dy means nothing on the
// next image, which has different dimensions. So batch offers the two framings
// that are defined for any source — Cover and Fit — and says so, rather than
// pretending your placement generalises.

/// What we will open. Anything else in the tree is simply not an image.
const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "svg"];

/// How many result rows the list will show. Beyond this the run is still
/// complete and still counted — the LIST is what is capped, and it says so.
/// A silent truncation reads as "that was everything".
const RESULT_ROWS: usize = 300;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Framing {
    Cover,
    Fit,
}

#[derive(Clone, Copy)]
struct Recipe {
    canvas: i32,
    framing: Framing,
    /// `None` = no disc: just the framed square. The disc is an option, not the
    /// point — resizing a folder of covers is a job on its own.
    disc: Option<OuterFill>,
}

/// Refuse a source/output pairing that would eat itself.
///
/// This is ntree's `guard_deletable` lesson in its non-destructive form: the
/// output folder is a user-typed path, and the originals are the irreplaceable
/// thing. Batch never writes into the source tree — not as an option, not with a
/// confirmation. It also refuses output nested INSIDE the source, which would
/// not overwrite anything but would make the next run batch its own output.
fn guard_batch(src: &Path, out: &Path) -> Result<()> {
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let (s, o) = (real(src), real(out));
    if o == s {
        anyhow::bail!("The output folder is the source folder. Batch never writes over your originals.");
    }
    if o.starts_with(&s) {
        anyhow::bail!(
            "The output folder is inside the source folder.\n\nNothing would be overwritten, but the next run would pick up its own output as input."
        );
    }
    if s.starts_with(&o) {
        anyhow::bail!("The source folder is inside the output folder. Choose an output folder beside it, not above it.");
    }
    Ok(())
}

/// Walk `root` for images whose filename contains `needle` (case-insensitively;
/// empty matches everything). Hidden directories are skipped — nothing in a
/// `.git` is artwork.
fn find_images(root: &Path, needle: &str, into: &mut Vec<PathBuf>) -> Result<()> {
    let needle = needle.to_lowercase();
    let rd = fs::read_dir(root).with_context(|| format!("cannot read {}", root.display()))?;
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort(); // a stable order, so a dry run and the real run agree
    for path in entries {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            // A folder we cannot read is not fatal to the whole run.
            let _ = find_images(&path, &needle, into);
            continue;
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .is_some_and(|e| IMAGE_EXTS.contains(&e.as_str()));
        if ext_ok && (needle.is_empty() || name.to_lowercase().contains(&needle)) {
            into.push(path);
        }
    }
    Ok(())
}

// ---------- published-discography scope ----------
//
// A folder is the obvious source, but the one that makes this part of the suite
// rather than a standalone utility is ndisc's manifest: the exact set of releases
// published to Nostr, in the suite's own scope language. xcolor reading it costs
// nothing and turns "disc-label every published release" into one choice.

/// ndisc's export: the releases it has published, each with its on-disk dir.
/// Only the fields batch needs — serde ignores the rest (id, artist, title…).
#[derive(serde::Deserialize)]
struct Manifest {
    #[serde(rename = "libraryRoot")]
    library_root: PathBuf,
    releases: Vec<ManifestRelease>,
}

#[derive(serde::Deserialize)]
struct ManifestRelease {
    dir: PathBuf,
}

/// Where ndisc writes it. `None` if it has never exported one — in which case
/// the published scope is not offered, because a scope that matches nothing is
/// worse than no scope.
fn published_manifest_path() -> Option<PathBuf> {
    let p = dirs::data_dir()?.join("ndisc-suite").join("published.json");
    p.exists().then_some(p)
}

fn load_manifest(path: &Path) -> Result<Manifest> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let m: Manifest = serde_json::from_str(&text).context("published.json is not the shape we expect")?;
    Ok(m)
}

/// Where a batch run gets its images, and what to strip to mirror the tree.
///
/// The distinction the two scopes share is exactly this: a set of roots to walk,
/// and the prefix that turns an absolute hit into the relpath the output mirrors.
/// For a folder both are the same path; for the manifest the roots are the
/// release dirs and the prefix is the library root — so `artist/release/cover.png`
/// comes out the same shape either way.
struct Scope {
    /// Every directory to search. One for a folder; N release dirs for published.
    roots: Vec<PathBuf>,
    /// Stripped off each hit to form the mirrored relpath.
    strip: PathBuf,
    /// The source boundary the guard protects. The originals live under here.
    guard_root: PathBuf,
}

impl Scope {
    fn folder(root: PathBuf) -> Scope {
        Scope {
            roots: vec![root.clone()],
            strip: root.clone(),
            guard_root: root,
        }
    }

    /// The published releases become the walk roots; the library root is both the
    /// mirror prefix and the boundary the guard protects — the originals ARE the
    /// library, and every release dir lives under it.
    fn published(m: &Manifest) -> Scope {
        Scope {
            roots: m.releases.iter().map(|r| r.dir.clone()).collect(),
            strip: m.library_root.clone(),
            guard_root: m.library_root.clone(),
        }
    }
}

/// Load one image path into the pieces the view needs: the pixbuf, a display
/// name, and — for an SVG — its declared colours/fonts/structure. The single
/// place a file becomes something on the canvas, shared by Open and the browser
/// so they can never drift apart.
fn load_image_file(path: &Path) -> Result<(gdk_pixbuf::Pixbuf, String, Option<SvgInfo>)> {
    // SVG has no intrinsic pixel size worth trusting, so rasterise it big enough
    // to interrogate. PNG/JPEG/WebP load at their own size.
    let is_svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let pb = if is_svg {
        gdk_pixbuf::Pixbuf::from_file_at_scale(path, 1024, 1024, true)
    } else {
        gdk_pixbuf::Pixbuf::from_file(path)
    }
    .with_context(|| format!("could not open {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .to_string();
    // An SVG is read as well as rendered: the raster is for looking at, the parse
    // is for knowing.
    let info = if is_svg {
        fs::read_to_string(path).ok().and_then(|t| inspect_svg(&t).ok())
    } else {
        None
    };
    Ok((pb, name, info))
}

/// Output path: the source tree's shape, mirrored under `out_root`. Always PNG —
/// an alpha corner fill has nowhere to live in a JPEG.
fn batch_dest(rel: &Path, out_root: &Path, disc: bool) -> PathBuf {
    let stem = rel
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .to_string();
    let suffix = if disc { "-disc" } else { "" };
    let mut p = out_root.to_path_buf();
    if let Some(parent) = rel.parent() {
        p.push(parent);
    }
    p.push(format!("{stem}{suffix}.png"));
    p
}

/// Render one source through the recipe to the final pixbuf — read, frame, disc.
/// The ONE place the batch output is produced, so what a Preview shows and what a
/// Run writes are byte-identical by construction, not by two code paths agreeing.
fn batch_render(src: &Path, r: Recipe) -> Result<gdk_pixbuf::Pixbuf> {
    let is_svg = src
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let pb = if is_svg {
        // An SVG has no pixel size worth trusting; rasterise it comfortably
        // above any canvas we offer so scaling down is the only thing we do.
        gdk_pixbuf::Pixbuf::from_file_at_scale(src, 1024, 1024, true)
    } else {
        gdk_pixbuf::Pixbuf::from_file(src)
    }
    .with_context(|| "could not read the image".to_string())?;

    let (w, h) = (pb.width(), pb.height());
    if w == 0 || h == 0 {
        anyhow::bail!("empty image");
    }
    let place = match r.framing {
        Framing::Cover => Placement::cover(w, h, r.canvas),
        Framing::Fit => Placement::fit(w, h, r.canvas),
    };
    let framed = compose(&pb, place, r.canvas)?;
    match r.disc {
        Some(fill) => disc_template(&framed, fill),
        None => Ok(framed),
    }
}

/// One file, start to finish. `dry` renders (so it still fails on a file that
/// would really fail) but does everything except write.
fn batch_one(src: &Path, rel: &Path, out_root: &Path, r: Recipe, dry: bool) -> Result<PathBuf> {
    let final_pb = batch_render(src, r)?;
    let dest = batch_dest(rel, out_root, r.disc.is_some());
    if dry {
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    final_pb
        .savev(&dest, "png", &[])
        .with_context(|| format!("could not write {}", dest.display()))?;
    Ok(dest)
}

/// Progress from the worker thread. Deliberately carries the REASON on failure:
/// "31 failed" is unactionable and sends you guessing at the tool from the
/// outside — the single most valuable thing ntree's sampler learned.
enum BatchMsg {
    Total(usize),
    One {
        rel: String,
        result: std::result::Result<PathBuf, String>,
    },
    Done {
        ok: usize,
        failed: usize,
        stopped: bool,
    },
}

/// How many outputs a Preview renders. A contact sheet is a SAMPLE you eyeball to
/// catch "my recipe is wrong" before it touches 500 files — not the whole run.
const PREVIEW_N: usize = 12;

/// Thumbnail side, px. Big enough to see a bad crop or a wrong corner fill.
const PREVIEW_PX: i32 = 132;

/// A rendered thumbnail on its way back from the worker. Pixbuf is `!Send`, so it
/// cannot cross the thread — the raw RGBA bytes (which ARE Send) do, and the main
/// thread rebuilds the pixbuf. Rendering itself is fine off-thread: it is pure
/// image data, never GTK, exactly as the real run already does.
enum PreviewMsg {
    Total(usize),
    Thumb {
        label: String,
        /// `(rgba, w, h, rowstride, has_alpha)` on success; `None` with a reason
        /// on failure — a preview must show the failures too, or it lies about
        /// what the run will do.
        thumb: Option<(Vec<u8>, i32, i32, i32, bool)>,
        err: Option<String>,
    },
    Done,
}

// ---------- SVG introspection ----------
//
// An SVG *states* its colours; a PNG only implies them. So for an SVG we do not
// sample pixels and guess — we read the declared fills, strokes and gradient
// stops out of the file. That is the difference between "roughly these tones"
// and "these exact values, and here is where each one is used".
//
// Also surfaced: the fonts it asks for, the text it contains, and what it is
// made of. For label art that is most of what you want to know before you touch
// anything.

#[derive(Default, Debug, Clone)]
struct SvgInfo {
    view_box: Option<String>,
    width: Option<String>,
    height: Option<String>,
    /// Element tag -> count, most common first.
    elements: Vec<(String, usize)>,
    /// font-family values, in first-seen order.
    fonts: Vec<String>,
    /// The actual text content — for label art, this is the copy.
    texts: Vec<String>,
    gradients: usize,
    /// Declared colours, most-used first, with a use count.
    colors: Vec<(Rgb, usize)>,
}

fn clone_info(i: &SvgInfo) -> SvgInfo {
    i.clone()
}

/// CSS named colours we bother with. A full table is 148 entries and mostly
/// noise; these are the ones that actually show up in exported SVG.
fn named_color(s: &str) -> Option<Rgb> {
    Some(match s {
        "black" => Rgb { r: 0, g: 0, b: 0 },
        "white" => Rgb { r: 255, g: 255, b: 255 },
        "red" => Rgb { r: 255, g: 0, b: 0 },
        "lime" | "green" => Rgb { r: 0, g: 128, b: 0 },
        "blue" => Rgb { r: 0, g: 0, b: 255 },
        "yellow" => Rgb { r: 255, g: 255, b: 0 },
        "cyan" | "aqua" => Rgb { r: 0, g: 255, b: 255 },
        "magenta" | "fuchsia" => Rgb { r: 255, g: 0, b: 255 },
        "gray" | "grey" => Rgb { r: 128, g: 128, b: 128 },
        "silver" => Rgb { r: 192, g: 192, b: 192 },
        "orange" => Rgb { r: 255, g: 165, b: 0 },
        _ => return None,
    })
}

/// Parse one colour token. Handles `#rgb`, `#rrggbb`, `rgb(r,g,b)` and the
/// common named colours. `none`, `currentColor` and `url(#grad)` are NOT
/// colours — they are references or absences, and inventing a value for them
/// would put a colour in the palette that the file never declares.
fn parse_svg_color(v: &str) -> Option<Rgb> {
    let v = v.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("none") || v.starts_with("url(") {
        return None;
    }
    if let Some(hex) = v.strip_prefix('#') {
        if hex.len() == 3 {
            let f = |c: char| c.to_digit(16).map(|d| (d * 17) as u8);
            let mut it = hex.chars();
            return Some(Rgb {
                r: f(it.next()?)?,
                g: f(it.next()?)?,
                b: f(it.next()?)?,
            });
        }
        return Rgb::from_hex(v);
    }
    if let Some(inner) = v.strip_prefix("rgb(").and_then(|x| x.strip_suffix(')')) {
        let parts: Vec<u8> = inner
            .split(',')
            .filter_map(|p| p.trim().parse::<u8>().ok())
            .collect();
        if parts.len() == 3 {
            return Some(Rgb { r: parts[0], g: parts[1], b: parts[2] });
        }
    }
    named_color(&v.to_ascii_lowercase())
}

/// Pull colour tokens out of a `style="fill:#abc;stroke:red"` attribute.
fn colors_in_style(style: &str, out: &mut Vec<Rgb>) {
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let k = k.trim();
        if k == "fill" || k == "stroke" || k == "stop-color" || k == "flood-color" {
            if let Some(c) = parse_svg_color(v) {
                out.push(c);
            }
        }
    }
}

fn inspect_svg(text: &str) -> Result<SvgInfo> {
    let doc = roxmltree::Document::parse(text).context("not valid XML/SVG")?;
    let root = doc.root_element();

    let mut info = SvgInfo {
        view_box: root.attribute("viewBox").map(str::to_string),
        width: root.attribute("width").map(str::to_string),
        height: root.attribute("height").map(str::to_string),
        ..Default::default()
    };

    let mut tags: HashMap<String, usize> = HashMap::new();
    let mut counts: HashMap<Rgb, usize> = HashMap::new();

    for node in doc.descendants().filter(|n| n.is_element()) {
        let tag = node.tag_name().name().to_string();
        *tags.entry(tag.clone()).or_insert(0) += 1;
        if tag.ends_with("Gradient") {
            info.gradients += 1;
        }

        let mut found: Vec<Rgb> = Vec::new();
        for a in ["fill", "stroke", "stop-color", "flood-color"] {
            if let Some(c) = node.attribute(a).and_then(parse_svg_color) {
                found.push(c);
            }
        }
        if let Some(st) = node.attribute("style") {
            colors_in_style(st, &mut found);
        }
        for c in found {
            *counts.entry(c).or_insert(0) += 1;
        }

        if let Some(f) = node.attribute("font-family") {
            let f = f.trim().trim_matches('\'').trim_matches('"').to_string();
            if !f.is_empty() && !info.fonts.contains(&f) {
                info.fonts.push(f);
            }
        }
        if tag == "text" || tag == "tspan" {
            // ONLY text nodes. descendants() includes the node itself, and an
            // *element*'s .text() returns its first text child — so filtering on
            // .text() alone collects the same string twice.
            let t: String = node
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !t.is_empty() && !info.texts.contains(&t) {
                info.texts.push(t);
            }
        }
    }

    let mut el: Vec<(String, usize)> = tags.into_iter().collect();
    el.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    info.elements = el;

    let mut cs: Vec<(Rgb, usize)> = counts.into_iter().collect();
    cs.sort_by(|a, b| b.1.cmp(&a.1));
    info.colors = cs;

    Ok(info)
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

/// Alpha below this is treated as "not really there" — a transparent corner is
/// not a colour the artwork uses, and letting it into the palette would fill it
/// with the checkerboard.
const ALPHA_FLOOR: u8 = 128;

/// Quantisation bucket: 4 bits per channel (16 levels), i.e. 4096 bins. Coarse
/// enough that a gradient collapses into the few tones a human would name,
/// fine enough to keep two similar-but-distinct brand colours apart.
const QUANT_SHIFT: u8 = 4;

/// The image's dominant colours, most-used first. Near-transparent pixels are
/// skipped; the representative for each bin is the MEAN of the pixels in it, not
/// the bin centre, so the result is a colour the image actually contains.
fn dominant_colors(pb: &gdk_pixbuf::Pixbuf, want: usize) -> Vec<Rgb> {
    let (w, h) = (pb.width(), pb.height());
    let nch = pb.n_channels() as usize;
    let rowstride = pb.rowstride() as usize;
    let has_alpha = pb.has_alpha();
    let bytes = unsafe { pb.pixels() };

    // (count, sum_r, sum_g, sum_b) per bin.
    let mut bins: HashMap<u16, (u64, u64, u64, u64)> = HashMap::new();
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * rowstride + x * nch;
            if i + nch > bytes.len() {
                continue;
            }
            if has_alpha && bytes[i + 3] < ALPHA_FLOOR {
                continue;
            }
            let (r, g, b) = (bytes[i], bytes[i + 1], bytes[i + 2]);
            let key = (((r >> QUANT_SHIFT) as u16) << 8)
                | (((g >> QUANT_SHIFT) as u16) << 4)
                | ((b >> QUANT_SHIFT) as u16);
            let e = bins.entry(key).or_insert((0, 0, 0, 0));
            e.0 += 1;
            e.1 += r as u64;
            e.2 += g as u64;
            e.3 += b as u64;
        }
    }

    let mut v: Vec<(u64, Rgb)> = bins
        .into_values()
        .map(|(n, sr, sg, sb)| {
            (
                n,
                Rgb {
                    r: (sr / n) as u8,
                    g: (sg / n) as u8,
                    b: (sb / n) as u8,
                },
            )
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().take(want).map(|(_, c)| c).collect()
}

fn read_gpl(text: &str) -> Result<Palette> {
    let mut name = String::new();
    let mut colors: Vec<Swatch> = Vec::new();

    for line in text.lines() {
        let line = line.trim_end();
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.eq_ignore_ascii_case("GIMP Palette") {
            continue;
        }
        if let Some(rest) = t.strip_prefix("Name:") {
            name = rest.trim().to_string();
            continue;
        }
        if t.starts_with("Columns:") {
            continue;
        }

        // r g b [\t hex] [\t name]  — or  r g b  name with spaces.
        let mut it = t.split_whitespace();
        let (Some(r), Some(g), Some(b)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let (Ok(r), Ok(g), Ok(b)) = (r.parse::<u8>(), g.parse::<u8>(), b.parse::<u8>()) else {
            continue; // not a colour line — skip rather than fail the import
        };
        // Whatever remains is hex and/or name. Drop a leading hex token; keep
        // the rest verbatim (names contain spaces).
        let rest: Vec<&str> = it.collect();
        let rest = match rest.split_first() {
            Some((first, tail)) if first.starts_with('#') => tail.to_vec(),
            _ => rest,
        };
        let label = rest.join(" ").trim().to_string();
        colors.push(Swatch {
            rgb: Rgb { r, g, b },
            name: if label.is_empty() { None } else { Some(label) },
        });
    }

    if colors.is_empty() {
        anyhow::bail!("no colours found — is this a GIMP palette (.gpl)?");
    }
    if name.is_empty() {
        name = "Imported".into();
    }
    Ok(Palette { name, colors })
}

/// Import a palette. `.gpl` is the interchange format; `.json` reads back what
/// this app exported (so a palette round-trips without loss).
fn read_palette(path: &Path) -> Result<Palette> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let is_json = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if is_json {
        return serde_json::from_str(&text).context("not a palette JSON");
    }
    read_gpl(&text)
}

fn write_json(path: &Path, pal: &Palette) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(pal)?)?;
    Ok(())
}

// ---------- main UI build ----------

fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("XColor")
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
                Err(e) => eprintln!("xcolor-gui: could not write samples: {e}"),
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
mod gpl_tests {
    use super::*;

    #[test]
    fn reads_a_named_palette_and_keeps_the_names() {
        let text = "GIMP Palette\nName: fizx.uk\nColumns: 0\n#\n# a comment\n\
                      9  13  18\t#090D12\tbg\n\
                    122 240 205\t#7AF0CD\taccent\n";
        let p = read_gpl(text).unwrap();
        assert_eq!(p.name, "fizx.uk");
        assert_eq!(p.colors.len(), 2);
        assert_eq!(p.colors[0].rgb, Rgb { r: 9, g: 13, b: 18 });
        assert_eq!(p.colors[0].name.as_deref(), Some("bg"));
        assert_eq!(p.colors[1].name.as_deref(), Some("accent"));
    }

    #[test]
    fn reads_a_plain_gimp_palette_with_no_names() {
        // komodo.gpl's shape — the format most tools actually emit.
        let text = "GIMP Palette\nName: komodo\nColumns: 0\n#\n 29 153 142\t#1D998E\n";
        let p = read_gpl(text).unwrap();
        assert_eq!(p.colors.len(), 1);
        assert!(p.colors[0].name.is_none());
    }

    #[test]
    fn tolerates_names_with_spaces_and_a_missing_hex_column() {
        let text = "GIMP Palette\nName: x\n#\n10 20 30 deep sea blue\n";
        let p = read_gpl(text).unwrap();
        assert_eq!(p.colors[0].name.as_deref(), Some("deep sea blue"));
    }

    #[test]
    fn a_file_with_no_colours_is_an_error_not_an_empty_palette() {
        assert!(read_gpl("GIMP Palette\nName: empty\n#\n").is_err());
    }

    #[test]
    fn round_trips_through_write_gpl() {
        let pal = Palette {
            name: "rt".into(),
            colors: vec![
                Swatch { rgb: Rgb { r: 1, g: 2, b: 3 }, name: Some("one".into()) },
                Swatch { rgb: Rgb { r: 4, g: 5, b: 6 }, name: None },
            ],
        };
        let dir = std::env::temp_dir().join("xcolor-gpl-rt.gpl");
        write_gpl(&dir, &pal).unwrap();
        let back = read_gpl(&fs::read_to_string(&dir).unwrap()).unwrap();
        assert_eq!(back.name, pal.name);
        assert_eq!(back.colors, pal.colors);
    }
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
mod svg_tests {
    use super::*;

    const SAMPLE: &str = r##"<svg viewBox="0 0 100 50" width="100" height="50">
      <defs><linearGradient id="g">
        <stop offset="0" stop-color="#ff0000"/>
        <stop offset="1" stop-color="rgb(0, 0, 255)"/>
      </linearGradient></defs>
      <rect fill="#abc" stroke="black" x="0" y="0"/>
      <rect fill="url(#g)"/>
      <path fill="none" stroke="#ff0000"/>
      <text font-family="Helvetica" fill="white">Label Art</text>
    </svg>"##;

    #[test]
    fn reads_the_declared_colours_not_the_pixels() {
        let i = inspect_svg(SAMPLE).unwrap();
        let hexes: Vec<String> = i.colors.iter().map(|(c, _)| c.hex()).collect();
        assert!(hexes.contains(&"#FF0000".to_string())); // stop-color + stroke
        assert!(hexes.contains(&"#0000FF".to_string())); // rgb(...) form
        assert!(hexes.contains(&"#AABBCC".to_string())); // #abc shorthand expands
        assert!(hexes.contains(&"#000000".to_string())); // named "black"
        assert!(hexes.contains(&"#FFFFFF".to_string())); // named "white"
        // #ff0000 is used twice (a stop and a stroke) and must rank first.
        assert_eq!(i.colors[0].0.hex(), "#FF0000");
    }

    #[test]
    fn none_and_url_refs_are_not_colours() {
        // Inventing a value for `none` or `url(#grad)` would put a colour in the
        // palette that the file never declares.
        assert!(parse_svg_color("none").is_none());
        assert!(parse_svg_color("url(#g)").is_none());
        assert!(parse_svg_color("currentColor").is_none());
    }

    #[test]
    fn reads_fonts_text_and_structure() {
        let i = inspect_svg(SAMPLE).unwrap();
        assert_eq!(i.fonts, vec!["Helvetica"]);
        assert_eq!(i.texts, vec!["Label Art"]);
        assert_eq!(i.gradients, 1);
        assert_eq!(i.view_box.as_deref(), Some("0 0 100 50"));
        assert_eq!(i.elements.iter().find(|(t, _)| t == "rect").unwrap().1, 2);
    }

    #[test]
    fn a_style_attribute_is_read_too() {
        let d = r#"<svg><rect style="fill:#123456;stroke:none"/></svg>"#;
        let i = inspect_svg(d).unwrap();
        assert_eq!(i.colors.len(), 1);
        assert_eq!(i.colors[0].0.hex(), "#123456");
    }
}

#[cfg(test)]
mod disc_tests {
    use super::*;

    /// A solid red square, `n`×`n`, fully opaque.
    fn red(n: i32) -> gdk_pixbuf::Pixbuf {
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, n, n).unwrap();
        pb.fill(0xFF0000FF);
        pb
    }

    fn px(pb: &gdk_pixbuf::Pixbuf, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let b = unsafe { pb.pixels() };
        let i = y as usize * pb.rowstride() as usize + x as usize * 4;
        (b[i], b[i + 1], b[i + 2], b[i + 3])
    }

    #[test]
    fn alpha_fill_leaves_the_corners_actually_transparent() {
        let out = disc_template(&red(64), OuterFill::Alpha).unwrap();
        assert_eq!(px(&out, 0, 0).3, 0, "corner must be fully transparent");
        assert_eq!(px(&out, 32, 32), (255, 0, 0, 255), "centre is the artwork");
    }

    #[test]
    fn white_fill_puts_white_in_the_corners_not_transparency() {
        let out = disc_template(&red(64), OuterFill::White).unwrap();
        assert_eq!(px(&out, 0, 0), (255, 255, 255, 255));
        assert_eq!(px(&out, 32, 32), (255, 0, 0, 255));
    }

    #[test]
    fn solid_fill_uses_the_colour_given() {
        let blue = Rgb { r: 0, g: 0, b: 255 };
        let out = disc_template(&red(64), OuterFill::Solid(blue)).unwrap();
        assert_eq!(px(&out, 0, 0), (0, 0, 255, 255));
    }

    #[test]
    fn gradient_runs_from_the_rim_outward() {
        // inner (at the rim) -> outer (at the corner).
        let out = disc_template(
            &red(64),
            OuterFill::Gradient {
                inner: Rgb { r: 0, g: 0, b: 0 },
                outer: Rgb { r: 255, g: 255, b: 255 },
            },
        )
        .unwrap();
        let corner = px(&out, 0, 0);
        assert_eq!(corner, (255, 255, 255, 255), "corner is the OUTER stop");

        // NB an INSCRIBED disc touches the canvas edges, so there is no "outside"
        // at the mid-edge — pixel (63,32) IS the rim. The fill only exists in the
        // four corners. Sample along the diagonal, just outside the rim: it must
        // still be near the INNER stop.
        let near_rim = px(&out, 6, 6);
        assert!(
            near_rim.0 < 128,
            "just past the rim should be near the inner stop, got {near_rim:?}"
        );
    }

    #[test]
    fn a_non_square_source_is_covered_not_letterboxed() {
        // A disc with bars through it is not a disc. A 128x64 source must fill
        // the square canvas, cropping the long axis rather than padding.
        let wide = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 128, 64).unwrap();
        wide.fill(0x00FF00FF);
        let out = disc_template(&wide, OuterFill::Alpha).unwrap();
        assert_eq!(out.width(), 128);
        assert_eq!(out.height(), 128);
        // Top-centre is inside the disc and must be artwork, not padding.
        assert_eq!(px(&out, 64, 4), (0, 255, 0, 255));
    }
}

#[cfg(test)]
mod placement_tests {
    use super::*;

    #[test]
    fn cover_centres_and_fills_the_canvas() {
        // A wide source: scaled so the SHORT axis reaches across, centred, with
        // the long axis overhanging equally both sides.
        let p = Placement::cover(800, 400, 400);
        assert_eq!(p.scale, 1.0); // 400/400 on the short axis
        assert_eq!(p.dy, 0.0);
        assert_eq!(p.dx, -200.0); // (400 - 800) / 2
        // Nothing is left uncovered.
        assert!(p.w(800) >= 400.0 && p.h(400) >= 400.0);
    }

    #[test]
    fn snaps_to_centre() {
        let p = Placement { dx: 3.0, dy: -4.0, scale: 1.0 }; // a 400x400 source
        let (s, sx, sy) = snap(p, 400, 400, 400);
        assert_eq!((s.dx, s.dy), (0.0, 0.0)); // centre == edges here
        assert!(sx && sy);
    }

    #[test]
    fn snaps_each_axis_independently() {
        // Snapped horizontally, free vertically — that is what makes it feel
        // like a guide and not a magnet.
        let p = Placement { dx: 2.0, dy: 100.0, scale: 1.0 };
        let (s, sx, sy) = snap(p, 400, 400, 400);
        assert_eq!(s.dx, 0.0);
        assert_eq!(s.dy, 100.0, "vertical was far from any guide; leave it alone");
        assert!(sx && !sy);
    }

    #[test]
    fn does_not_snap_beyond_the_threshold() {
        // You must be able to sit deliberately off-centre.
        let p = Placement { dx: 25.0, dy: 25.0, scale: 1.0 };
        let (s, sx, sy) = snap(p, 400, 400, 400);
        assert_eq!((s.dx, s.dy), (25.0, 25.0));
        assert!(!sx && !sy);
    }

    #[test]
    fn snaps_a_small_image_to_the_canvas_centre() {
        // A 100x100 image: centre-to-centre means dx = (400-100)/2 = 150.
        let p = Placement { dx: 146.0, dy: 150.0, scale: 1.0 };
        let (s, _, _) = snap(p, 100, 100, 400);
        assert_eq!(s.dx, 150.0);
    }

    #[test]
    fn compose_places_the_image_where_you_put_it() {
        let src = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 10, 10).unwrap();
        src.fill(0xFF0000FF);
        let c = compose(&src, Placement { dx: 100.0, dy: 100.0, scale: 1.0 }, 400).unwrap();
        assert_eq!(c.width(), 400);
        let b = unsafe { c.pixels() };
        let at = |x: i32, y: i32| {
            let i = y as usize * c.rowstride() as usize + x as usize * 4;
            (b[i], b[i + 1], b[i + 2], b[i + 3])
        };
        assert_eq!(at(105, 105), (255, 0, 0, 255), "inside the placed image");
        assert_eq!(at(50, 50).3, 0, "outside it is transparent, not black");
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("xcolor-samples-{name}"));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn seeds_all_four_demos() {
        let d = tmp("all");
        assert_eq!(write_samples_into(&d).unwrap(), 4);
        for f in ["shapes.svg", "disc-label.svg", "swatches.png", "artwork.png"] {
            assert!(d.join(f).exists(), "{f} missing");
        }
        // The shapes SVG must actually parse and declare the colours the tips
        // promise — a demo that does not demo the feature is worse than none.
        let info = inspect_svg(&fs::read_to_string(d.join("shapes.svg")).unwrap()).unwrap();
        assert!(info.colors.len() >= 6);

        // The label SVG must carry the fonts and text the Inspect panel reads.
        let l = inspect_svg(&fs::read_to_string(d.join("disc-label.svg")).unwrap()).unwrap();
        assert_eq!(l.fonts, vec!["Helvetica"]);
        assert!(l.texts.iter().any(|t| t.contains("SIDE A")));
        assert_eq!(l.gradients, 1);
    }

    #[test]
    fn never_overwrites_an_existing_sample() {
        // Edit a sample and it must survive "Restore". Clobbering the user's
        // file to give them a demo back would be an absurd trade.
        let d = tmp("keep");
        write_samples_into(&d).unwrap();
        fs::write(d.join("shapes.svg"), "MINE").unwrap();
        assert_eq!(write_samples_into(&d).unwrap(), 0, "nothing should be rewritten");
        assert_eq!(fs::read_to_string(d.join("shapes.svg")).unwrap(), "MINE");
    }

    #[test]
    fn restore_writes_back_only_what_is_missing() {
        let d = tmp("restore");
        write_samples_into(&d).unwrap();
        fs::remove_file(d.join("artwork.png")).unwrap();
        assert_eq!(write_samples_into(&d).unwrap(), 1);
        assert!(d.join("artwork.png").exists());
    }

    #[test]
    fn the_artwork_demo_is_non_square_and_bigger_than_the_canvas() {
        // Its whole job is to make the point of the canvas: it must NOT fit.
        let d = tmp("shape");
        write_samples_into(&d).unwrap();
        let pb = gdk_pixbuf::Pixbuf::from_file(d.join("artwork.png")).unwrap();
        assert_ne!(pb.width(), pb.height(), "must be non-square");
        assert!(
            pb.width() > CANVAS_DEFAULT && pb.height() > CANVAS_DEFAULT,
            "must overflow the canvas"
        );
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

#[cfg(test)]
mod batch_tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xcolor-batch-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_output_folder_may_not_be_the_source_folder() {
        let d = tmpdir("same");
        assert!(guard_batch(&d, &d).is_err(), "that would overwrite originals");
    }

    #[test]
    fn the_output_folder_may_not_sit_inside_the_source() {
        // Nothing gets overwritten, but the NEXT run would take its own output
        // as input — the batch would compound on itself.
        let src = tmpdir("nested");
        let out = src.join("out");
        fs::create_dir_all(&out).unwrap();
        assert!(guard_batch(&src, &out).is_err());
    }

    #[test]
    fn the_source_may_not_sit_inside_the_output() {
        let out = tmpdir("parent");
        let src = out.join("art");
        fs::create_dir_all(&src).unwrap();
        assert!(guard_batch(&src, &out).is_err());
    }

    #[test]
    fn siblings_are_fine() {
        let base = tmpdir("siblings");
        let (src, out) = (base.join("in"), base.join("out"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&out).unwrap();
        assert!(guard_batch(&src, &out).is_ok());
    }

    #[test]
    fn the_destination_mirrors_the_source_tree() {
        // The shape of the library is the only thing that tells 1,600 files named
        // cover.png apart. Flatten it and they all land on each other.
        let rel = Path::new("Autechre/Amber/cover.jpg");
        let d = batch_dest(rel, Path::new("/tmp/out"), true);
        assert_eq!(d, Path::new("/tmp/out/Autechre/Amber/cover-disc.png"));
        // No disc: no suffix. Always PNG — alpha has nowhere to live in a JPEG.
        let d = batch_dest(rel, Path::new("/tmp/out"), false);
        assert_eq!(d, Path::new("/tmp/out/Autechre/Amber/cover.png"));
    }

    #[test]
    fn discovery_recurses_filters_by_name_and_skips_hidden() {
        let root = tmpdir("find");
        let rel = root.join("Artist/Release");
        fs::create_dir_all(&rel).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        for f in ["cover.png", "back.png", "notes.txt"] {
            fs::write(rel.join(f), b"x").unwrap();
        }
        fs::write(root.join(".git/cover.png"), b"x").unwrap();

        let mut hits = Vec::new();
        find_images(&root, "cover", &mut hits).unwrap();
        assert_eq!(hits, vec![rel.join("cover.png")], "name filter + no dotdirs");

        let mut all = Vec::new();
        find_images(&root, "", &mut all).unwrap();
        assert_eq!(all.len(), 2, "empty filter = every image, but .txt is not one");
    }

    #[test]
    fn changing_the_output_size_keeps_the_framing() {
        // Framing is a decision. Doubling the canvas must not re-crop it — the
        // square grows, the picture stays where it was put.
        let a = Placement::cover(800, 400, 400);
        let b = a.rescaled(400, 800);
        let direct = Placement::cover(800, 400, 800);
        assert!((b.scale - direct.scale).abs() < 1e-9);
        assert!((b.dx - direct.dx).abs() < 1e-9);
        assert!((b.dy - direct.dy).abs() < 1e-9);
    }

    #[test]
    fn a_stored_size_we_do_not_offer_is_not_honoured() {
        // It would be a canvas with no way back to it in the picker.
        assert_eq!(sane_canvas(437), CANVAS_DEFAULT);
        assert_eq!(sane_canvas(1000), 1000);
        assert!(CANVAS_SIZES.contains(&CANVAS_DEFAULT));
    }

    #[test]
    #[ignore] // touches /data/music; run explicitly
    fn preview_render_of_a_real_cover_is_a_square_thumb() {
        let src = std::path::Path::new("/data/music/214/Fuel Cells/cover.jpg");
        let r = Recipe { canvas: 600, framing: Framing::Cover, disc: Some(OuterFill::Alpha) };
        let full = batch_render(src, r).unwrap();
        assert_eq!((full.width(), full.height()), (600, 600));
        let t = full.scale_simple(PREVIEW_PX, PREVIEW_PX, gdk_pixbuf::InterpType::Bilinear).unwrap();
        assert_eq!((t.width(), t.height()), (PREVIEW_PX, PREVIEW_PX));
        // Alpha corner present.
        assert!(t.has_alpha());
    }

    #[test]
    fn batch_writes_a_square_png_at_the_chosen_size() {
        let base = tmpdir("run");
        let (src, out) = (base.join("in"), base.join("out"));
        fs::create_dir_all(src.join("A/B")).unwrap();
        // A deliberately non-square source: the point is that it gets PLACED.
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 90, 60).unwrap();
        pb.fill(0x3366FFFF);
        pb.savev(src.join("A/B/cover.png"), "png", &[]).unwrap();

        let r = Recipe {
            canvas: 200,
            framing: Framing::Cover,
            disc: Some(OuterFill::Alpha),
        };
        let rel = Path::new("A/B/cover.png");
        let dest = batch_one(&src.join(rel), rel, &out, r, false).unwrap();
        assert_eq!(dest, out.join("A/B/cover-disc.png"));
        let got = gdk_pixbuf::Pixbuf::from_file(&dest).unwrap();
        assert_eq!((got.width(), got.height()), (200, 200));
        // Alpha corners: the disc is inscribed, so (0,0) is outside it.
        let b = unsafe { got.pixels() };
        assert_eq!(b[3], 0, "the corner must be transparent");
    }

    fn px(pb: &gdk_pixbuf::Pixbuf, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let n = pb.n_channels() as usize;
        let b = unsafe { pb.pixels() };
        let i = y as usize * pb.rowstride() as usize + x as usize * n;
        (b[i], b[i + 1], b[i + 2], if pb.has_alpha() { b[i + 3] } else { 255 })
    }

    #[test]
    fn a_blank_canvas_is_opaque_white() {
        let pb = blank_canvas(64).unwrap();
        assert_eq!((pb.width(), pb.height()), (64, 64));
        assert_eq!(px(&pb, 0, 0), (255, 255, 255, 255));
        assert_eq!(px(&pb, 63, 63), (255, 255, 255, 255));
    }

    #[test]
    fn invert_flips_rgb_and_leaves_alpha_alone() {
        let src = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 4, 4).unwrap();
        // A semi-transparent teal: 10,200,150 @ alpha 80.
        {
            let n = src.n_channels() as usize;
            let b = unsafe { src.pixels() };
            for i in (0..b.len()).step_by(n) {
                b[i] = 10;
                b[i + 1] = 200;
                b[i + 2] = 150;
                b[i + 3] = 80;
            }
        }
        let inv = invert_rgb(&src).unwrap();
        assert_eq!(
            px(&inv, 1, 1),
            (245, 55, 105, 80),
            "RGB inverted (255-x); alpha untouched — a colour tool inverts colour, not shape"
        );
        // Twice is identity.
        let back = invert_rgb(&inv).unwrap();
        assert_eq!(px(&back, 2, 2), (10, 200, 150, 80));
    }

    #[test]
    fn a_square_is_centred_black_and_opaque() {
        // 100px canvas, 40% square = 40px, centred → spans [30,70).
        let base = blank_canvas(100).unwrap();
        let out = stamp_square(&base, 0.40, Rgb { r: 0, g: 0, b: 0 }).unwrap();
        assert_eq!(px(&out, 50, 50), (0, 0, 0, 255), "centre is inside the square");
        assert_eq!(px(&out, 31, 31), (0, 0, 0, 255), "just inside the top-left corner");
        assert_eq!(px(&out, 28, 28), (255, 255, 255, 255), "just outside stays white");
        assert_eq!(px(&out, 0, 0), (255, 255, 255, 255), "the ground is untouched");
    }

    #[test]
    fn squares_stack_and_a_zero_square_is_a_no_op() {
        // Build: a big square, then a smaller one on top — both black here, but
        // the point is the second reads the first's result, not the ground.
        let base = blank_canvas(100).unwrap();
        let big = stamp_square(&base, 0.80, Rgb { r: 0, g: 0, b: 0 }).unwrap();
        let small = stamp_square(&big, 0.20, Rgb { r: 255, g: 0, b: 0 }).unwrap();
        assert_eq!(px(&small, 50, 50), (255, 0, 0, 255), "the small square is on top");
        assert_eq!(px(&small, 15, 15), (0, 0, 0, 255), "the big one still shows around it");
        // A zero-size square changes nothing.
        let noop = stamp_square(&base, 0.0, Rgb { r: 0, g: 0, b: 0 }).unwrap();
        assert_eq!(px(&noop, 50, 50), (255, 255, 255, 255));
    }

    #[test]
    fn the_manifest_becomes_release_dirs_stripped_to_the_library_root() {
        // The point of published scope: cover.png in each release mirrors to
        // artist/release/ under the output, the same shape a folder walk gives —
        // because the strip prefix is the library root, not each release dir.
        let m: Manifest = serde_json::from_str(
            r#"{
                "libraryRoot": "/data/music",
                "releases": [
                    {"id": 1, "artist": "214", "title": "Fuel Cells", "dir": "/data/music/214/Fuel Cells"},
                    {"id": 4, "artist": "2562", "title": "Aerial", "dir": "/data/music/2562/Aerial"}
                ]
            }"#,
        )
        .unwrap();
        let s = Scope::published(&m);
        assert_eq!(s.roots.len(), 2);
        assert_eq!(s.strip, Path::new("/data/music"));
        assert_eq!(s.guard_root, Path::new("/data/music"));
        // A hit under a release dir strips to artist/release/file.
        let hit = Path::new("/data/music/214/Fuel Cells/cover.png");
        let rel = hit.strip_prefix(&s.strip).unwrap();
        assert_eq!(
            batch_dest(rel, Path::new("/tmp/out"), true),
            Path::new("/tmp/out/214/Fuel Cells/cover-disc.png")
        );
    }

    #[test]
    fn the_guard_protects_the_whole_library_under_published_scope() {
        // Output inside the library would be caught, because guard_root IS the
        // library root — the originals are the entire tree, not one release.
        let m: Manifest = serde_json::from_str(
            r#"{"libraryRoot": "/data/music", "releases": []}"#,
        )
        .unwrap();
        let s = Scope::published(&m);
        assert!(guard_batch(&s.guard_root, Path::new("/data/music/_discs")).is_err());
        assert!(guard_batch(&s.guard_root, Path::new("/data/discs")).is_ok());
    }

    #[test]
    fn a_dry_run_writes_nothing_but_still_opens_every_file() {
        let base = tmpdir("dry");
        let (src, out) = (base.join("in"), base.join("out"));
        fs::create_dir_all(&src).unwrap();
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 40, 40).unwrap();
        pb.savev(src.join("cover.png"), "png", &[]).unwrap();
        let r = Recipe { canvas: 400, framing: Framing::Fit, disc: None };
        let rel = Path::new("cover.png");

        let dest = batch_one(&src.join(rel), rel, &out, r, true).unwrap();
        assert_eq!(dest, out.join("cover.png"));
        assert!(!dest.exists(), "a dry run writes nothing");

        // And a file that would really fail, fails in the dry run too — that is
        // what makes it worth running.
        fs::write(src.join("broken-cover.png"), b"not a png").unwrap();
        let bad = Path::new("broken-cover.png");
        assert!(batch_one(&src.join(bad), bad, &out, r, true).is_err());
    }
}
