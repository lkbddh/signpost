//! The small widgets and styles the picker's view is built from.

use cosmic::cosmic_theme::{Container, palette::Srgba};
use cosmic::iced::advanced::layout::{Limits, Node};
use cosmic::iced::advanced::widget::{Operation, Tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, overlay, renderer};
use cosmic::iced::core::text::{Ellipsize, EllipsizeHeightLimit, Wrapping};
use cosmic::iced::id::Id;
use cosmic::iced::widget::text::Style as TextStyle;
use cosmic::iced::widget::{canvas, container as container_style, svg as svg_style};
use cosmic::iced::{
    Background, Border, Color, Event, Gradient, Length, Point, Radians, Rectangle, Shadow, Size,
    Vector, alignment, gradient, keyboard, mouse, touch, window,
};
use cosmic::widget::button::Catalog as _;
use cosmic::widget::{self, button, container, text};
use cosmic::{Element, theme};

use super::{TILE_HEIGHT, TILE_WIDTH, tint};
use crate::app::Message;
use crate::colors;
use crate::icons::{self, Icon};
use crate::picker::{Input, Key, map_key};
use crate::profiles::Rgb;

const WARNING_ICON_SIZE: u16 = 20;
pub(super) const BADGE_SIZE: f32 = 18.0;
pub(super) const BADGE_HALO: f32 = 3.0;
const NEUTRAL_TINT_ALPHA: f32 = 0.08;
const FOCUS_RING: f32 = 2.0;
const MUTED_ALPHA: f32 = 0.75;
const HIGHLIGHT_ALPHA: f32 = 0.2;
pub(super) const BORDER_WIDTH: f32 = 1.0;
const CHIP_RADIUS: f32 = 4.0;
const CHIP_PADDING: [f32; 2] = [0.0, 6.0];
const PILL_RADIUS: f32 = 100.0;
const PILL_PADDING: [f32; 2] = [2.0, 10.0];
const DASHES: [f32; 2] = [4.0, 4.0];

pub(super) fn muted(theme: &cosmic::Theme) -> TextStyle {
    let mut color = theme.current_container().component.on;
    color.alpha *= MUTED_ALPHA;
    TextStyle {
        color: Some(color.into()),
        ..Default::default()
    }
}

pub(super) fn warning_text(theme: &cosmic::Theme) -> TextStyle {
    TextStyle {
        color: Some(theme.cosmic().warning_text_color().into()),
        ..Default::default()
    }
}

/// `line` kept to one line that ends in an ellipsis rather than wrapping.
pub(super) fn ellipsized(
    line: widget::Text<'_, cosmic::Theme, cosmic::Renderer>,
) -> widget::Text<'_, cosmic::Theme, cosmic::Renderer> {
    line.wrapping(Wrapping::None)
        .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1)))
}

pub(super) fn one_line<'a>(label: String) -> widget::Text<'a, cosmic::Theme, cosmic::Renderer> {
    ellipsized(text::body(label))
}

pub(super) fn centered_line<'a>(
    label: String,
) -> widget::Text<'a, cosmic::Theme, cosmic::Renderer> {
    one_line(label)
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Center)
}

/// A button class that draws `rest`, and `hover` while the pointer is over it or it is pressed, ringed while it has
/// keyboard focus.
pub(super) fn button_class(
    rest: impl Fn(&cosmic::Theme) -> button::Style + Copy + 'static,
    hover: impl Fn(&cosmic::Theme) -> button::Style + Copy + 'static,
) -> theme::Button {
    theme::Button::Custom {
        active: Box::new(move |focused, theme| ringed(focused, theme, rest)),
        disabled: Box::new(rest),
        hovered: Box::new(move |focused, theme| ringed(focused, theme, hover)),
        pressed: Box::new(move |focused, theme| ringed(focused, theme, hover)),
    }
}

/// What `style` draws, with the ring libcosmic draws around a button with keyboard focus.
fn ringed(
    focused: bool,
    theme: &cosmic::Theme,
    style: impl Fn(&cosmic::Theme) -> button::Style,
) -> button::Style {
    let style = style(theme);
    if !focused {
        return style;
    }
    let native = theme.active(true, false, &theme::Button::Standard);
    button::Style {
        outline_width: native.outline_width,
        outline_color: native.outline_color,
        border_width: native.border_width,
        border_color: native.border_color,
        ..style
    }
}

