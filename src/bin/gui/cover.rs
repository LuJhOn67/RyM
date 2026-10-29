//! Cover art with a fixed size. iced's own image widget decodes its picture while laying out
//! the page, so every cover in a long list sat decoded in RAM even when scrolled out of sight.
//! This one never measures the picture (the size is known) and only draws when it's on screen,
//! so only visible covers are decoded.

use iced::advanced::image::{self as img, FilterMethod, Handle};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Widget, layout, mouse, renderer};
use iced::{Element, Length, Radians, Rectangle, Size};

pub struct Cover {
    handle: Handle,
    /// The picture's own size in pixels.
    source: Size,
    side: f32,
    radius: f32,
    opacity: f32,
}

/// A square of `side` filled by the picture: the shorter edge fits, the rest is cut off
/// (video thumbnails are 16:9 or letterboxed).
pub fn cover(
    handle: Handle,
    width: u32,
    height: u32,
    side: f32,
    radius: f32,
    opacity: f32,
) -> Cover {
    Cover {
        handle,
        source: Size::new(width.max(1) as f32, height.max(1) as f32),
        side,
        radius,
        opacity,
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Cover
where
    Renderer: img::Renderer<Handle = Handle>,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.side), Length::Fixed(self.side))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.side, self.side))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if !bounds.intersects(viewport) || self.opacity <= 0.0 {
            return;
        }
        let scale = self.side / self.source.width.min(self.source.height);
        let (w, h) = (self.source.width * scale, self.source.height * scale);
        let drawn = Rectangle {
            x: bounds.center_x() - w / 2.0,
            y: bounds.center_y() - h / 2.0,
            width: w,
            height: h,
        };
        let image = img::Image {
            handle: self.handle.clone(),
            filter_method: FilterMethod::Linear,
            rotation: Radians(0.0),
            // Applied to the clip bounds: the visible square gets the round corners.
            border_radius: self.radius.into(),
            opacity: self.opacity,
            snap: true,
        };
        renderer.draw_image(image, drawn, bounds);
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer> From<Cover> for Element<'a, Message, Theme, Renderer>
where
    Renderer: img::Renderer<Handle = Handle> + 'a,
{
    fn from(c: Cover) -> Self {
        Element::new(c)
    }
}
