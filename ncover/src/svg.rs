use super::*;

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
pub(crate) struct SvgInfo {
    pub(crate) view_box: Option<String>,
    pub(crate) width: Option<String>,
    pub(crate) height: Option<String>,
    /// Element tag -> count, most common first.
    pub(crate) elements: Vec<(String, usize)>,
    /// font-family values, in first-seen order.
    pub(crate) fonts: Vec<String>,
    /// The actual text content — for label art, this is the copy.
    pub(crate) texts: Vec<String>,
    pub(crate) gradients: usize,
    /// Declared colours, most-used first, with a use count.
    pub(crate) colors: Vec<(Rgb, usize)>,
}

pub(crate) fn clone_info(i: &SvgInfo) -> SvgInfo {
    i.clone()
}

/// CSS named colours we bother with. A full table is 148 entries and mostly
/// noise; these are the ones that actually show up in exported SVG.
pub(crate) fn named_color(s: &str) -> Option<Rgb> {
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
pub(crate) fn parse_svg_color(v: &str) -> Option<Rgb> {
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
pub(crate) fn colors_in_style(style: &str, out: &mut Vec<Rgb>) {
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

pub(crate) fn inspect_svg(text: &str) -> Result<SvgInfo> {
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
