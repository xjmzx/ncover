#![allow(clippy::upper_case_acronyms)]

mod atoms;
mod cli;
mod color;
mod draw;
mod format;
mod location;
mod pixel;
mod selection;
mod util;

use anyhow::{anyhow, Result};
use clap::ArgMatches;
use nix::unistd::ForkResult;
use xcb::Connection;

use crate::cli::get_cli;
use crate::format::{Format, FormatColor, FormatString};
use crate::location::wait_for_location;
use crate::selection::{into_daemon, set_selection, Selection};

const DEFAULT_PREVIEW_SIZE: u32 = 256 - 1;
const DEFAULT_SCALE: u32 = 8;

fn fail(msg: impl AsRef<str>) -> ! {
    eprintln!("error: {}", msg.as_ref());
    std::process::exit(2);
}

fn run(args: &ArgMatches) -> Result<()> {
    let custom_format;
    let simple_format;
    let formatter: &dyn FormatColor = if let Some(custom) = args.get_one::<String>("custom") {
        custom_format = custom
            .parse::<FormatString>()
            .unwrap_or_else(|_| fail("Invalid format string"));
        &custom_format
    } else {
        let fmt_name = args
            .get_one::<String>("format")
            .map(|s| s.as_str())
            .unwrap_or("hex");
        simple_format = fmt_name
            .parse::<Format>()
            .unwrap_or_else(|e| fail(format!("{e}")));
        &simple_format
    };

    let scale = args
        .get_one::<u32>("scale")
        .copied()
        .unwrap_or(DEFAULT_SCALE);
    let preview_size = args
        .get_one::<u32>("preview_size")
        .copied()
        .unwrap_or(DEFAULT_PREVIEW_SIZE);

    let selection = args
        .get_one::<String>("selection")
        .map(|s| s.parse::<Selection>().unwrap_or(Selection::Clipboard));
    // The flag "-s" with no value is also possible; clap's default_missing_value handles it.
    let use_selection = selection.is_some();
    let background = std::env::var("XCOLOR_FOREGROUND").is_err();

    let mut in_parent = true;

    let (conn, screen_num) = Connection::connect_with_xlib_display()?;

    {
        let screen = conn
            .get_setup()
            .roots()
            .nth(screen_num as usize)
            .ok_or_else(|| anyhow!("Could not find screen"))?;
        let root = screen.root();

        if let Some(color) = wait_for_location(&conn, screen, preview_size, scale)? {
            let output = formatter.format(color);

            if use_selection {
                if background {
                    in_parent = match into_daemon()? {
                        ForkResult::Parent { .. } => true,
                        ForkResult::Child => false,
                    }
                }

                if !(background && in_parent) {
                    set_selection(&conn, root, &selection.unwrap(), &output)?;
                }
            } else {
                println!("{}", output);
            }
        }
    }

    if background && in_parent {
        std::mem::forget(conn);
    }

    Ok(())
}

fn main() {
    let args = get_cli().get_matches();
    if let Err(err) = run(&args) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
