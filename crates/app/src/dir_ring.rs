//! Folder preview "ring" (from Atlas's `dir_ring.rs`, AGPL-3.0): the thumbnails of a folder's
//! first pictures on a carousel above the folder's listing in the preview pane. The pointer steers
//! it: the closer to the left or right edge, the faster it spins that way; with the pointer away it
//! eases to the nearest card so one always sits in front.
//!
//! Clicking a card opens that picture; right-clicking it opens a small menu (open, show in folder,
//! copy, ...) in the explorer that owns the ring.
//!
//! The ring keeps its animation state in its own entity, so a spinning frame redraws only the ring
//! and never the pane around it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    Bounds, Context, CursorStyle, Entity, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit,
    Pixels, RenderImage, WeakEntity, Window, canvas, div, img, prelude::*, px,
};

use crate::explorer::Explorer;

/// At most this many pictures ride the ring.
pub(crate) const RING_LIMIT: usize = 7;

const WIDTH: f32 = 236.;
/// Snug around the front card (the box plus the depth drop), so no band of empty space sits
/// above the pictures.
const HEIGHT: f32 = 204.;
/// Box each picture is fitted into; cards behind shrink toward `BACK_SCALE` of it.
const BOX_W: f32 = 130.;
const BOX_H: f32 = 178.;
const BACK_SCALE: f32 = 0.42;
/// Horizontal orbit radius and how far the front card sits below the back ones.
const RADIUS_X: f32 = 104.;
const DEPTH_DROP: f32 = 18.;
/// Cards whose depth (cosine of their angle) is below this are behind the ring and not drawn.
const FRONT_CUTOFF: f32 = -0.2;
/// Pointer offset (of the half width) that still counts as the center.
const DEAD_ZONE: f32 = 0.14;
/// Spin speed (rad/s) with the pointer on the very edge.
const MAX_SPEED: f32 = 4.2;

pub(crate) struct DirRing {
    key: PathBuf,
    items: Vec<Arc<RenderImage>>,
    /// The file each item shows, for opening it and its menu.
    paths: Vec<PathBuf>,
    /// The explorer that opens a clicked card and shows its menu.
    owner: Option<WeakEntity<Explorer>>,
    angle: f32,
    velocity: f32,
    /// Pointer offset in `[-1, 1]` from the ring's center while hovering.
    pointer: Option<f32>,
    bounds: Option<Bounds<Pixels>>,
    last_frame: Option<Instant>,
    /// Scale of the whole ring, so it fills the preview pane's width (1 = 236 px).
    zoom: f32,
}

impl DirRing {
    const fn new() -> Self {
        Self {
            key: PathBuf::new(),
            items: Vec::new(),
            paths: Vec::new(),
            owner: None,
            angle: 0.,
            velocity: 0.,
            pointer: None,
            bounds: None,
            last_frame: None,
            zoom: 1.,
        }
    }

    /// Fills `width` (the preview pane's), between 0.8 and 2.2 times the ring's own size.
    /// Returns whether that changed anything.
    pub(crate) fn set_width(&mut self, width: f32) -> bool {
        let zoom = (width / WIDTH).clamp(0.8, 2.2);
        let changed = (zoom - self.zoom).abs() > 0.01;
        if changed {
            self.zoom = zoom;
        }
        changed
    }

    /// Height the ring takes at its current scale.
    pub(crate) fn height(&self) -> f32 {
        HEIGHT * self.zoom
    }

    /// Replace the cards; another folder restarts the ring at its first card. Whether anything
    /// changed (and the ring needs a redraw).
    pub(crate) fn set_items(
        &mut self,
        key: &Path,
        owner: WeakEntity<Explorer>,
        entries: Vec<(PathBuf, Arc<RenderImage>)>,
    ) -> bool {
        self.owner = Some(owner);
        let (paths, items): (Vec<PathBuf>, Vec<Arc<RenderImage>>) = entries.into_iter().unzip();
        self.paths = paths;
        let mut changed = false;
        if self.key != key {
            self.key = key.to_path_buf();
            self.angle = 0.;
            self.velocity = 0.;
            changed = true;
        }
        let same = self.items.len() == items.len()
            && self
                .items
                .iter()
                .zip(&items)
                .all(|(a, b)| Arc::ptr_eq(a, b));
        if !same {
            self.items = items;
            changed = true;
        }
        changed
    }

