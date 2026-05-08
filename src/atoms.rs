use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use xcb::x;
use xcb::Connection;

fn cache() -> &'static Mutex<HashMap<&'static str, x::Atom>> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, x::Atom>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn get(conn: &Connection, name: &'static str) -> Result<x::Atom> {
    let mut map = cache()
        .lock()
        .map_err(|_| anyhow!("Failed to access atom cache"))?;
    if let Some(atom) = map.get(name) {
        return Ok(*atom);
    }
    let cookie = conn.send_request(&x::InternAtom {
        only_if_exists: false,
        name: name.as_bytes(),
    });
    let atom = conn.wait_for_reply(cookie)?.atom();
    map.insert(name, atom);
    Ok(atom)
}
