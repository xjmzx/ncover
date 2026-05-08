use clap::{Arg, Command};

pub fn get_cli() -> Command {
    Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::new("format")
                .short('f')
                .long("format")
                .value_name("NAME")
                .help("Output format (defaults to hex)")
                .value_parser(["hex", "HEX", "hex!", "HEX!", "plain", "rgb"])
                .conflicts_with("custom"),
        )
        .arg(
            Arg::new("custom")
                .short('c')
                .long("custom")
                .value_name("FORMAT")
                .help("Custom output format")
                .conflicts_with("format"),
        )
        .arg(
            Arg::new("selection")
                .short('s')
                .long("selection")
                .value_name("SELECTION")
                .num_args(0..=1)
                .default_missing_value("clipboard")
                .value_parser(["primary", "secondary", "clipboard"])
                .help("Output to selection (defaults to clipboard)"),
        )
        .arg(
            Arg::new("scale")
                .short('S')
                .long("scale")
                .value_name("SCALE")
                .value_parser(clap::value_parser!(u32))
                .help("Scale of magnification (defaults to 8)"),
        )
        .arg(
            Arg::new("preview_size")
                .short('P')
                .long("preview-size")
                .value_name("PREVIEW_SIZE")
                .value_parser(clap::value_parser!(u32))
                .help("Size of preview, must be odd (defaults to 255)"),
        )
}
