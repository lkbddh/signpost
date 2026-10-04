//! Widgets laid out and driven with the software renderer: no window, no compositor.
//!
//! Laying out text and icons reads fonts and icon themes from the user's directories, so each test runs
//! again in a child process (this test binary) whose home and XDG directories are a scratch directory
//! (the system's data and config directories stay) and which has no display.

use std::cell::Cell;
use std::process::Command;

use cosmic::iced::advanced::clipboard;
use cosmic::iced::advanced::graphics::Image;
use cosmic::iced::advanced::graphics::text::Text;
use cosmic::iced::advanced::renderer::{Headless, Style};
use cosmic::iced::advanced::widget::operation::{Focusable, Outcome, Scrollable};
use cosmic::iced::advanced::widget::{Id, Operation};
use cosmic::iced::event::{PlatformSpecific, Status, wayland};
use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
use cosmic::iced::{Color, Event, Font, Pixels, Point, Rectangle, Size, Vector, mouse, window};
use cosmic::widget::svg;
use cosmic::{Element, Renderer, Theme};
use futures_util::FutureExt;

/// Runs `test` (its path as the harness lists it) again with its home in a scratch directory, and says
/// whether this is that run. The child is told so by the variable `marker`, and finds `<home>/bin` first on its
/// `PATH`, for the stand-ins of host programs it writes there.
pub fn in_scratch_home(marker: &str, test: &str) -> bool {
    if std::env::var_os(marker).is_some() {
        return true;
    }
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let home = scratch.path();
    let path = std::env::join_paths(std::iter::once(home.join("bin")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("a PATH");
    let output = Command::new(std::env::current_exe().expect("this test binary"))
        .args(["--exact", test])
        .env(marker, home)
        .env("PATH", path)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
        .env("XDG_CONFIG_DIRS", "/etc/xdg")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .output()
        .expect("the test binary runs");
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{report}");
    assert!(report.contains("1 passed"), "no test {test} ran:\n{report}");
    false
}

pub fn renderer() -> Renderer {
    <Renderer as Headless>::new(Font::DEFAULT, Pixels(16.0), Some("tiny-skia"))
        .now_or_never()
        .flatten()
        .expect("the software renderer")
}

/// What the surface shows behind a view, as a compositor would: a flat mid-gray.
pub const BACKDROP: Color = Color::from_rgb(0.5, 0.5, 0.5);
/// How far a drawn channel may sit from the one the arithmetic gives, for rounding.
const ROUNDING: f32 = 2.0 / 255.0;

/// `top` laid over `under`, as the software renderer blends them.
pub fn over(top: Color, under: Color) -> Color {
    let blend = |over: f32, under: f32| over * top.a + under * (1.0 - top.a);
    Color::from_rgb(
        blend(top.r, under.r),
        blend(top.g, under.g),
        blend(top.b, under.b),
    )
}

/// The largest difference between `a` and `b` in any color channel.
pub fn channel_gap(a: Color, b: Color) -> f32 {
    [a.r - b.r, a.g - b.g, a.b - b.b]
        .map(f32::abs)
        .into_iter()
        .fold(0.0, f32::max)
}

/// Whether two colors are the same but for the rounding of drawing.
pub fn is_rounding_apart(a: Color, b: Color) -> bool {
    [a.r - b.r, a.g - b.g, a.b - b.b]
        .iter()
        .all(|gap| gap.abs() <= ROUNDING)
}

/// Whether `inner` lies inside `outer`, to a thousandth of a pixel: laying out sums lengths, which rounds.
pub fn lies_inside(inner: Rectangle, outer: Rectangle) -> bool {
    const SLACK: f32 = 1e-3;
    inner.x >= outer.x - SLACK
        && inner.y >= outer.y - SLACK
        && inner.x + inner.width <= outer.x + outer.width + SLACK
        && inner.y + inner.height <= outer.y + outer.height + SLACK
}

/// A string as the renderer drew it: where its text widget put it, the same on the surface, and the part of
/// the surface the renderer let it show on.
struct Paragraph {
    text: String,
    local: Rectangle,
    on_screen: Rectangle,
    clip: Rectangle,
}

/// How much of a string, in square pixels, may be clipped without counting: rounding at its edges.
const CLIPPED_AREA: f32 = 1.0;

/// What a view drew on the surface, four bytes a pixel.
#[derive(PartialEq, Eq)]
pub struct Drawn {
    bytes: Vec<u8>,
    width: usize,
}

impl Drawn {
    pub fn at(&self, point: Point) -> Color {
        let pixel = Rectangle::new(point, Size::new(1.0, 1.0))
            .snap()
            .expect("a pixel");
        let first = 4 * (pixel.y as usize * self.width + pixel.x as usize);
        let [r, g, b, a] = self.bytes[first..first + 4] else {
            panic!("a pixel is four bytes");
        };
        Color::from_rgba8(r, g, b, f32::from(a) / 255.0)
    }
}

/// A scrollable as it reported itself.
pub struct Scrolled {
    id: Option<Id>,
    pub bounds: Rectangle,
    /// Where its content was laid out, at its full size and from where it starts.
    pub content: Rectangle,
    /// How far its content is moved up and left from where it starts.
    pub offset: Vector,
}

/// What the widgets report about themselves when asked.
#[derive(Default)]
pub struct Probe {
    /// The bounds of every focus stop, in the order the widgets report them.
    pub stops: Vec<Rectangle>,
    /// For each of [`Probe::stops`], the bounds of the widgets that hold it, outermost first.
    pub stops_enclosing: Vec<Vec<Rectangle>>,
    /// The bounds of the focus stops that have the native focus.
    pub focused: Vec<Rectangle>,
    containers: Vec<(Id, Rectangle)>,
    /// Every scrollable; one drawn under another comes first.
    pub scrollables: Vec<Scrolled>,
    /// Every text widget's string and the bounds it was laid out in, in the order the widgets report them.
    pub texts: Vec<(String, Rectangle)>,
    /// For each of [`Probe::texts`], the bounds of the widgets that hold it, outermost first, a scrollable's
    /// content among them.
    pub enclosing: Vec<Vec<Rectangle>>,
    /// The widgets the next widget's children are inside.
    holders: Vec<Rectangle>,
    /// The bounds the widget being visited announced, which hold its children once it visits them.
    announced: Option<Rectangle>,
}

impl Probe {
    /// The bounds of the container named `id`.
    pub fn container_named(&self, id: &Id) -> Option<Rectangle> {
        let (_, bounds) = self.containers.iter().find(|(named, _)| named == id)?;
        Some(*bounds)
    }

    /// The scrollable named `id`.
    pub fn scrollable_named(&self, id: &Id) -> Option<&Scrolled> {
        self.scrollables
            .iter()
            .find(|scrolled| scrolled.id.as_ref() == Some(id))
    }
}

impl Operation for Probe {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let holder = self.announced.take();
        self.holders.extend(holder);
        operate(self);
        if holder.is_some() {
            self.holders.pop();
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if let Some(id) = id {
            self.containers.push((id.clone(), bounds));
        }
        self.announced = Some(bounds);
    }

    fn focusable(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        self.stops.push(bounds);
        self.stops_enclosing.push(self.holders.clone());
        if state.is_focused() {
            self.focused.push(bounds);
        }
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, text: &str) {
        self.texts.push((text.to_owned(), bounds));
        self.enclosing.push(self.holders.clone());
        self.announced = None;
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        self.scrollables.push(Scrolled {
            id: id.cloned(),
            bounds,
            content,
            offset: translation,
        });
    }
}

/// The surface the runtime gives a popup that asked for no size, until its widgets say how big they are
/// (`vendor/iced_winit/src/platform_specific/wayland/event_loop/state.rs`, `get_popup`).
pub const NEW_POPUP: Size = Size::new(1.0, 1.0);

/// A clipboard that holds nothing, and keeps the size a widget asked its window to take.
#[derive(Default)]
struct WindowRequests {
    size: Cell<Option<Size>>,
}

impl clipboard::Clipboard for WindowRequests {
    fn read(&self, _kind: clipboard::Kind) -> Option<String> {
        None
    }

    fn write(&mut self, _kind: clipboard::Kind, _contents: String) {}

    fn request_logical_window_size(&self, width: f32, height: f32) {
        self.size.set(Some(Size::new(width, height)));
    }
}

/// What the shell keeps between frames: the renderer and the widgets' own state.
pub struct Frames {
    surface: Size,
    pub renderer: Renderer,
    cache: Cache,
}

impl Frames {
    pub fn new(surface: Size) -> Self {
        Self {
            surface,
            renderer: renderer(),
            cache: Cache::new(),
        }
    }

    /// The size of the surface these frames are drawn on.
    pub fn surface(&self) -> Size {
        self.surface
    }

    /// The interface of the next frame. The widgets' state moves into it; [`Frames::probe`] and
    /// [`Frames::send`] take it back out.
    pub fn build<'a, Message>(
        &mut self,
        view: impl Into<Element<'a, Message>>,
    ) -> UserInterface<'a, Message, Theme, Renderer> {
        UserInterface::build(
            view,
            self.surface,
            std::mem::take(&mut self.cache),
            &mut self.renderer,
        )
    }

    fn keep<Message>(&mut self, interface: UserInterface<'_, Message, Theme, Renderer>) {
        self.cache = interface.into_cache();
    }

    /// What `view`'s widgets report about themselves.
    pub fn probe<'a, Message>(&mut self, view: impl Into<Element<'a, Message>>) -> Probe {
        let mut interface = self.build(view);
        let mut probe = Probe::default();
        interface.operate(&self.renderer, &mut probe);
        self.keep(interface);
        probe
    }

    /// The size `view`'s widgets ask their window to take when the runtime tells them to (as it does a new
    /// popup, in the frame it is created), if they ask. The window is the one these frames were made for.
    pub fn requested_size<'a, Message>(
        &mut self,
        view: impl Into<Element<'a, Message>>,
    ) -> Option<Size> {
        let mut interface = self.build(view);
        let requests = &mut WindowRequests::default();
        interface.update(
            &[Event::PlatformSpecific(PlatformSpecific::Wayland(
                wayland::Event::RequestResize,
            ))],
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            requests,
            &mut Vec::<Message>::new(),
        );
        self.keep(interface);
        requests.size.get()
    }

    /// Gives `view` the `events` with the pointer at `at`, and returns what its widgets sent.
    pub fn send<'a, Message>(
        &mut self,
        view: impl Into<Element<'a, Message>>,
        events: &[Event],
        at: Point,
    ) -> Vec<Message> {
        self.send_with_statuses(view, events, at).0
    }

    /// [`Frames::send`], and whether the widgets took each event, which decides whether the runtime's
    /// subscriptions get to act on it.
    pub fn send_with_statuses<'a, Message>(
        &mut self,
        view: impl Into<Element<'a, Message>>,
        events: &[Event],
        at: Point,
    ) -> (Vec<Message>, Vec<Status>) {
        let mut interface = self.build(view);
        let mut messages = Vec::new();
        let (_, statuses) = interface.update(
            events,
            mouse::Cursor::Available(at),
            &mut self.renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        self.keep(interface);
        (messages, statuses)
    }

    /// `view` drawn under `theme` over [`BACKDROP`].
    pub fn drawn<'a, Message>(
        &mut self,
        view: impl Into<Element<'a, Message>>,
        theme: &Theme,
    ) -> Drawn {
        self.drawn_over(view, theme, BACKDROP)
    }

    /// `view` drawn under `theme` over `backdrop`.
    pub fn drawn_over<'a, Message>(
        &mut self,
        view: impl Into<Element<'a, Message>>,
        theme: &Theme,
        backdrop: Color,
    ) -> Drawn {
        let mut interface = self.build(view);
        // A popup is laid out by an update, so a frame that gets no events still needs one to show it.
        interface.update(
            &[],
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut Vec::<Message>::new(),
        );
        interface.draw(
            &mut self.renderer,
            theme,
            &Style::default(),
            mouse::Cursor::Unavailable,
        );
        let surface = Rectangle::new(Point::ORIGIN, self.surface)
            .snap()
            .expect("a surface");
        let size = Size::new(surface.width, surface.height);
        Drawn {
            bytes: self.renderer.screenshot(size, 1.0, backdrop),
            width: surface.width as usize,
        }
    }

    /// The strings `view` draws under `theme`, in drawing order, those of an open tooltip among them.
    pub fn drawn_texts<'a, Message>(
        &mut self,
        view: impl Into<Element<'a, Message>>,
        theme: &Theme,
    ) -> Vec<String> {
        let mut interface = self.build(view);
        // An overlay is laid out by an update, so a frame that gets no events still needs one to show it.
        interface.update(
            &[],
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut Vec::<Message>::new(),
        );
        interface.draw(
            &mut self.renderer,
            theme,
            &Style::default(),
            mouse::Cursor::Unavailable,
        );
        self.keep(interface);
        let Renderer::Secondary(software) = &mut self.renderer else {
            panic!("the software renderer");
        };
        software
            .layers()
            .iter()
            .flat_map(|layer| &layer.text)
            .flat_map(|item| item.as_slice().iter())
            .flat_map(|text| match text {
                Text::Paragraph { paragraph, .. } => paragraph
                    .upgrade()
                    .map(|paragraph| {
                        paragraph
                            .buffer()
                            .lines
                            .iter()
                            .map(|line| line.text().to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
                Text::Cached { content, .. } => vec![content.clone()],
                _ => Vec::new(),
            })
            .collect()
    }

    /// The strings `view` draws outside the bounds it laid them out in or outside a widget that holds them, so
    /// cut off or spilling onto what is beside them: each with where it was drawn and the bounds it left. A
    /// string scrolled out of view is not cut off, and one that is not drawn is not judged.
    pub fn overflowing_texts<'a, Message>(
        &mut self,
        view: impl Into<Element<'a, Message>>,
    ) -> Vec<(String, Rectangle, Rectangle)> {
        let mut interface = self.build(view);
        // An overlay is laid out by an update, so a frame that gets no events still needs one to show it.
        interface.update(
            &[],
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut Vec::<Message>::new(),
        );
        let mut probe = Probe::default();
        interface.operate(&self.renderer, &mut probe);
        interface.draw(
            &mut self.renderer,
            &Theme::dark(),
            &Style::default(),
            mouse::Cursor::Unavailable,
        );
        self.keep(interface);
        let Renderer::Secondary(software) = &mut self.renderer else {
            panic!("the software renderer");
        };
        let mut drawn: Vec<Paragraph> = software
            .layers()
            .iter()
            .flat_map(|layer| {
                layer
                    .text
                    .iter()
                    .flat_map(|item| item.as_slice().iter())
                    .map(move |text| (layer.bounds, text))
            })
            .filter_map(|(layer, text)| match text {
                Text::Paragraph {
                    paragraph,
                    position,
                    clip_bounds,
                    transformation,
                    ..
                } => {
                    let lines: Vec<String> = paragraph
                        .upgrade()?
                        .buffer()
                        .lines
                        .iter()
                        .map(|line| line.text().to_owned())
                        .collect();
                    let local = Rectangle::new(*position, paragraph.min_bounds);
                    Some(Paragraph {
                        text: lines.join("\n"),
                        local,
                        on_screen: local * *transformation,
                        clip: layer
                            .intersection(&(*clip_bounds * *transformation))
                            .unwrap_or_default(),
                    })
                }
                _ => None,
            })
            .collect();
        let surface = Rectangle::new(Point::ORIGIN, self.surface);
        probe
            .texts
            .into_iter()
            .zip(probe.enclosing)
            .filter_map(|((text, laid_out), holders)| {
                let at = drawn.iter().position(|drawn| drawn.text == text)?;
                let paragraph = drawn.remove(at);
                // What a scrollable moves out of its viewport is not cut off, so a text it scrolls is held only
                // by its content, which announces itself, and what lies inside that. A text and what holds it
                // are laid out in the same frame, which scrolling moves as a whole.
                let scroll = holders.iter().enumerate().rev().find_map(|(at, holder)| {
                    let scrolled = probe.scrollables.iter().find(|s| s.content == *holder)?;
                    Some((at, scrolled.bounds))
                });
                let (inside, viewport) = scroll.unwrap_or((0, surface));
                if let Some(left) = std::iter::once(laid_out)
                    .chain(holders[inside..].iter().copied())
                    .find(|holder| !lies_inside(paragraph.local, *holder))
                {
                    return Some((text, paragraph.local, left));
                }
                // The renderer's own clipping may cut it only where the viewport does. A scrollable inside
                // another is taken to have its viewport where the window would show it.
                let shown = |within: Rectangle| {
                    paragraph
                        .on_screen
                        .intersection(&within)
                        .map_or(0.0, |part| part.width * part.height)
                };
                (shown(paragraph.clip) + CLIPPED_AREA < shown(viewport)).then_some((
                    text,
                    paragraph.on_screen,
                    paragraph.clip,
                ))
            })
            .collect()
    }

    /// The handle `view` draws its one copy of `file`'s SVG with: `file` itself, unless the view built its own.
    pub fn drawn_copy_of<'a, Message>(
        &mut self,
        file: &svg::Handle,
        view: impl Into<Element<'a, Message>>,
    ) -> svg::Handle {
        self.draw_one_copy_of(file, view).handle
    }

    /// Where, and how much of it, `view` draws its one copy of `file`'s SVG.
    pub fn drawn_svg_of<'a, Message>(
        &mut self,
        file: &svg::Handle,
        view: impl Into<Element<'a, Message>>,
    ) -> DrawnSvg {
        self.draw_one_copy_of(file, view)
    }

    fn draw_one_copy_of<'a, Message>(
        &mut self,
        file: &svg::Handle,
        view: impl Into<Element<'a, Message>>,
    ) -> DrawnSvg {
        let mut interface = self.build(view);
        interface.draw(
            &mut self.renderer,
            &Theme::dark(),
            &Style::default(),
            mouse::Cursor::Unavailable,
        );
        self.keep(interface);
        let Renderer::Secondary(software) = &mut self.renderer else {
            panic!("the software renderer");
        };
        let mut copies = software.layers().iter().flat_map(|layer| {
            layer.images.iter().filter_map(|image| match image {
                Image::Vector {
                    svg,
                    bounds,
                    clip_bounds,
                } if svg.handle.id() == file.id() => Some(DrawnSvg {
                    handle: svg.handle.clone(),
                    bounds: *bounds,
                    visible: layer.bounds.intersection(clip_bounds).unwrap_or_default(),
                }),
                _ => None,
            })
        });
        let copy = copies.next().expect("the view draws the file");
        assert!(copies.next().is_none(), "the view draws the file twice");
        copy
    }
}

