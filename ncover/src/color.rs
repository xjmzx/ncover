use super::*;

// ---------- color model ----------

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub(crate) struct Rgb {
    pub(crate) r: u8,
    pub(crate) g: u8,
    pub(crate) b: u8,
}

impl Rgb {
    pub(crate) fn from_hex(s: &str) -> Option<Rgb> {
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
    pub(crate) fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
    pub(crate) fn rgb(&self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }
    pub(crate) fn hsl(&self) -> String {
        let (h, s, l) = rgb_to_hsl(self.r, self.g, self.b);
        format!(
            "hsl({}, {}%, {}%)",
            h.round() as i32,
            (s * 100.0).round() as i32,
            (l * 100.0).round() as i32
        )
    }
    pub(crate) fn format(&self, f: Format) -> String {
        match f {
            Format::Hex => self.hex(),
            Format::Rgb => self.rgb(),
            Format::Hsl => self.hsl(),
        }
    }
}

/// A colour typed by hand, in any of the forms the app shows or a person
/// would write: `#FFFFFF`, `#fff`, `ffffff`, `rgb(255, 255, 255)`,
/// `255, 255, 255`, `255 255 255`, `hsl(0, 0%, 100%)`, or a basic name
/// (`white`, `black`, `red`, …). None for anything else.
pub(crate) fn parse_color(input: &str) -> Option<Rgb> {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    if let Some(c) = named_color(&s) {
        return Some(c);
    }
    let hex = s.trim_start_matches('#');
    if hex.len() == 3 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let d: Vec<u8> = hex.chars().map(|c| c.to_digit(16).unwrap() as u8 * 17).collect();
        return Some(Rgb { r: d[0], g: d[1], b: d[2] });
    }
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Rgb::from_hex(hex);
    }
    let (body, is_hsl) = if let Some(b) = s.strip_prefix("hsl(").and_then(|b| b.strip_suffix(')')) {
        (b, true)
    } else if let Some(b) = s.strip_prefix("rgb(").and_then(|b| b.strip_suffix(')')) {
        (b, false)
    } else {
        (s.as_str(), false)
    };
    let nums: Vec<f32> = body
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(|p| p.trim_end_matches('%').parse::<f32>())
        .collect::<Result<_, _>>()
        .ok()?;
    if nums.len() != 3 {
        return None;
    }
    if is_hsl {
        let (h, sat, l) = (nums[0], nums[1], nums[2]);
        if !(0.0..=100.0).contains(&sat) || !(0.0..=100.0).contains(&l) {
            return None;
        }
        let (r, g, b) = hsl_to_rgb(h.rem_euclid(360.0), sat / 100.0, l / 100.0);
        return Some(Rgb { r, g, b });
    }
    if nums.iter().any(|n| !(0.0..=255.0).contains(n) || n.fract() != 0.0) {
        return None;
    }
    Some(Rgb { r: nums[0] as u8, g: nums[1] as u8, b: nums[2] as u8 })
}

/// The names worth typing: the Basics strip's colours and a few more.
fn named_color(s: &str) -> Option<Rgb> {
    let hex = match s {
        "white" => "FFFFFF",
        "black" => "000000",
        "grey" | "gray" => "808080",
        "silver" | "light grey" | "light gray" | "lightgrey" | "lightgray" => "C0C0C0",
        "dark grey" | "dark gray" | "darkgrey" | "darkgray" => "404040",
        "red" => "FF0000",
        "green" | "lime" => "00FF00",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "cyan" | "aqua" => "00FFFF",
        "magenta" | "fuchsia" => "FF00FF",
        "orange" => "FF8000",
        _ => return None,
    };
    Rgb::from_hex(hex)
}

/// The Basics strip: white, black and three greys, then the RGB primaries and
/// secondaries. Built in and fixed: not a palette, never exported.
pub(crate) const BASICS: [&str; 11] = [
    "FFFFFF", "C0C0C0", "808080", "404040", "000000", "FF0000", "00FF00", "0000FF", "FFFF00", "00FFFF",
    "FF00FF",
];

pub(crate) fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r1), to(g1), to(b1))
}

pub(crate) fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
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
pub(crate) enum Format {
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
pub(crate) struct Swatch {
    #[serde(flatten)]
    pub(crate) rgb: Rgb,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

impl Swatch {
    pub(crate) fn new(rgb: Rgb) -> Self {
        Swatch { rgb, name: None }
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub(crate) struct Palette {
    pub(crate) name: String,
    pub(crate) colors: Vec<Swatch>,
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    fn c(r: u8, g: u8, b: u8) -> Option<Rgb> {
        Some(Rgb { r, g, b })
    }

    #[test]
    fn every_form_the_app_shows_or_a_person_types() {
        for s in ["#FFFFFF", "#fff", "ffffff", "FFF", " #FfFfFf ", "white", "WHITE", "rgb(255, 255, 255)",
            "255, 255, 255", "255 255 255", "hsl(0, 0%, 100%)", "hsl(0 0% 100%)"]
        {
            assert_eq!(parse_color(s), c(255, 255, 255), "{s}");
        }
        assert_eq!(parse_color("#1a2B3c"), c(0x1a, 0x2b, 0x3c));
        assert_eq!(parse_color("hsl(120, 100%, 50%)"), c(0, 255, 0));
        assert_eq!(parse_color("hsl(240, 100%, 25%)"), c(0, 0, 128));
    }

    #[test]
    fn the_app_s_own_text_reads_back_as_the_same_colour() {
        for rgb in [Rgb { r: 12, g: 200, b: 99 }, Rgb { r: 255, g: 128, b: 0 }, Rgb { r: 0, g: 0, b: 0 }] {
            assert_eq!(parse_color(&rgb.hex()), Some(rgb));
            assert_eq!(parse_color(&rgb.rgb()), Some(rgb));
            // HSL text is rounded to whole degrees and percents: within one step.
            let back = parse_color(&rgb.hsl()).unwrap();
            for (a, b) in [(rgb.r, back.r), (rgb.g, back.g), (rgb.b, back.b)] {
                assert!((a as i16 - b as i16).abs() <= 3, "{rgb:?} -> {} -> {back:?}", rgb.hsl());
            }
        }
    }

    #[test]
    fn nonsense_is_not_a_colour() {
        for s in ["", "#ffff", "#gggggg", "rgb(256, 0, 0)", "1, 2", "1.5, 2, 3", "hsl(0, 120%, 50%)", "teal-ish"] {
            assert_eq!(parse_color(s), None, "{s}");
        }
    }

    #[test]
    fn the_basics_are_colours() {
        assert!(BASICS.iter().all(|h| Rgb::from_hex(h).is_some()));
        assert_eq!(Rgb::from_hex(BASICS[0]), parse_color("white"));
    }
}