/// The header's icon buttons: libcosmic's own Standard look, focus ring and all, on the lock badge's
/// surface, so the header's controls share one tone.
pub(super) fn header_button_class() -> theme::Button {
    theme::Button::Custom {
        active: Box::new(|focused, theme| button::Style {
            background: Some(Background::Color(card_surface(theme))),
            ..theme.active(focused, false, &theme::Button::Standard)
        }),
        disabled: Box::new(|theme| button::Style {
            background: Some(Background::Color(card_surface(theme))),
            ..theme.disabled(&theme::Button::Standard)
        }),
        hovered: Box::new(|focused, theme| button::Style {
            background: Some(Background::Color(hover_color(theme))),
            ..theme.hovered(focused, false, &theme::Button::Standard)
        }),
        pressed: Box::new(|focused, theme| button::Style {
            background: Some(Background::Color(hover_color(theme))),
            ..theme.pressed(focused, false, &theme::Button::Standard)
        }),
    }
}

fn lightness(color: Srgba) -> f32 {
    (color.red + color.green + color.blue) / 3.0
}

/// The Background layer as the opaque theme draws it, which the picker's parts take their colors from
/// whether or not `theme` is translucent.
fn opaque(theme: &cosmic::Theme) -> &Container {
    theme.cosmic().background(false)
}

/// `target`, the color the opaque theme draws over `under`, as it must be drawn in `theme`. Over a
/// translucent surface the same color can land on the wrong side of it (darker, in a dark theme over a
/// light backdrop), so there it is a tint of the lightest neutral (the darkest, for a part darker than
/// `under`) that takes the opaque `under` to `target`.
fn raised(theme: &cosmic::Theme, target: Srgba, under: Srgba) -> Color {
    if !theme.transparent {
        return target.into();
    }
    let cosmic = theme.cosmic();
    let gap = lightness(target) - lightness(under);
    if gap == 0.0 {
        return Color::TRANSPARENT;
    }
    let (darkest, lightest) = if cosmic.is_dark {
        (cosmic.control_0(), cosmic.control_10())
    } else {
        (cosmic.control_10(), cosmic.control_0())
    };
    let tint = if gap > 0.0 { lightest } else { darkest };
    // A tint as light as `under` makes the quotient infinite, which clamps to all of the tint or none.
    Color {
        a: (gap / (lightness(tint) - lightness(under))).clamp(0.0, 1.0),
        ..tint.into()
    }
}

/// The fill of what stands on the card: tiles, the lock badge and the queue row.
pub(super) fn card_surface(theme: &cosmic::Theme) -> Color {
    let opaque = opaque(theme);
    raised(theme, opaque.component.base, opaque.base)
}

pub(super) fn hover_color(theme: &cosmic::Theme) -> Color {
    let opaque = opaque(theme);
    raised(theme, opaque.component.hover, opaque.base)
}

pub(super) fn accent_tint(theme: &cosmic::Theme) -> Color {
    Color {
        a: HIGHLIGHT_ALPHA,
        ..theme.cosmic().accent_color().into()
    }
}

pub(super) fn rgb_of(color: Color) -> Rgb {
    let [r, g, b, _] = color.into_rgba8();
    Rgb { r, g, b }
}

/// A small bordered box: a key.
pub(super) fn chip<'a>(label: String) -> Element<'a, Message> {
    container(text::monotext(label))
        .padding(CHIP_PADDING)
        .class(theme::Container::custom(|theme| container_style::Style {
            border: Border {
                color: raised(theme, opaque(theme).divider, opaque(theme).component.base),
                width: BORDER_WIDTH,
                radius: CHIP_RADIUS.into(),
            },
            ..Default::default()
        }))
        .into()
}

/// A rounded label for a count, on a tile-colored surface.
pub(super) fn pill<'a>(label: String) -> Element<'a, Message> {
    container(text::caption(label))
        .padding(PILL_PADDING)
        .class(theme::Container::custom(|theme| container_style::Style {
            background: Some(Background::Color(raised(
                theme,
                opaque(theme).component.pressed,
                opaque(theme).component.base,
            ))),
            border: Border {
                radius: PILL_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        }))
        .into()
}

pub(super) fn warning_icon<'a>() -> Element<'a, Message> {
    icons::icon(Icon::WarningCircle, WARNING_ICON_SIZE)
        .class(theme::Svg::custom(|theme| svg_style::Style {
            color: Some(theme.cosmic().warning_color().into()),
        }))
        .into()
}

pub(super) fn tooltip_below<'a>(
    content: impl Into<Element<'a, Message>>,
    tip: String,
) -> Element<'a, Message> {
    widget::tooltip(content, text::body(tip), widget::tooltip::Position::Bottom).into()
}

