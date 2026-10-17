//! The menu a right click on a text field opens: cut, copy, paste and
//! select all, drawn by the app's context menu. An entry reaches the field
//! the next frame as the event its shortcut sends, so the field's editor
//! runs it as it runs the keyboard.

use accshift_app::actions::dialogs::{AppMenuCommand, AppMenuEntry};
use accshift_app::text::Text;
use egui::{Context, Event, Id, Key, Modifiers, Pos2};

use crate::clipboard;

/// The `CustomMenu` ids of the entries start with this.
const PREFIX: &str = "field.";
/// The field a right click asked the menu for, and where.
const REQUEST: &str = "accshift-field-menu/request";
/// The field the open menu belongs to.
const TARGET: &str = "accshift-field-menu/target";
/// The events an entry left for its field.
const PENDING: &str = "accshift-field-menu/pending";

/// A field's frame: a right click on it asks for the menu.
pub fn on_response(ctx: &Context, id: Id, response: &egui::Response) {
    if response.secondary_clicked() {
        if let Some(at) = response.interact_pointer_pos() {
            ctx.data_mut(|d| d.insert_temp(Id::new(REQUEST), (id, at)));
        }
    }
}

/// The app, once a frame: where to open the menu a field asked for, and
/// its entries.
pub fn take_request(ctx: &Context) -> Option<(Pos2, Vec<AppMenuEntry>)> {
    let (id, at) = ctx.data_mut(|d| take::<(Id, Pos2)>(d, REQUEST))?;
    ctx.data_mut(|d| d.insert_temp(Id::new(TARGET), id));
    let entry = |what: &str, key: &str| {
        AppMenuEntry::item(
            format!("{PREFIX}{what}"),
            Text::key(key),
            AppMenuCommand::Custom(format!("{PREFIX}{what}")),
        )
    };
    Some((
        at,
        vec![
            entry("cut", "context.field.cut"),
            entry("copy", "context.field.copy"),
            entry("paste", "context.field.paste"),
            AppMenuEntry::Separator,
            entry("selectAll", "context.field.selectAll"),
        ],
    ))
}

/// The app runs a `CustomMenu` id: false when it is not a field entry.
pub fn run(ctx: &Context, id: &str) -> bool {
    let Some(what) = id.strip_prefix(PREFIX) else {
        return false;
    };
    let Some(target) = ctx.data(|d| d.get_temp::<Id>(Id::new(TARGET))) else {
        return true;
    };
    let event = match what {
        "cut" => Some(Event::Cut),
        "copy" => Some(Event::Copy),
        // The clipboard's text comes back as an `Event::Paste`.
        "paste" => None,
        "selectAll" => Some(Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        }),
        _ => return true,
    };
    if what == "paste" {
        clipboard::request_text(ctx);
    }
    ctx.data_mut(|d| d.insert_temp(Id::new(PENDING), (target, event)));
    // The press on the menu may have taken focus from the field.
    ctx.memory_mut(|m| m.request_focus(target));
    ctx.request_repaint();
    true
}

/// The focused field `id`, before editing: the event an entry left for it.
pub fn take_event(ctx: &Context, id: Id) -> Option<Event> {
    let pending = ctx.data(|d| d.get_temp::<(Id, Option<Event>)>(Id::new(PENDING)))?;
    if pending.0 != id {
        return None;
    }
    ctx.data_mut(|d| take::<(Id, Option<Event>)>(d, PENDING));
    pending.1
}

/// `remove_temp` for a type with no `Default`.
fn take<T: 'static + Clone + Send + Sync>(d: &mut egui::util::IdTypeMap, key: &str) -> Option<T> {
    let value = d.get_temp::<T>(Id::new(key));
    d.remove::<T>(Id::new(key));
    value
}
