use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
    history_list: ListBox,
    palettes_list: ListBox,
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
    while let Some(child) = state.history_list.first_child() {
        state.history_list.remove(&child);
    }
    for (idx, color) in state.data.history.iter().enumerate() {
        let row = build_color_row(*color, state.data.format, window, shared, idx, true);
        state.history_list.append(&row);
    }
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

fn build_color_row(
    color: Rgb,
    fmt: Format,
    window: &ApplicationWindow,
    shared: &SharedState,
    history_idx: usize,
    is_history: bool,
) -> GBox {
    let row = GBox::new(Orientation::Horizontal, 8);
    row.set_margin_top(4);
    row.set_margin_bottom(4);
    row.set_margin_start(6);
    row.set_margin_end(6);

    let chip = DrawingArea::new();
    chip.set_size_request(28, 28);
    chip.set_draw_func(move |_, cr, w, h| {
        cr.set_source_rgb(
            color.r as f64 / 255.0,
            color.g as f64 / 255.0,
            color.b as f64 / 255.0,
        );
        cr.rectangle(0.0, 0.0, w as f64, h as f64);
        let _ = cr.fill();
    });
    row.append(&chip);

    let label = Label::new(Some(&color.format(fmt)));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let copy_btn = Button::from_icon_name("edit-copy-symbolic");
    copy_btn.set_tooltip_text(Some("Copy"));
    copy_btn.connect_clicked(clone!(
        #[weak]
        window,
        move |_| {
            copy_to_clipboard(&window, &color.format(fmt));
        }
    ));
    row.append(&copy_btn);

    let use_btn = Button::from_icon_name("object-select-symbolic");
    use_btn.set_tooltip_text(Some("Set as current color"));
    use_btn.connect_clicked(clone!(
        #[strong]
        shared,
        move |_| {
            let s = shared.borrow_mut();
            let mut s = s;
            s.current = Some(color);
            refresh_swatch(&s);
            refresh_code(&s);
        }
    ));
    row.append(&use_btn);

    if is_history {
        let del_btn = Button::from_icon_name("user-trash-symbolic");
        del_btn.set_tooltip_text(Some("Remove from history"));
        del_btn.connect_clicked(clone!(
            #[strong]
            shared,
            #[weak]
            window,
            move |_| {
                {
                    let mut s = shared.borrow_mut();
                    if history_idx < s.data.history.len() {
                        s.data.history.remove(history_idx);
                    }
                    let _ = save_data(&s.data);
                }
                let s = shared.borrow();
                refresh_history_ui(&s, &window, &shared);
            }
        ));
        row.append(&del_btn);
    }

    row
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
            refresh_palettes_ui(&s, &window, &shared);
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
                    s.data.palettes.remove(idx);
                }
                let _ = save_data(&s.data);
            }
            let s = shared.borrow();
            refresh_palettes_ui(&s, &window, &shared);
        }
    ));
    header.append(&del_btn);

    row.append(&header);

    let chips = GBox::new(Orientation::Horizontal, 4);
    for (cidx, swatch) in pal.colors.iter().enumerate() {
        let color = &swatch.rgb;
        let chip_box = GBox::new(Orientation::Vertical, 0);
        let chip = DrawingArea::new();
        chip.set_size_request(24, 24);
        let c = *color;
        chip.set_draw_func(move |_, cr, w, h| {
            cr.set_source_rgb(c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0);
            cr.rectangle(0.0, 0.0, w as f64, h as f64);
            let _ = cr.fill();
        });
        let click = gtk::GestureClick::new();
        click.set_button(0); // any button
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
                    refresh_palettes_ui(&s, &window, &shared);
                } else {
                    let mut s = shared.borrow_mut();
                    s.current = Some(c);
                    refresh_swatch(&s);
                    refresh_code(&s);
                    copy_to_clipboard(&window, &c.format(s.data.format));
                }
            }
        ));
        chip.add_controller(click);
        chip.set_tooltip_text(Some(&match &swatch.name {
            Some(n) => format!("{n} — {} (left: select+copy, right: remove)", c.hex()),
            None => format!("{} (left: select+copy, right: remove)", c.hex()),
        }));
        chip_box.append(&chip);
        chips.append(&chip_box);
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
                        refresh_palettes_ui(&s, &window, &shared);
                    }
                    Err(e) => show_error(&window, &format!("Import failed: {e}")),
                }
            }
        ),
    );
}

