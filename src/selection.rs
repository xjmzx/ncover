use anyhow::{anyhow, Error, Result};
use nix::unistd::{self, fork, ForkResult};
use std::fs;
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::io::IntoRawFd;
use std::str::FromStr;
use xcb::x;
use xcb::Connection;

use crate::atoms;

pub fn into_daemon() -> Result<ForkResult> {
    match unsafe { fork() }? {
        parent @ ForkResult::Parent { .. } => Ok(parent),
        child @ ForkResult::Child => {
            unistd::setsid()?;
            std::env::set_current_dir("/")?;
            let dev_null = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/null")?;
            let dev_null_raw = dev_null.into_raw_fd();
            let stdin = io::stdin();
            let stdout = io::stdout();
            let stderr = io::stderr();
            unsafe {
                redirect_to_dev_null(stdin.as_fd().as_raw_fd(), dev_null_raw)?;
                redirect_to_dev_null(stdout.as_fd().as_raw_fd(), dev_null_raw)?;
                redirect_to_dev_null(stderr.as_fd().as_raw_fd(), dev_null_raw)?;
            }
            Ok(child)
        }
    }
}

unsafe fn redirect_to_dev_null(target_fd: i32, dev_null_raw: i32) -> Result<()> {
    // dup2 on raw fds atomically replaces the target stdio fd with /dev/null
    let ret = nix::libc::dup2(dev_null_raw, target_fd);
    if ret < 0 {
        return Err(anyhow!("dup2 failed: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

pub enum Selection {
    Primary,
    Secondary,
    Clipboard,
}

impl FromStr for Selection {
    type Err = Error;

    fn from_str(string: &str) -> Result<Selection, Self::Err> {
        match string {
            "primary" => Ok(Selection::Primary),
            "secondary" => Ok(Selection::Secondary),
            "clipboard" => Ok(Selection::Clipboard),
            _ => Err(anyhow!("Invalid selection")),
        }
    }
}

impl Selection {
    fn to_atom(&self, conn: &Connection) -> Result<x::Atom> {
        Ok(match *self {
            Selection::Primary => atoms::get(conn, "PRIMARY")?,
            Selection::Secondary => atoms::get(conn, "SECONDARY")?,
            Selection::Clipboard => atoms::get(conn, "CLIPBOARD")?,
        })
    }
}

// The selection daemon presented here is not a perfect implementation of the
// ICCCM recommendation. It does not support large transfers or MULTIPLE/TIMESTAMP
// targets, but works in practice for short color codes.

pub fn set_selection(
    conn: &Connection,
    root: x::Window,
    selection: &Selection,
    string: &str,
) -> Result<()> {
    let selection_atom = selection.to_atom(conn)?;
    let utf8_string = atoms::get(conn, "UTF8_STRING")?;
    let targets = atoms::get(conn, "TARGETS")?;

    let window: x::Window = conn.generate_id();

    conn.check_request(conn.send_request_checked(&x::CreateWindow {
        depth: 0,
        wid: window,
        parent: root,
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        border_width: 0,
        class: x::WindowClass::InputOnly,
        visual: x::COPY_FROM_PARENT,
        value_list: &[],
    }))?;

    conn.check_request(conn.send_request_checked(&x::SetSelectionOwner {
        owner: window,
        selection: selection_atom,
        time: x::CURRENT_TIME,
    }))?;

    let owner_cookie = conn.send_request(&x::GetSelectionOwner {
        selection: selection_atom,
    });
    if conn.wait_for_reply(owner_cookie)?.owner() != window {
        return Err(anyhow!("Could not take selection ownership"));
    }

    loop {
        match conn.wait_for_event() {
            Ok(xcb::Event::X(x::Event::SelectionRequest(ev))) => {
                let target = ev.target();
                let property = if target == utf8_string {
                    conn.check_request(conn.send_request_checked(&x::ChangeProperty {
                        mode: x::PropMode::Replace,
                        window: ev.requestor(),
                        property: ev.property(),
                        r#type: target,
                        data: string.as_bytes(),
                    }))?;
                    ev.property()
                } else if target == targets {
                    let payload: [x::Atom; 2] = [targets, utf8_string];
                    conn.check_request(conn.send_request_checked(&x::ChangeProperty {
                        mode: x::PropMode::Replace,
                        window: ev.requestor(),
                        property: ev.property(),
                        r#type: target,
                        data: &payload,
                    }))?;
                    ev.property()
                } else {
                    x::ATOM_NONE
                };

                let response = x::SelectionNotifyEvent::new(
                    ev.time(),
                    ev.requestor(),
                    ev.selection(),
                    target,
                    property,
                );
                conn.check_request(conn.send_request_checked(&x::SendEvent {
                    propagate: false,
                    destination: x::SendEventDest::Window(ev.requestor()),
                    event_mask: x::EventMask::empty(),
                    event: &response,
                }))?;
            }
            Ok(xcb::Event::X(x::Event::SelectionClear(_))) => {
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(())
}
