use super::*;

/// DropdownMenu 注入的 selected 是实际开合状态，箭头不再猜测点击时序。
#[derive(IntoElement)]
pub(super) struct ProviderSelectTrigger {
    button: Button,
    open: bool,
}

impl ProviderSelectTrigger {
    pub(super) fn new(button: Button) -> Self {
        Self { button, open: false }
    }
}
impl gpui::Styled for ProviderSelectTrigger {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.button.style()
    }
}
impl gpui::InteractiveElement for ProviderSelectTrigger {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.button.interactivity()
    }
}
impl gpui_component::Selectable for ProviderSelectTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.open = selected;
        self.button = self.button.selected(selected);
        self
    }
    fn is_selected(&self) -> bool {
        self.open
    }
}
impl gpui_component::menu::DropdownMenu for ProviderSelectTrigger {}
impl gpui::RenderOnce for ProviderSelectTrigger {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.button.dropdown_caret(false).child(
            Icon::new(if self.open { IconName::ChevronUp } else { IconName::ChevronDown })
                .size(px(12.0)),
        )
    }
}
