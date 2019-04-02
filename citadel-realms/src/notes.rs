use cursive::view::ViewWrapper;
use cursive::event::{EventResult, Event};
use cursive::traits::{View,Identifiable,Boxable,Finder};
use crate::dialogs::DialogButtonAdapter;
use cursive::{Cursive, Vec2};
use cursive::views::{LinearLayout, TextArea, TextView, DummyView, Dialog, ViewBox};
use std::rc::Rc;

pub struct NotesDialog {
    inner: ViewBox,
    callback: Rc<Fn(&mut Cursive, &str)>,
}

impl NotesDialog {
    pub fn open<F>(s: &mut Cursive, item: &str, content: impl Into<String>, ok_callback: F)
        where F: Fn(&mut Cursive, &str) + 'static
    {
        s.add_layer(NotesDialog::new(item, content, ok_callback).with_id("notes-dialog"));
    }

    fn new<F>(item: &str, content: impl Into<String>, ok_callback: F) -> Self
        where F: Fn(&mut Cursive, &str) + 'static
    {
        let edit = TextArea::new()
            .content(content)
            .with_id("notes-text")
            .min_size((60,8));

        let message = format!("Enter some notes to associate with {}", item);

        let content = LinearLayout::vertical()
            .child(TextView::new(message))
            .child(DummyView.fixed_height(2))
            .child(edit);
        let dialog = Dialog::around(content)
            .title("Edit notes")
            .dismiss_button("Cancel")
            .button("Save", Self::on_ok)
            .with_id("edit-notes-inner");

        NotesDialog { inner: ViewBox::boxed(dialog), callback: Rc::new(ok_callback) }
    }

    fn set_cursor(&mut self) {
        self.call_on_id("notes-text", |v: &mut TextArea| {
            let cursor = v.get_content().len();
            v.set_cursor(cursor);
        }).expect("call_on_id(notes-text)")
    }

    fn get_text(&mut self) -> String {
        self.call_on_id("notes-text", |v: &mut TextArea| v.get_content().to_string())
            .expect("call_on_id(notes-text)")
    }

    fn on_ok(s: &mut Cursive) {
        let (cb,notes) = s.call_on_id("notes-dialog", |v: &mut NotesDialog| (v.callback.clone(), v.get_text())).expect("call_on_id(notes-dialog)");
        (cb)(s, &notes);
        s.pop_layer();
    }

}

impl ViewWrapper for NotesDialog {
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

    fn wrap_on_event(&mut self, event: Event) -> EventResult {
        self.handle_event("cs", event)
    }

    fn wrap_layout(&mut self, size: Vec2) {
        self.inner.layout(size);
        self.set_cursor();
    }
}

impl DialogButtonAdapter for NotesDialog {
    fn inner_id(&self) -> &'static str {
        "edit-notes-inner"
    }
}
