use super::cards::{restack, shape_count, Stack};
use super::layout::{cubic_out, section_items};
use super::*;
use accshift_app::display::{DisplayItem, SectionLabel};

#[test]
fn restack_lifts_the_open_extension_and_sinks_fading_surfaces() {
    let ctx = Context::default();
    let mut order = Vec::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let ctx = ui.ctx().clone();
        let layer = ui.layer_id();
        let count = || shape_count(&ctx, layer);
        let mark = |n: u8| {
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(f32::from(n), 0.0), vec2(1.0, 1.0)),
                0.0,
                egui::Color32::from_gray(n),
            );
        };
        let from = count();
        let mut stacks = Vec::new();
        // A face; a fading surface and its face; an open extension's
        // surface and its face; a face.
        let a = count();
        mark(1);
        stacks.push(Stack {
            range: a..count(),
            surface: None,
            raised: false,
        });
        let b = count();
        mark(2);
        let fading = b..count();
        mark(3);
        stacks.push(Stack {
            range: b..count(),
            surface: Some(fading),
            raised: false,
        });
        let c = count();
        mark(4);
        let open = c..count();
        mark(5);
        stacks.push(Stack {
            range: c..count(),
            surface: Some(open),
            raised: true,
        });
        let d = count();
        mark(6);
        stacks.push(Stack {
            range: d..count(),
            surface: None,
            raised: false,
        });
        restack(&ctx, layer, from..count(), &stacks);
        order = ctx.graphics(|g| {
            g.get(layer)
                .map(|l| {
                    l.all_entries()
                        .skip(from)
                        .map(|c| match &c.shape {
                            egui::Shape::Rect(r) => r.fill.r(),
                            _ => 0,
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
    });
    output.textures_delta.clear();
    assert_eq!(order, vec![2, 1, 3, 6, 4, 5]);
}

#[test]
fn slide_easing_ends_where_it_should() {
    assert_eq!(cubic_out(0.0), 0.0);
    assert_eq!(cubic_out(1.0), 1.0);
    assert!(cubic_out(0.5) > 0.8);
}

#[test]
fn section_items_takes_one_section() {
    use accshift_app::display::SectionHeaderItem;
    let header = |key: &str| {
        DisplayItem::SectionHeader(SectionHeaderItem {
            label: SectionLabel::Root,
            collapse_key: key.into(),
            count: 1,
            card_color: String::new(),
            collapsed: false,
            dragged: false,
        })
    };
    let items = vec![
        header("a"),
        DisplayItem::Back,
        header("b"),
        DisplayItem::Back,
    ];
    assert_eq!(section_items(&items, "a"), vec![DisplayItem::Back]);
    assert_eq!(section_items(&items, "c"), Vec::<DisplayItem>::new());
}
