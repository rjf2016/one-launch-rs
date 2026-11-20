use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Point, ScrollHandle, Window,
};

type Shift = Box<dyn Fn(&ScrollHandle) -> Point<Pixels>>;

// Moves its child by an amount read from a scroll handle while the frame is laid out, after the scrolled element before it has clamped the offset: read while rendering, the offset is the one the wheel left, which can lie outside the range the list then allows.
pub struct Follow {
    child: AnyElement,
    handle: ScrollHandle,
    shift: Shift,
}

pub fn follow(
    handle: &ScrollHandle,
    shift: impl Fn(&ScrollHandle) -> Point<Pixels> + 'static,
    child: impl IntoElement,
) -> Follow {
    Follow {
        child: child.into_any_element(),
        handle: handle.clone(),
        shift: Box::new(shift),
    }
}

impl IntoElement for Follow {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Follow {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let shift = (self.shift)(&self.handle);
        window.with_element_offset(shift, |window| {
            self.child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
