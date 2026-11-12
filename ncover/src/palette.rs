use super::*;

pub(crate) fn sanitize(s: &str) -> String {
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

pub(crate) fn write_gpl(path: &Path, pal: &Palette) -> Result<()> {
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

pub(crate) fn write_css(path: &Path, pal: &Palette) -> Result<()> {
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
pub(crate) fn read_gpl(text: &str) -> Result<Palette> {
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
pub(crate) fn read_palette(path: &Path) -> Result<Palette> {
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

pub(crate) fn write_json(path: &Path, pal: &Palette) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(pal)?)?;
    Ok(())
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