/// A top-to-bottom fade from nothing into `color`.
pub(super) fn fade_in(color: Color) -> Background {
    let linear = gradient::Linear::new(Radians::PI)
        .add_stop(0.0, Color { a: 0.0, ..color })
        .add_stop(1.0, color);
    Background::Gradient(Gradient::Linear(linear))
}

/// How a tile is drawn: its profile color, whether it is lit (hovered, or its menu is open), and whether
/// it carries the keyboard focus ring.
#[derive(Clone, Copy)]
pub(super) struct TileLook {
    pub(super) color: Option<Rgb>,
    pub(super) lit: bool,
    pub(super) ring: bool,
}

impl TileLook {
    /// The tile's card around `face`: the card surface, the profile's color (else a neutral) fading in
    /// toward the foot of the tile while it is lit or focused, and the focus ring over both.
    pub(super) fn surface(self, id: Id, face: Element<'_, Message>) -> Element<'_, Message> {
        let glow = container(face)
            .width(Length::Fill)
            .height(Length::Fill)
            .class(theme::Container::custom(move |theme| self.glow(theme)));
        container(glow)
            .id(id)
            .width(Length::Fixed(TILE_WIDTH))
            .height(Length::Fixed(TILE_HEIGHT))
            .class(theme::Container::custom(card_style))
            .into()
    }

    fn glow(self, theme: &cosmic::Theme) -> container_style::Style {
        let neutral = Color {
            a: NEUTRAL_TINT_ALPHA,
            ..theme.current_container().component.on.into()
        };
        container_style::Style {
            background: (self.lit || self.ring).then(|| fade_in(tint(self.color, neutral))),
            border: Border {
                color: theme.cosmic().accent.base.into(),
                width: if self.ring { FOCUS_RING } else { 0.0 },
                radius: theme.cosmic().radius_s().into(),
            },
            ..Default::default()
        }
    }
}

fn card_style(theme: &cosmic::Theme) -> container_style::Style {
    container_style::Style {
        background: Some(Background::Color(card_surface(theme))),
        border: Border {
            radius: theme.cosmic().radius_s().into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The header's lock badge: a tile that also sets the color of its icon.
pub(super) fn badge_style(theme: &cosmic::Theme) -> container_style::Style {
    let ink = theme.current_container().component.on.into();
    container_style::Style {
        icon_color: Some(ink),
        text_color: Some(ink),
        ..card_style(theme)
    }
}

pub(super) fn row_style(theme: &cosmic::Theme, background: Option<Color>) -> button::Style {
    let mut style = button::Style::new();
    style.border_radius = theme.cosmic().radius_s().into();
    style.background = background.map(Background::Color);
    style
}

/// The picker's fill: the theme's Background layer, rounded at `radius_m`, with no edge and no shadow.
pub(super) fn shell_fill(theme: &cosmic::Theme) -> container_style::Style {
    let cosmic = theme.cosmic();
    container_style::Style {
        border: Border {
            radius: cosmic.radius_m().into(),
            ..Default::default()
        },
        ..theme::Container::background(cosmic, theme.transparent)
    }
}

/// [`shell_fill`]'s text and icon colors alone, for content over a fill that is a layer of its own.
pub(super) fn shell_ink(theme: &cosmic::Theme) -> container_style::Style {
    let fill = shell_fill(theme);
    container_style::Style {
        text_color: fill.text_color,
        icon_color: fill.icon_color,
        ..Default::default()
    }
}

/// The picker's surface: [`shell_fill`] edged by Background's divider, 1 px wide as COSMIC's dialogs are,
/// and still without a shadow.
pub(super) fn shell_style(theme: &cosmic::Theme) -> container_style::Style {
    let fill = shell_fill(theme);
    container_style::Style {
        border: Border {
            color: theme.cosmic().background(theme.transparent).divider.into(),
            width: BORDER_WIDTH,
            ..fill.border
        },
        ..fill
    }
}

/// [`shell_style`] without its fill, to draw the edge over everything the card holds.
pub(super) fn shell_edge(theme: &cosmic::Theme) -> container_style::Style {
    container_style::Style {
        background: None,
        ..shell_style(theme)
    }
}

/// A band below the card in the card's own fill, rounded where the band ends at the bottom. A lone band
/// has no edge and reads as part of the card; `edged` bands, stacked two or more, wear the card's edge
/// so each reads apart.
pub(super) fn sheet_style(theme: &cosmic::Theme, edged: bool) -> container_style::Style {
    let base = if edged {
        shell_style(theme)
    } else {
        shell_fill(theme)
    };
    let [_, _, bottom_right, bottom_left] = theme.cosmic().radius_m();
    container_style::Style {
        border: Border {
            radius: [0.0, 0.0, bottom_right, bottom_left].into(),
            ..base.border
        },
        ..base
    }
}

/// The actions menu's surface: COSMIC's opaque dialog, without the shadow that its popup has no room for.
pub(super) fn menu_style(theme: &cosmic::Theme) -> container_style::Style {
    container_style::Style {
        shadow: Shadow::default(),
        ..container_style::Catalog::style(theme, &theme::Container::Dialog(true))
    }
}

/// A column whose rows are all as wide as the widest of them. In a `Column` a row that fills its width takes
/// all the room it is offered, which has no end in a popup that sizes itself, and a row that shrinks takes only
/// its own. Rows here are laid out twice: as small as they can be, then as wide as the widest. The rows that
/// fill their height are as tall as the tallest of them, so that a scroll position can name one by its place
/// in the list; the others, a separator, keep their own height. Its size is whole pixels, which a popup
/// surface is, so that no sliver of the surface is left bare.
// A known limit: it forwards no `a11y_nodes` or drag destinations: `frost::scoped` around the menu already drops
// the first, and the menu takes no drops. Forward them when either matters.
pub(super) struct Rows<'a> {
    rows: Vec<Element<'a, Message>>,
}

impl<'a> Rows<'a> {
    pub(super) fn new(rows: Vec<Element<'a, Message>>) -> Self {
        Self { rows }
    }
}

fn fills_height(row: &Element<'_, Message>) -> bool {
    row.as_widget().size().height == Length::Fill
}

impl Widget<Message, cosmic::Theme, cosmic::Renderer> for Rows<'_> {
    fn children(&self) -> Vec<Tree> {
        self.rows.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.rows);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &cosmic::Renderer, limits: &Limits) -> Node {
        let smallest = Limits::with_compression(Size::ZERO, limits.max(), Size::new(true, true));
        let sizes: Vec<Size> = self
            .rows
            .iter_mut()
            .zip(&mut tree.children)
            .map(|(row, tree)| row.as_widget_mut().layout(tree, renderer, &smallest).size())
            .collect();
        let width = sizes
            .iter()
            .map(|size| size.width)
            .fold(0.0, f32::max)
            .ceil();
        let height = self
            .rows
            .iter()
            .zip(&sizes)
            .filter(|(row, _)| fills_height(row))
            .map(|(_, size)| size.height)
            .fold(0.0, f32::max);
        let widest = Limits::new(Size::new(width, 0.0), Size::new(width, limits.max().height));
        let tallest = Limits::new(Size::new(width, height), Size::new(width, height));
        let mut top = 0.0;
        let nodes = self
            .rows
            .iter_mut()
            .zip(&mut tree.children)
            .map(|(row, tree)| {
                let limits = if fills_height(row) { &tallest } else { &widest };
                let node = row
                    .as_widget_mut()
                    .layout(tree, renderer, limits)
                    .move_to(Point::new(0.0, top));
                top += node.size().height;
                node
            })
            .collect();
        Node::with_children(Size::new(width, top.ceil()), nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((row, tree), row_layout) in self
                .rows
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                row.as_widget_mut().operate(
                    tree,
                    row_layout.with_virtual_offset(layout.virtual_offset()),
                    renderer,
                    operation,
                );
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for ((row, tree), row_layout) in self
            .rows
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            row.as_widget_mut().update(
                tree,
                event,
                row_layout.with_virtual_offset(layout.virtual_offset()),
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        self.rows
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((row, tree), row_layout)| {
                row.as_widget().mouse_interaction(
                    tree,
                    row_layout.with_virtual_offset(layout.virtual_offset()),
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for ((row, tree), row_layout) in self.rows.iter().zip(&tree.children).zip(layout.children())
        {
            row.as_widget().draw(
                tree,
                renderer,
                theme,
                style,
                row_layout.with_virtual_offset(layout.virtual_offset()),
                cursor,
                viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, cosmic::Theme, cosmic::Renderer>> {
        overlay::from_children(
            &mut self.rows,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

/// The dashed outline of the no-apps panel.
pub(super) struct DashedOutline;

impl<Message> canvas::Program<Message, cosmic::Theme> for DashedOutline {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &cosmic::Renderer,
        theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let inset = BORDER_WIDTH / 2.0;
        let outline = canvas::Path::rounded_rectangle(
            Point::new(inset, inset),
            Size::new(bounds.width - BORDER_WIDTH, bounds.height - BORDER_WIDTH),
            theme.cosmic().radius_s().into(),
        );
        frame.stroke(
            &outline,
            canvas::Stroke {
                line_dash: canvas::LineDash {
                    segments: &DASHES,
                    offset: 0,
                },
                ..canvas::Stroke::default()
                    .with_width(BORDER_WIDTH)
                    .with_color(theme.current_container().divider.into())
            },
        );
        vec![frame.into_geometry()]
    }
}

/// Takes Tab and Shift+Tab for the open menu, which walks its rows with them. A canvas sees every event, and
/// one it captures never reaches libcosmic's keyboard navigation, which acts on the events left alone and
/// would move the native focus as well.
pub(super) struct TabCatcher {
    pub(super) id: window::Id,
}

impl canvas::Program<Message, cosmic::Theme> for TabCatcher {
    type State = ();

    fn update(
        &self,
        (): &mut (),
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Tab),
            modifiers,
            ..
        }) = event
        else {
            return None;
        };
        let tab = map_key(Key::Tab, modifiers.control(), true)?.with_shift(modifiers.shift());
        Some(canvas::Action::publish(Message::Tile(self.id, tab)).and_capture())
    }

    fn draw(
        &self,
        (): &(),
        _renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        Vec::new()
    }
}

/// Takes every mouse and touch press on the card while its menu is open, and has the menu close on it. A canvas
/// sees every event and decides what to capture, where `mouse_area` leaves a press of the right or middle
/// button to the widgets beneath it.
pub(super) struct PressShield {
    pub(super) id: window::Id,
}

impl canvas::Program<Message, cosmic::Theme> for PressShield {
    type State = ();

    fn update(
        &self,
        (): &mut (),
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let pressed = matches!(
            event,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(_))
                | canvas::Event::Touch(touch::Event::FingerPressed { .. })
        );
        pressed.then(|| {
            canvas::Action::publish(Message::Tile(self.id, Input::CloseMenu)).and_capture()
        })
    }

    fn draw(
        &self,
        (): &(),
        _renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        Vec::new()
    }
}

/// The profile color as a disk with a halo in the tile's own surface; a thin outline keeps a color that
/// matches the surface visible. A translucent tile has no halo: a second translucent layer would show as
/// a disk of its own, so the disk sits on the icon bare. What it sits on is drawn over a backdrop the
/// picker cannot see, so no contrast can be judged there and the outline is always drawn.
pub(super) fn badge_disk<'a>(color: Rgb) -> Element<'a, Message> {
    let dot = container(widget::Space::new())
        .width(Length::Fixed(BADGE_SIZE))
        .height(Length::Fixed(BADGE_SIZE))
        .class(theme::Container::custom(move |theme| {
            let outlined = theme.transparent
                || colors::needs_outline(color, rgb_of(opaque(theme).component.base.into()));
            container_style::Style {
                background: Some(Background::Color(Color::from_rgb8(
                    color.r, color.g, color.b,
                ))),
                border: Border {
                    color: theme.current_container().component.on.into(),
                    width: if outlined { BORDER_WIDTH } else { 0.0 },
                    radius: (BADGE_SIZE / 2.0).into(),
                },
                ..Default::default()
            }
        }));
    container(dot)
        .padding(BADGE_HALO)
        .class(theme::Container::custom(|theme| container_style::Style {
            background: (!theme.transparent).then(|| Background::Color(card_surface(theme))),
            border: Border {
                radius: (BADGE_SIZE / 2.0 + BADGE_HALO).into(),
                ..Default::default()
            },
            ..Default::default()
        }))
        .into()
}
