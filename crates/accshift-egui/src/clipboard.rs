//! Reading the clipboard (`navigator.clipboard.readText`). The window's
//! integration answers `ViewportCommand::RequestPaste` with an
//! `Event::Paste` the next frame, and sends nothing when the clipboard holds
//! no text. A scene run stages its own text instead: a scene never reads the
//! machine's clipboard.

use egui::{Context, Event, Id, ViewportCommand};

const STAGED: &str = "accshift-clipboard/staged";
const ASKED: &str = "accshift-clipboard/asked";

/// Asks for the clipboard's text; it comes back as an `Event::Paste`.
pub fn request_text(ctx: &Context) {
    if ctx.data(|d| d.get_temp::<String>(Id::new(STAGED)).is_some()) {
        ctx.data_mut(|d| d.insert_temp(Id::new(ASKED), true));
    } else {
        ctx.send_viewport_cmd(ViewportCommand::RequestPaste);
    }
}

/// This frame's pasted text, if any.
pub fn pasted(ctx: &Context) -> Option<String> {
    ctx.input(|i| {
        i.events.iter().rev().find_map(|e| match e {
            Event::Paste(text) => Some(text.clone()),
            _ => None,
        })
    })
}

/// Scene runs, every frame: the text the scene's clipboard holds, and the
/// answer to a request made last frame.
pub fn stage(ctx: &Context, text: &str, events: &mut Vec<Event>) {
    let asked = ctx.data_mut(|d| {
        d.insert_temp(Id::new(STAGED), text.to_owned());
        d.remove_temp::<bool>(Id::new(ASKED)).unwrap_or(false)
    });
    if asked && !text.is_empty() {
        events.push(Event::Paste(text.to_owned()));
    }
}