/// Applies `operation`, and what it chains on, to the widgets of every window in turn, as the runtime
/// applies the widget operations of a task: each step of a chain visits all the windows before the next,
/// and is told which window it visits.
pub fn operate_windows<Message>(
    windows: Vec<(window::Id, &mut Frames, Element<'_, Message>)>,
    operation: Box<dyn Operation>,
) {
    let mut interfaces: Vec<_> = windows
        .into_iter()
        .map(|(id, frames, view)| (id, frames.build(view), frames))
        .collect();
    let mut next = Some(operation);
    while let Some(mut operation) = next.take() {
        for (id, interface, frames) in &mut interfaces {
            operation.set_window_id(*id);
            interface.operate(&frames.renderer, operation.as_mut());
        }
        if let Outcome::Chain(chained) = operation.finish() {
            next = Some(chained);
        }
    }
    for (_, interface, frames) in interfaces {
        frames.keep(interface);
    }
}

/// An SVG as a view drew it.
pub struct DrawnSvg {
    pub handle: svg::Handle,
    /// Where the whole image is drawn, past the edges of what shows of it.
    pub bounds: Rectangle,
    /// What shows of it: inside the clips of the image and of the layer it is drawn on.
    pub visible: Rectangle,
}

#[cfg(test)]
mod tests {
    use cosmic::iced::Length;
    use cosmic::iced::advanced::layout::{Limits, Node};
    use cosmic::iced::advanced::widget::Tree;
    use cosmic::iced::advanced::{Layout, Renderer as _, Widget, renderer};
    use cosmic::iced::mouse::ScrollDelta;
    use cosmic::iced::widget::{column, scrollable};
    use cosmic::widget::text;

    use super::*;

    const SCRATCH_HOME: &str = "SIGNPOST_HEADLESS_SCRATCH_HOME";

    fn in_scratch(name: &str) -> bool {
        in_scratch_home(
            SCRATCH_HOME,
            &format!("test_support::headless::tests::{name}"),
        )
    }

    /// A box `height` tall that lays its child out at the child's full size and clips it, so a taller child is
    /// cut off: a holder cutting off a text that fits its own bounds. `announces` is whether it tells operations
    /// its bounds, as containers do; one that does not is caught only by its clipping.
    struct Spill<'a> {
        child: Element<'a, ()>,
        height: f32,
        announces: bool,
    }

    fn spill(lines: &'static str, announces: bool) -> Element<'static, ()> {
        Element::new(Spill {
            child: text::body(lines).into(),
            height: 20.0,
            announces,
        })
    }

    impl Widget<(), Theme, Renderer> for Spill<'_> {
        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.child)]
        }

        fn diff(&mut self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_mut(&mut self.child));
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Shrink, Length::Fixed(self.height))
        }

        fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, _limits: &Limits) -> Node {
            let child = self.child.as_widget_mut().layout(
                &mut tree.children[0],
                renderer,
                &Limits::new(Size::ZERO, Size::new(f32::INFINITY, f32::INFINITY)),
            );
            Node::with_children(Size::new(child.size().width, self.height), vec![child])
        }

        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn Operation,
        ) {
            if self.announces {
                operation.container(None, layout.bounds());
            }
            operation.traverse(&mut |operation| {
                self.child.as_widget_mut().operate(
                    &mut tree.children[0],
                    layout.children().next().expect("the child"),
                    renderer,
                    operation,
                );
            });
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            renderer.with_layer(layout.bounds(), |renderer| {
                self.child.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    layout.children().next().expect("the child"),
                    cursor,
                    viewport,
                );
            });
        }
    }

    fn cut(frames: &mut Frames, view: impl Into<Element<'static, ()>>) -> Vec<String> {
        frames
            .overflowing_texts(view)
            .into_iter()
            .map(|(text, ..)| text)
            .collect()
    }

    /// A list three lines long in a viewport a line and a half tall, with `first` as its first line.
    fn list(first: Element<'static, ()>) -> Element<'static, ()> {
        let lines: cosmic::iced::widget::Column<'_, (), Theme, Renderer> =
            column![first, text::body("next"), text::body("last")];
        scrollable(lines).height(Length::Fixed(30.0)).into()
    }

    #[test]
    fn a_text_its_holder_or_a_clip_cuts_off_overflows_and_one_scrolled_out_of_view_does_not() {
        if !in_scratch(
            "a_text_its_holder_or_a_clip_cuts_off_overflows_and_one_scrolled_out_of_view_does_not",
        ) {
            return;
        }
        let mut frames = Frames::new(Size::new(200.0, 200.0));
        assert_eq!(
            cut(&mut frames, spill("first\nsecond", true)),
            ["first\nsecond"],
            "two lines in a box one line tall"
        );
        assert_eq!(
            cut(&mut frames, spill("first\nsecond", false)),
            ["first\nsecond"],
            "a box that only clips cuts them off too"
        );

        assert_eq!(
            cut(&mut frames, list(text::body("first").into())),
            Vec::<String>::new()
        );
        let over = Point::new(5.0, 5.0);
        let wheel = [
            Event::Mouse(mouse::Event::CursorMoved { position: over }),
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: ScrollDelta::Pixels { x: 0.0, y: -12.0 },
            }),
        ];
        frames.send(list(text::body("first").into()), &wheel, over);
        let offset = frames.probe(list(text::body("first").into())).scrollables[0].offset;
        assert!(offset.y > 0.0, "the list scrolled: {offset:?}");
        assert_eq!(
            cut(&mut frames, list(text::body("first").into())),
            Vec::<String>::new(),
            "scrolled part-way, every line is only out of view"
        );
        assert_eq!(
            cut(&mut frames, list(spill("first\nsecond", true))),
            ["first\nsecond"],
            "scrolled, a box one line tall still cuts its two lines off"
        );
    }
}
