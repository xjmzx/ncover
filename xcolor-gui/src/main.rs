use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context, Result};
use gio::prelude::*;
use gtk::glib::clone;
use gtk::prelude::*;
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

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
struct Palette {
    name: String,
    colors: Vec<Rgb>,
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
                    if !p.colors.contains(&c) {
                        p.colors.push(c);
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
    for (cidx, color) in pal.colors.iter().enumerate() {
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
        chip.set_tooltip_text(Some(&format!(
            "{} (left: select+copy, right: remove)",
            c.hex()
        )));
        chip_box.append(&chip);
        chips.append(&chip_box);
    }
    row.append(&chips);
    row.append(&Separator::new(Orientation::Horizontal));
    row
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
    for c in &pal.colors {
        out.push_str(&format!("{:3} {:3} {:3}\t{}\n", c.r, c.g, c.b, c.hex()));
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
    for (i, c) in pal.colors.iter().enumerate() {
        out.push_str(&format!(
            "  --{}-{}: {};\n",
            stem,
            i + 1,
            c.hex().to_lowercase()
        ));
    }
    out.push_str("}\n");
    fs::write(path, out)?;
    Ok(())
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
        .default_width(420)
        .default_height(640)
        .build();

    let header = HeaderBar::new();
    let title = Label::new(Some("XColor Picker"));
    title.add_css_class("title");
    header.set_title_widget(Some(&title));
    window.set_titlebar(Some(&header));

    let outer = GBox::new(Orientation::Vertical, 12);
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);
    outer.set_margin_start(12);
    outer.set_margin_end(12);

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

    window.set_child(Some(&outer));

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