/// The image section: open, view, pick a pixel, extract a palette.
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
    open_btn.set_tooltip_text(Some("Open an image (PNG / SVG / JPEG)"));
    header.append(&open_btn);

    let pal_btn = Button::with_label("Palette from image");
    pal_btn.set_tooltip_text(Some(
        "Extract the image's dominant colours as a new palette",
    ));
    pal_btn.set_sensitive(false);
    header.append(&pal_btn);
    box_.append(&header);

    // 400x400 is the floor, not the size — it expands with the window.
    let area = gtk::DrawingArea::new();
    area.set_content_width(400);
    area.set_content_height(400);
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.add_css_class("frame");

    area.set_draw_func(clone!(
        #[strong]
        pixbuf,
        #[strong]
        out,
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

            let Some(pb) = out.borrow().clone().or_else(|| pixbuf.borrow().clone()) else {
                return;
            };
            let (ox, oy, scale) =
                fitted(pb.width() as f64, pb.height() as f64, w, h);
            cr.save().ok();
            cr.translate(ox, oy);
            cr.scale(scale, scale);
            cr.set_source_pixbuf(&pb, 0.0, 0.0);
            // Nearest-neighbour when magnified: this is a colour tool, and
            // smoothing invents colours that are not in the file.
            if scale > 1.0 {
                cr.source().set_filter(gtk::cairo::Filter::Nearest);
            }
            let _ = cr.paint();
            cr.restore().ok();
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

    let apply = {
        let pixbuf = pixbuf.clone();
        let out = out.clone();
        let shared = shared.clone();
        let area = area.clone();
        let window = window.clone();
        let b_save = b_save.clone();
        Rc::new(move |fill_kind: u8| {
            let Some(src) = pixbuf.borrow().clone() else { return };
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
            match disc_template(&src, fill) {
                Ok(pb) => {
                    *out.borrow_mut() = Some(pb);
                    b_save.set_sensitive(true);
                    area.queue_draw();
                }
                Err(e) => show_error(&window, &format!("Template failed: {e}")),
            }
        })
    };

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
                .title("Save disc")
                .initial_name(format!("{stem}-disc.png"))
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
    open_btn.connect_clicked(clone!(
        #[weak]
        window,
        #[strong]
        pixbuf,
        #[strong]
        name,
        #[strong]
        svg,
        #[strong]
        out,
        #[weak]
        tpl,
        #[weak]
        b_alpha,
        #[weak]
        b_white,
        #[weak]
        b_solid,
        #[weak]
        b_grad,
        #[weak]
        b_reset,
        #[weak]
        b_save,
        #[strong]
        rebuild_inspect,
        #[weak]
        area,
        #[weak]
        file_lbl,
        #[weak]
        pal_btn,
        move |_| {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Images (PNG / SVG / JPEG)"));
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
                    pixbuf,
                    #[strong]
                    name,
                    #[strong]
                    svg,
                    #[strong]
                    out,
                    #[weak]
                    tpl,
                    #[weak]
                    b_alpha,
                    #[weak]
                    b_white,
                    #[weak]
                    b_solid,
                    #[weak]
                    b_grad,
                    #[weak]
                    b_reset,
                    #[weak]
                    b_save,
                    #[strong]
                    rebuild_inspect,
                    #[weak]
                    area,
                    #[weak]
                    file_lbl,
                    #[weak]
                    pal_btn,
                    move |res| {
                        let Ok(file) = res else { return };
                        let Some(path) = file.path() else { return };
                        // SVG has no intrinsic pixel size worth trusting, so
                        // rasterise it big enough to interrogate. PNG/JPEG load
                        // at their own size.
                        let is_svg = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
                        let loaded = if is_svg {
                            gdk_pixbuf::Pixbuf::from_file_at_scale(&path, 1024, 1024, true)
                        } else {
                            gdk_pixbuf::Pixbuf::from_file(&path)
                        };
                        match loaded {
                            Ok(pb) => {
                                let n = path
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("image")
                                    .to_string();
                                // An SVG gets read as well as rendered: the
                                // rasterisation is for looking at, the parse is
                                // for knowing.
                                let info = if is_svg {
                                    fs::read_to_string(&path)
                                        .ok()
                                        .and_then(|t| inspect_svg(&t).ok())
                                } else {
                                    None
                                };
                                file_lbl.set_text(&format!(
                                    "{n}  ·  {}×{}",
                                    pb.width(),
                                    pb.height()
                                ));
                                file_lbl.set_tooltip_text(Some(&path.to_string_lossy()));
                                pal_btn.set_label(if info.is_some() {
                                    "Palette from SVG"
                                } else {
                                    "Palette from image"
                                });
                                pal_btn.set_tooltip_text(Some(if info.is_some() {
                                    "The colours the file DECLARES — exact, not sampled"
                                } else {
                                    "The image's dominant colours, quantised from its pixels"
                                }));
                                *name.borrow_mut() = n;
                                *pixbuf.borrow_mut() = Some(pb);
                                *svg.borrow_mut() = info;
                                *out.borrow_mut() = None; // a new image drops any old template
                                pal_btn.set_sensitive(true);
                                tpl.set_visible(true);
                                for b in [&b_alpha, &b_white, &b_solid, &b_grad, &b_reset] {
                                    b.set_sensitive(true);
                                }
                                b_save.set_sensitive(false);
                                area.queue_draw();
                                rebuild_inspect();
                            }
                            Err(e) => show_error(&window, &format!("Could not open: {e}")),
                        }
                    }
                ),
            );
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
            refresh_palettes_ui(&s, &window, &shared);
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
    let title = Label::new(Some("XColor Picker"));
    title.add_css_class("title");
    header.set_title_widget(Some(&title));
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

    // history section
    let hist_header = GBox::new(Orientation::Horizontal, 8);
    let hist_title = Label::new(Some("History"));
    hist_title.add_css_class("heading");
    hist_title.set_xalign(0.0);
    hist_title.set_hexpand(true);
    hist_header.append(&hist_title);
    let clear_hist = Button::with_label("Clear");
    hist_header.append(&clear_hist);
    outer.append(&hist_header);

    let history_scroll = gtk::ScrolledWindow::new();
    history_scroll.set_min_content_height(120);
    history_scroll.set_max_content_height(200);
    history_scroll.set_vexpand(true);
    let history_list = ListBox::new();
    history_list.set_selection_mode(gtk::SelectionMode::None);
    history_list.add_css_class("boxed-list");
    history_scroll.set_child(Some(&history_list));
    outer.append(&history_scroll);

    // palettes section
    let pal_header = GBox::new(Orientation::Horizontal, 8);
    let pal_title = Label::new(Some("Palettes"));
    pal_title.add_css_class("heading");
    pal_title.set_xalign(0.0);
    pal_title.set_hexpand(true);
    pal_header.append(&pal_title);
    let import_btn = Button::with_label("Import");
    import_btn.set_tooltip_text(Some(
        "Import a palette (.gpl / .json). Swatch names are kept.",
    ));
    pal_header.append(&import_btn);
    let new_pal_btn = Button::with_label("New palette");
    pal_header.append(&new_pal_btn);
    outer.append(&pal_header);

    let pal_scroll = gtk::ScrolledWindow::new();
    pal_scroll.set_min_content_height(140);
    pal_scroll.set_max_content_height(300);
    pal_scroll.set_vexpand(true);
    let palettes_list = ListBox::new();
    palettes_list.set_selection_mode(gtk::SelectionMode::None);
    palettes_list.add_css_class("boxed-list");
    pal_scroll.set_child(Some(&palettes_list));
    outer.append(&pal_scroll);

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

    window.set_child(Some(&columns));

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
        history_list: history_list.clone(),
        palettes_list: palettes_list.clone(),
    };
    let shared: SharedState = Rc::new(RefCell::new(state));
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
        refresh_palettes_ui(&s, &window, &shared);
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
            if let Some(root) = s.history_list.root() {
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

    // clear history
    {
        let shared = shared.clone();
        let window = window.clone();
        clear_hist.connect_clicked(move |_| {
            {
                let mut s = shared.borrow_mut();
                s.data.history.clear();
                let _ = save_data(&s.data);
            }
            let s = shared.borrow();
            refresh_history_ui(&s, &window, &shared);
        });
    }

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
                    refresh_palettes_ui(&s, &window, &shared);
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
