use anyhow::{anyhow, Result};
use x11::xcursor::{XcursorImageCreate, XcursorImageDestroy, XcursorImageLoadCursor};
use xcb::x;
use xcb::Connection;
use xcb::XidNew;

use crate::color::{self, ARGB};
use crate::draw::draw_magnifying_glass;
use crate::pixel::PixelSquare;
use crate::util::EnsureOdd;

// Left mouse button
const SELECTION_BUTTON: x::Button = 1;

fn grab_event_mask() -> x::EventMask {
    x::EventMask::BUTTON_PRESS | x::EventMask::POINTER_MOTION
}

// Exclusively grabs the pointer so we get all its events
fn grab_pointer(conn: &Connection, root: x::Window, cursor: x::Cursor) -> Result<()> {
    let cookie = conn.send_request(&x::GrabPointer {
        owner_events: false,
        grab_window: root,
        event_mask: grab_event_mask(),
        pointer_mode: x::GrabMode::Async,
        keyboard_mode: x::GrabMode::Async,
        confine_to: x::WINDOW_NONE,
        cursor,
        time: x::CURRENT_TIME,
    });
    let reply = conn.wait_for_reply(cookie)?;
    if reply.status() != x::GrabStatus::Success {
        return Err(anyhow!("Could not grab pointer"));
    }
    Ok(())
}

// Updates the cursor for an _already grabbed pointer_
fn update_cursor(conn: &Connection, cursor: x::Cursor) -> Result<()> {
    conn.check_request(conn.send_request_checked(&x::ChangeActivePointerGrab {
        cursor,
        time: x::CURRENT_TIME,
        event_mask: grab_event_mask(),
    }))?;
    Ok(())
}

fn free_cursor(conn: &Connection, cursor: x::Cursor) {
    let _ = conn.send_request_checked(&x::FreeCursor { cursor });
}

// Creates a new XcursorImage, draws the picker into it and loads it, returning a Cursor.
fn create_new_xcursor(
    conn: &Connection,
    screenshot_pixels: &PixelSquare<&[ARGB]>,
    preview_width: u32,
) -> Result<x::Cursor> {
    let cursor_id = unsafe {
        let cursor_image = XcursorImageCreate(preview_width as i32, preview_width as i32);

        // hot spot — pointer position inside the image
        (*cursor_image).xhot = preview_width / 2;
        (*cursor_image).yhot = preview_width / 2;

        let mut cursor_pixels =
            PixelSquare::from_raw_parts((*cursor_image).pixels, preview_width as usize);

        // pixel size for the picker; must be odd and slightly larger than the
        // ratio between cursor and screenshot to avoid OOB on integer division
        let mut pixel_size = cursor_pixels.width() / screenshot_pixels.width();
        if pixel_size % 2 == 0 {
            pixel_size += 1;
        } else {
            pixel_size += 2;
        }

        draw_magnifying_glass(&mut cursor_pixels, screenshot_pixels, pixel_size);

        let cursor_id = XcursorImageLoadCursor(conn.get_raw_dpy(), cursor_image) as u32;
        XcursorImageDestroy(cursor_image);
        cursor_id
    };
    // Wrap the raw cursor id in xcb's typed Cursor handle
    Ok(XidNew::new(cursor_id))
}

// NOTE: this works for multi-monitor configurations since X fills in blank
// space with empty pixels when XGetImage straddles screens.
fn get_window_rect_around_pointer(
    conn: &Connection,
    screen: &x::Screen,
    (pointer_x, pointer_y): (i16, i16),
    preview_width: u32,
    scale: u32,
) -> Result<(u16, Vec<ARGB>)> {
    let root = screen.root();
    let root_width = screen.width_in_pixels() as isize;
    let root_height = screen.height_in_pixels() as isize;

    let size = ((preview_width / scale) as isize).ensure_odd();

    // top-left of the rect; clamp to non-negative
    let mut x = (pointer_x as isize) - (size / 2);
    let mut y = (pointer_y as isize) - (size / 2);
    let x_offset = if x < 0 { -x } else { 0 };
    let y_offset = if y < 0 { -y } else { 0 };
    x += x_offset;
    y += y_offset;

    let size_x = if x + size > root_width {
        root_width - x
    } else {
        size - x_offset
    };
    let size_y = if y + size > root_height {
        root_height - y
    } else {
        size - y_offset
    };

    let rect = (x as i16, y as i16, size_x as u16, size_y as u16);
    let screenshot_rect = color::window_rect(conn, root, rect)?;

    if size_x == size && size_y == size {
        return Ok((size as u16, screenshot_rect));
    }

    // clamp + pad with transparent pixels
    let mut pixels = vec![ARGB::TRANSPARENT; (size * size) as usize];
    for x in 0..size_x {
        for y in 0..size_y {
            let screenshot_idx = (y * size_x) + x;
            let pixels_idx = (y + y_offset) * size + (x + x_offset);
            pixels[pixels_idx as usize] = screenshot_rect[screenshot_idx as usize];
        }
    }

    Ok((size as u16, pixels))
}

fn create_new_cursor(
    conn: &Connection,
    screen: &x::Screen,
    preview_width: u32,
    scale: u32,
    point: Option<(i16, i16)>,
) -> Result<x::Cursor> {
    let point = match point {
        Some(point) => point,
        None => {
            let root = screen.root();
            let cookie = conn.send_request(&x::QueryPointer { window: root });
            let reply = conn.wait_for_reply(cookie)?;
            (reply.root_x(), reply.root_y())
        }
    };

    let (w, p) = get_window_rect_around_pointer(conn, screen, point, preview_width, scale)?;
    let pixels = PixelSquare::new(&p[..], w.into());
    create_new_xcursor(conn, &pixels, preview_width)
}

pub fn wait_for_location(
    conn: &Connection,
    screen: &x::Screen,
    preview_width: u32,
    scale: u32,
) -> Result<Option<ARGB>> {
    let root = screen.root();
    let preview_width = preview_width.ensure_odd();

    let mut cursor = create_new_cursor(conn, screen, preview_width, scale, None)?;
    grab_pointer(conn, root, cursor)?;

    let result = loop {
        match conn.wait_for_event() {
            Ok(xcb::Event::X(x::Event::ButtonPress(ev))) => {
                if ev.detail() == SELECTION_BUTTON {
                    let pixels = color::window_rect(conn, root, (ev.root_x(), ev.root_y(), 1, 1))?;
                    break Some(pixels[0]);
                }
            }
            Ok(xcb::Event::X(x::Event::MotionNotify(ev))) => {
                let new_cursor = create_new_cursor(
                    conn,
                    screen,
                    preview_width,
                    scale,
                    Some((ev.root_x(), ev.root_y())),
                )?;
                update_cursor(conn, new_cursor)?;
                free_cursor(conn, cursor);
                cursor = new_cursor;
            }
            Ok(_) => {}
            Err(_) => break None,
        }
    };

    let _ = conn.send_request_checked(&x::UngrabPointer {
        time: x::CURRENT_TIME,
    });
    free_cursor(conn, cursor);
    conn.flush()?;

    Ok(result)
}