    fn on_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else { return };
        let half = f32::from(bounds.size.width) * 0.5;
        if half <= 0. {
            return;
        }
        let center = f32::from(bounds.origin.x) + half;
        self.pointer = Some(((f32::from(event.position.x) - center) / half).clamp(-1., 1.));
        cx.notify();
    }

    /// Advance the motion by the time since the last frame; whether another frame is needed.
    fn step(&mut self) -> bool {
        let now = Instant::now();
        // The first frame after idle has no previous time: assume one 60 Hz tick so the ring
        // starts moving instead of parking with dt = 0.
        let dt = self
            .last_frame
            .map_or(1. / 60., |t| now.duration_since(t).as_secs_f32())
            .clamp(1. / 240., 0.05);
        self.last_frame = Some(now);
        let n = self.items.len();
        if n < 2 {
            self.angle = 0.;
            self.velocity = 0.;
            return false;
        }
        let step = std::f32::consts::TAU / n as f32;
        let ease = 1. - (-7. * dt).exp();
        if let Some(x) = self.pointer {
            let t = ((x.abs() - DEAD_ZONE).max(0.) / (1. - DEAD_ZONE)).powf(1.6);
            let target = x.signum() * t * MAX_SPEED;
            self.velocity += (target - self.velocity) * ease;
        } else {
            // Spring to the nearest card so one always rests in front.
            let snap = (self.angle / step).round() * step;
            let target = ((snap - self.angle) * 7.).clamp(-MAX_SPEED, MAX_SPEED);
            self.velocity += (target - self.velocity) * ease;
            if (snap - self.angle).abs() < 0.002 && self.velocity.abs() < 0.02 {
                self.angle = snap;
                self.velocity = 0.;
                self.last_frame = None;
                return false;
            }
        }
        self.angle += self.velocity * dt;
        if self.pointer.is_some() && self.velocity.abs() < 0.005 {
            // Parked in the dead zone: nothing to animate until the pointer moves.
            self.last_frame = None;
            return false;
        }
        true
    }
}

impl Render for DirRing {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.step() {
            window.request_animation_frame();
        }
        let this = cx.entity();
        let n = self.items.len();
        let step = if n == 0 {
            0.
        } else {
            std::f32::consts::TAU / n as f32
        };
        // (depth, card), painted back to front. Each picture keeps its own aspect ratio.
        let mut cards: Vec<(f32, gpui::Stateful<gpui::Div>)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, texture)| {
                let theta = step.mul_add(i as f32, self.angle);
                let depth = theta.cos();
                if depth < FRONT_CUTOFF {
                    return None;
                }
                let near = f32::midpoint(depth, 1.);
                // Fade in over the first stretch of the front arc.
                let edge_fade = ((depth - FRONT_CUTOFF) / 0.45).clamp(0., 1.);
                let scale = (1. - BACK_SCALE).mul_add(near, BACK_SCALE);
                let natural = texture.size(0);
                let (nw, nh) = (
                    natural.width.0.max(1) as f32,
                    natural.height.0.max(1) as f32,
                );
                let z = self.zoom;
                let fit = (BOX_W * z / nw).min(BOX_H * z / nh) * scale;
                let (w, h) = ((nw * fit).round().max(1.), (nh * fit).round().max(1.));
                let x = theta.sin().mul_add(RADIUS_X * z, WIDTH * z * 0.5) - w * 0.5;
                let y = (depth - 0.3).mul_add(DEPTH_DROP * z, (HEIGHT - DEPTH_DROP * 0.7) * z * 0.5) - h * 0.5;
                let path = self.paths.get(i).cloned().unwrap_or_default();
                let owner = self.owner.clone();
                let card = div()
                    .id(("ring-card", i))
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(w))
                    .h(px(h))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .shadow_md()
                    .opacity((0.7 * near).mul_add(near, 0.3) * edge_fade)
                    .on_click({
                        let (path, owner) = (path.clone(), owner.clone());
                        move |_, _, cx| {
                            if let Some(owner) = owner.as_ref().and_then(WeakEntity::upgrade) {
                                owner.update(cx, |e, cx| e.ring_click(path.clone(), cx));
                            }
                        }
                    })
                    .on_mouse_down(MouseButton::Right, move |e: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        if let Some(owner) = owner.as_ref().and_then(WeakEntity::upgrade) {
                            owner.update(cx, |ex, cx| {
                                ex.open_item_menu_at(path.clone(), false, e.position, window, cx);
                            });
                        }
                    })
                    .child(
                        img(Arc::clone(texture))
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    );
                Some((depth, card))
            })
            .collect();
        cards.sort_by(|a, b| a.0.total_cmp(&b.0));

        div()
            .id("dir-ring")
            .relative()
            .flex_none()
            .w(px(WIDTH * self.zoom))
            .h(px(HEIGHT * self.zoom))
            .cursor(CursorStyle::PointingHand)
            .child(
                canvas(
                    move |bounds, _, cx| {
                        this.update(cx, |ring: &mut Self, _| ring.bounds = Some(bounds));
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .children(cards.into_iter().map(|(_, card)| card))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.on_move(event, cx);
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered {
                    this.pointer = None;
                    cx.notify();
                }
            }))
    }
}

/// The ring entity: made once, kept afterwards.
pub(crate) fn ensure(slot: &mut Option<Entity<DirRing>>, cx: &mut gpui::App) -> Entity<DirRing> {
    slot.get_or_insert_with(|| cx.new(|_| DirRing::new()))
        .clone()
}
