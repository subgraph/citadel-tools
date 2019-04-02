use cursive::views::{TextContent, OnEventView};
use libcitadel::{Result, LogLevel, Logger, LogOutput, DefaultLogOutput};
use cursive::traits::{Boxable,Identifiable};
use cursive::views::TextView;
use cursive::views::HideableView;
use cursive::view::ScrollStrategy;
use cursive::view::ViewWrapper;
use cursive::views::ScrollView;
use cursive::views::Panel;
use cursive::view::{View,Finder};
use cursive::views::ViewBox;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use cursive::Cursive;
use crate::ui::GlobalState;


pub struct LogView {
    inner: ViewBox,
    visible: bool,
}

impl LogView {
    pub fn create(content: TextContent) -> impl View {
        Self::new(content).with_id("log").max_height(8)
    }

    pub fn open_popup(s: &mut Cursive) {
        let global = s.user_data::<GlobalState>()
            .expect("cannot retrieve GlobalState");
        let content = global.log_output().text_content();
        let view = Self::new(content).full_screen();
        let view = OnEventView::new(view)
            .on_pre_event('L', |s| { s.pop_layer(); });
        s.add_fullscreen_layer(view);
    }

    fn new(content: TextContent) -> Self {
        let panel = Self::create_panel(content);
        let hideable = HideableView::new(panel).with_id("log-hide");

        LogView { inner: ViewBox::boxed(hideable), visible: true }
    }

    fn create_panel(content: TextContent) -> impl View {
        let textview = TextView::new_with_content(content);
        let scroll = ScrollView::new(textview)
            .scroll_strategy(ScrollStrategy::StickToBottom)
            .with_id("log-scroll");

        ViewBox::boxed(Panel::new(scroll).title("Log"))
    }

    pub fn toggle_hidden(&mut self) {
        self.visible = !self.visible;
        let state = self.visible;
        self.inner.call_on_id("log-hide", |log: &mut HideableView<ViewBox>| log.set_visible(state));
    }
}

impl ViewWrapper for LogView {
    type V = View;

    fn with_view<F, R>(&self, f: F) -> Option<R>
        where F: FnOnce(&Self::V) -> R
    {
        Some(f(&*self.inner))
    }

    fn with_view_mut<F, R>(&mut self, f: F) -> Option<R>
        where F: FnOnce(&mut Self::V) -> R
    {
        Some(f(&mut *self.inner))
    }
}

#[derive(Clone)]
pub struct TextContentLogOutput{
    default_enabled: Arc<AtomicBool>,
    content: TextContent,
    default: DefaultLogOutput,
}

impl TextContentLogOutput {
    pub fn new() -> Self {
        let content = TextContent::new("");
        let default_enabled = Arc::new(AtomicBool::new(false));
        let default = DefaultLogOutput::new();
        TextContentLogOutput { default_enabled, content, default }
    }

    pub fn set_as_log_output(&self) {
        Logger::set_log_output(Box::new(self.clone()));
    }

    pub fn text_content(&self) -> TextContent {
        self.content.clone()
    }

    pub fn set_default_enabled(&self, v: bool) {
        self.default_enabled.store(v, Ordering::SeqCst);
    }

    fn default_enabled(&self) -> bool {
        self.default_enabled.load(Ordering::SeqCst)
    }

}

impl LogOutput for TextContentLogOutput {
    fn log_output(&mut self, level: LogLevel, line: &str) -> Result<()> {
        if self.default_enabled() {
            self.default.log_output(level, &line)?;
        }
        let line = Logger::format_logline(level, line);
        self.content.append(line);
        Ok(())
    }
}
