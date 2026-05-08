use clap_complete::{generate_to, shells};
use std::env;

include!("src/cli.rs");

fn main() {
    let mut cmd = get_cli();
    let out_dir = env::var_os("OUT_DIR").expect("OUT_DIR not set");
    for shell in [shells::Shell::Bash, shells::Shell::Fish, shells::Shell::Zsh] {
        let _ = generate_to(shell, &mut cmd, "xcolor", &out_dir);
    }
}
