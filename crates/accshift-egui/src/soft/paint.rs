//! Frame pacing, egui pass execution, software rasterization, and damage presentation.

use std::num::NonZeroU32;
use std::time::Instant;

use egui::{ViewportEvent, ViewportId};
use egui_software_backend::BufferMutRef;
use egui_winit::winit::event_loop::ActiveEventLoop;

use crate::bench;

#[cfg(target_os = "linux")]
use super::platform::resize_edge;
use super::platform::{os_wants_full_paint, show_window};
use super::present::{frame_buffer, read_pixels};
use super::{Live, Runner, HIDDEN_FRAME_INTERVAL};

impl Runner {
    pub(super) fn repaint_at(&mut self, when: Instant) {
        let interval = if self.live.as_ref().is_some_and(Live::hidden) {
            HIDDEN_FRAME_INTERVAL
        } else if self.armed {
            self.frame_interval * 2
        } else {
            self.frame_interval
        };
        let when = match self.last_paint {
            Some(last) => when.max(last + interval),
            None => when,
        };
        self.next_repaint = Some(self.next_repaint.map_or(when, |t| t.min(when)));
    }

    pub(super) fn paint(&mut self, event_loop: &ActiveEventLoop) {
        let ctx = self.ctx.clone();
        let now = Instant::now();
        let healing = self.heal_due.first().is_some_and(|due| *due <= now);
        self.heal_due.retain(|due| *due > now);
        let Some(live) = &mut self.live else { return };

        egui_winit::update_viewport_info(&mut live.info, &ctx, &live.window, false);
        let mut raw = live.state.take_egui_input(&live.window);
        raw.viewports.insert(ViewportId::ROOT, live.info.clone());
        live.info.events.clear();
        let zoom = live.shell.stage_zoom(&ctx);
        let zoomed = (zoom - 1.0).abs() > 1e-6;
        if (zoom - live.stage_zoom).abs() > 1e-6 {
            live.stage_zoom = zoom;
            live.full_present = true;
        }
        let script_due = live.script.as_mut().and_then(|script| {
            let sf = live.window.scale_factor() as f32;
            let size = live.window.inner_size();
            script.feed(
                &mut raw,
                egui::pos2(size.width as f32, size.height as f32) / (2.0 * sf),
            )
        });
        live.shell.input_hook(&ctx, &mut raw, zoom);
        let mut retry = None;

        let started = Instant::now();
        self.last_paint = Some(started);
        let shell = &mut live.shell;
        crate::zoom::begin_frame(&ctx, zoom, &raw);
        let mut out = ctx.run_ui(raw, |ui| {
            shell.frame(ui);
            if let Some(due) = script_due {
                ui.ctx().request_repaint_after(due);
            }
        });
        let ui_done = Instant::now();
        let mut overlay_continuous = false;
        let overlay_draw = live.shell.take_overlay().map(|(overlay_ctx, overlay_out)| {
            let overlay_hits = overlay_ctx.viewport(|vp| !vp.hits.contains_pointer.is_empty());
            if overlay_hits || overlay_out.platform_output.cursor_icon != egui::CursorIcon::Default
            {
                out.platform_output.cursor_icon = overlay_out.platform_output.cursor_icon;
            }
            out.platform_output
                .commands
                .extend(overlay_out.platform_output.commands);
            overlay_continuous = overlay_out
                .viewport_output
                .get(&ViewportId::ROOT)
                .is_some_and(|v| v.repaint_delay.is_zero());
            live.overlay_textures.append(overlay_out.textures_delta);
            let prims = overlay_ctx.tessellate(overlay_out.shapes, overlay_out.pixels_per_point);
            (
                prims,
                overlay_out.pixels_per_point,
                overlay_out.platform_output.ime,
                overlay_out.platform_output.accesskit_update,
            )
        });
        let overlay_captures = live.shell.overlay_captures();
        #[cfg(target_os = "linux")]
        if let Some(edge) = resize_edge(live) {
            out.platform_output.cursor_icon = edge.cursor();
        }
        if let Some(ime) = &mut out.platform_output.ime {
            // Back from the stage's points to the window's.
            ime.rect = ime.rect * zoom;
            ime.cursor_rect = ime.cursor_rect * zoom;
        }
        let overlay_draw = overlay_draw.map(|(prims, overlay_ppp, ime, tree)| {
            // Overlay points into stage points, so egui-winit's
            // `pixels_per_point` lands on the overlay's physical pixels.
            let ratio = overlay_ppp / out.pixels_per_point;
            out.platform_output.ime = crate::overlay_output::merge_ime(
                out.platform_output.ime,
                ime,
                ratio,
                overlay_captures,
            );
            // One AccessKit tree per window: the overlay's joins the stage's.
            crate::overlay_output::graft(
                &mut out.platform_output.accesskit_update,
                tree,
                overlay_captures,
            );
            (prims, overlay_ppp)
        });
        if overlay_draw.is_none() && overlay_captures {
            out.platform_output.ime = None;
        }
        live.state.handle_platform_output_with_event_loop(
            &live.window,
            event_loop,
            out.platform_output,
        );
        if (live.shell.stage_zoom(&ctx) - zoom).abs() > 1e-6 {
            // The app changed its zoom: the next frame lays out again.
            ctx.request_repaint();
        }
        live.textures.append(out.textures_delta);
        let viewport_output = out.viewport_output.remove(&ViewportId::ROOT);
        // An animation or a scroll asked for the next frame right away.
        let continuous = overlay_continuous
            || viewport_output
                .as_ref()
                .is_some_and(|v| v.repaint_delay.is_zero());
        let commands = viewport_output.map(|v| v.commands).unwrap_or_default();
        let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
        let overlay_active = overlay_draw.as_ref().is_some_and(|(p, _)| !p.is_empty());
        let tessellated = Instant::now();
        let rasterized;
        let mut damaged_px = 0usize;

        let size = live.window.inner_size();
        // A normal run's window waits for boot (or the failsafe); a scene's
        // shows with its first frame, cloaked.
        let show = !live.shown && live.shell.take_show(Instant::now());
        // Hidden in the tray, a frame only runs the UI, as when minimized.
        let hidden = live.hidden();
        let mut screenshot = None;
        let mut fault = None;
        let frame = match (
            NonZeroU32::new(size.width),
            NonZeroU32::new(size.height),
            hidden,
        ) {
            (Some(w), Some(h), false) => {
                match frame_buffer(&mut live.canvas, &mut live.faults, w, h) {
                    Ok(buffer) => Some((w, h, buffer)),
                    Err(err) => {
                        fault = Some(err);
                        None
                    }
                }
            }
            _ => None,
        };
        if let Some((w, h, mut buffer)) = frame {
            // A buffer that still holds the last frame only needs the tiles
            // that changed; a new one (first frame, resize), an active or
            // departing overlay, or a repaint the OS asked for gets everything.
            let full = std::mem::take(&mut live.full_present)
                || healing
                || show
                || overlay_active
                || live.overlay_shown
                || buffer.age() == 0
                || os_wants_full_paint(&live.window);
            // SAFETY: [u8; 4] has the size of u32 and a weaker alignment.
            let pixels = unsafe {
                std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<[u8; 4]>(), buffer.len())
            };
            let window_px = [w.get() as usize, h.get() as usize];
            let canvas_px = if zoomed {
                crate::zoom::stage_size(window_px, zoom)
            } else {
                window_px
            };
            let mut textures = std::mem::take(&mut live.textures);
            live.render.render_to_canvas(
                canvas_px[0],
                canvas_px[1],
                &prims,
                &textures,
                out.pixels_per_point,
            );
            // Applied: epaint asserts a dropped delta was handled.
            textures.clear();
            // Premultiplied BGRA over transparent, and GDI keeps the alpha
            // byte, so the acrylic shows through where egui paints nothing.
            let tiles = if zoomed {
                // The whole stage, then the compositor's scale into the
                // window; the window presents in full.
                live.stage.resize(canvas_px[0] * canvas_px[1], [0; 4]);
                let mut stage = BufferMutRef::new(&mut live.stage, canvas_px[0], canvas_px[1]);
                live.render.copy_canvas_to_buffer(&mut stage, true);
                crate::zoom::resample(&live.stage, canvas_px, pixels, window_px, zoom);
                vec![[0, 0, window_px[0], window_px[1]]]
            } else {
                let mut target = BufferMutRef::new(pixels, window_px[0], window_px[1]);
                live.render.copy_canvas_to_buffer(&mut target, full)
            };
            // Windows may have invalidated the window while this frame
            // rendered. The present validates all of it, so it carries all
            // of it; checked last to leave no frame in between.
            let mut tiles = if !full && !zoomed && os_wants_full_paint(&live.window) {
                let mut target = BufferMutRef::new(pixels, window_px[0], window_px[1]);
                live.render.copy_canvas_to_buffer(&mut target, true)
            } else {
                tiles
            };
            if let Some((overlay_prims, overlay_ppp)) = &overlay_draw {
                let mut overlay_textures = std::mem::take(&mut live.overlay_textures);
                let has_backdrop = overlay_prims
                    .iter()
                    .any(|p| matches!(p.primitive, egui::epaint::Primitive::Callback(_)));
                let backdrop_root = has_backdrop
                    .then(|| live.render.root_pixels())
                    .flatten()
                    .map(|root| {
                        if zoomed {
                            let mut scaled = vec![[0u8; 4]; window_px[0] * window_px[1]];
                            crate::zoom::resample(&root, canvas_px, &mut scaled, window_px, zoom);
                            scaled
                        } else {
                            root
                        }
                    });
                live.overlay_render.render_overlay(
                    pixels,
                    backdrop_root.as_deref(),
                    window_px[0],
                    window_px[1],
                    overlay_prims,
                    &overlay_textures,
                    *overlay_ppp,
                );
                overlay_textures.clear();
                if overlay_active {
                    tiles = vec![[0, 0, window_px[0], window_px[1]]];
                }
            }
            if live.overlay_shown && !overlay_active {
                live.overlay_render.forget_overlay_backdrops();
            }
            live.overlay_shown = overlay_active;
            let damage: Vec<softbuffer::Rect> = tiles
                .into_iter()
                .filter_map(|[x, y, width, height]| {
                    Some(softbuffer::Rect {
                        x: x as u32,
                        y: y as u32,
                        width: NonZeroU32::new(width as u32)?,
                        height: NonZeroU32::new(height as u32)?,
                    })
                })
                .collect();
            rasterized = Instant::now();

            egui_winit::process_viewport_commands(
                &ctx,
                &mut live.info,
                commands,
                &live.window,
                &mut live.actions,
            );
            if live
                .actions
                .iter()
                .any(|a| matches!(a, egui_winit::ActionRequested::Screenshot(_)))
            {
                screenshot = Some(read_pixels(pixels, [w.get() as usize, h.get() as usize]));
            }
            damaged_px = damage
                .iter()
                .map(|r| r.width.get() as usize * r.height.get() as usize)
                .sum();
            if show {
                // The window shows right before a full present: a blit into
                // a hidden window is lost, and the repaint the OS asks for
                // when it shows would be swallowed by the next damaged-tiles
                // present, since softbuffer validates the whole window.
                live.shown = true;
                live.script = bench::Script::from_env();
                // A `--tray` launch never reveals here: it waits in the tray.
                if live.shell.auto_reveal() {
                    show_window(
                        &live.window,
                        live.shell
                            .window
                            .as_ref()
                            .is_some_and(|h| h.focuses_on_show()),
                    );
                }
            }
            match buffer.present_with_damage(&damage) {
                Ok(()) => {
                    live.faults.succeeded();
                    // Startup bench builds only: the shown frame is on screen.
                    #[cfg(feature = "startup-bench")]
                    if show {
                        bench::mark("presented");
                    }
                }
                Err(err) => {
                    live.faults.fail();
                    fault = Some(format!("present: {err}"));
                }
            }
        } else {
            rasterized = Instant::now();
            egui_winit::process_viewport_commands(
                &ctx,
                &mut live.info,
                commands,
                &live.window,
                &mut live.actions,
            );
        }
        if let Some(err) = fault {
            // The frame is lost: the next one draws everything again.
            live.full_present = true;
            if live.faults.logged() {
                live.shell
                    .log_window_error("Skipped a frame the presenter failed to draw", &err);
            }
            retry = Some(Instant::now() + live.faults.retry_after());
        }

        if bench::enabled() {
            let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1000.0;
            let done = Instant::now();
            // What asked for this frame, `file:line` of each request.
            let why: Vec<String> = ctx
                .repaint_causes()
                .iter()
                .take(4)
                .map(|c| {
                    format!(
                        "{}:{}",
                        c.file.rsplit(['/', '\\']).next().unwrap_or(""),
                        c.line
                    )
                })
                .collect();
            bench::mark_with(
                "frame",
                &format!(
                    "\"spans\":{},\"why\":{:?},\"px\":[{},{}],\"damaged\":{},\"ui\":{:.3},\"tessellate\":{:.3},\"raster\":{:.3},\"present\":{:.3},\"held\":{:?}",
                    bench::take_spans(),
                    why.join(" "),
                    size.width,
                    size.height,
                    damaged_px,
                    ms(started, ui_done),
                    ms(ui_done, tessellated),
                    ms(tessellated, rasterized),
                    ms(rasterized, done),
                    live.render.held_bytes(),
                ),
            );
        }

        for action in live.actions.drain(..) {
            let event = match action {
                egui_winit::ActionRequested::Screenshot(callback) => {
                    if let Some(image) = screenshot.clone() {
                        callback.complete(image);
                    }
                    continue;
                }
                // Nothing here sends `RequestPaintWhileHidden`.
                egui_winit::ActionRequested::PaintWhileHidden => continue,
                egui_winit::ActionRequested::Cut => egui::Event::Cut,
                egui_winit::ActionRequested::Copy => egui::Event::Copy,
                egui_winit::ActionRequested::Paste => match live.state.clipboard_text() {
                    Some(text) => egui::Event::Paste(text),
                    None => continue,
                },
            };
            live.state.egui_input_mut().events.push(event);
            ctx.request_repaint();
        }

        if show && !live.shown {
            // A zero-sized frame presented nothing; show anyway.
            live.shown = true;
            if live.shell.auto_reveal() {
                show_window(
                    &live.window,
                    live.shell
                        .window
                        .as_ref()
                        .is_some_and(|h| h.focuses_on_show()),
                );
            }
        }
        // An animation's next frame starts on the vblank that shows this one.
        self.armed =
            continuous && live.shown && !hidden && self.vblank.as_ref().is_some_and(|c| c.arm());
        if live
            .info
            .events
            .iter()
            .any(|e| matches!(e, ViewportEvent::Close))
        {
            self.request_close(event_loop);
        }
        if let Some(&next) = self.heal_due.first() {
            self.repaint_at(next);
        }
        if let Some(at) = retry {
            self.repaint_at(at);
        }
    }
}
