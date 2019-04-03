use cursive::views::{Dialog, TextView, OnEventView, PaddedView, DialogFocus, EditView, ListView, LinearLayout, DummyView };
use cursive::traits::{View, Finder,Boxable,Identifiable,Scrollable};
use cursive::event::{EventResult, Event, EventTrigger};
use cursive::event::Key;
use cursive::Cursive;
use std::rc::Rc;
use cursive::view::ViewWrapper;
use cursive::direction::Direction;
use cursive::theme::ColorStyle;


pub fn confirm_dialog<F>(title: &str, message: &str, cb: F) -> impl View
    where F: 'static + Fn(&mut Cursive)
{
    let content = PaddedView::new((2,2,2,1), TextView::new(message));
    let dialog = Dialog::around(content)
        .title(title)
        .button("Yes", move |s| {
            s.pop_layer();
            (cb)(s);
        })
        .dismiss_button("No");

    OnEventView::new(dialog)
        .on_event_inner('y', |d: &mut Dialog, _| {
            Some(d.on_event(Event::Key(Key::Left)))
        })
        .on_event_inner('n', move |d: &mut Dialog, _| {
            Some(d.on_event(Event::Key(Key::Right)))
        })
        // Eat these global events
        .on_event_inner('?', |_,_| {
            Some(EventResult::Consumed(None))
        })
        .on_event_inner('T', |_,_| {
            Some(EventResult::Consumed(None))
        })

}


// Set focus on dialog button at index `idx` by injecting events
// into the Dialog view.
//
// If the dialog content is currently in focus send Tab
// character to move focus to first button.
//
// Then inject Right/Left key events as needed to select
// the correct index.
pub fn select_dialog_button_index(dialog: &mut Dialog, idx: usize) {
    let mut current = match dialog.focus() {
        DialogFocus::Content => {
            dialog.on_event(Event::Key(Key::Tab));
            0
        },
        DialogFocus::Button(n) => {
            n
        },
    };
    while current < idx {
        dialog.on_event(Event::Key(Key::Right));
        current += 1;
    }
    while current > idx {
        dialog.on_event(Event::Key(Key::Left));
        current -= 1;
    }
}

pub fn keyboard_navigation_adapter(dialog: Dialog, keys: &'static str) -> OnEventView<Dialog> {
    // a trigger that matches any character in 'keys'
    let trigger = EventTrigger::from_fn(move |ev| match ev {
            Event::Char(c) => keys.contains(|ch: char| ch == *c),
            _ => false,
    });
    OnEventView::new(dialog)

        // The button navigation is a hack that depends on Dialog internal behavior
        .on_event_inner(trigger, move |d: &mut Dialog,ev| {
            if let Event::Char(c) = ev {
                if let Some(idx) = keys.find(|ch: char| ch == *c) {
                    select_dialog_button_index(d, idx);
                    return Some(EventResult::Consumed(None))
                }
            }
            None
        })

        .on_event_inner(Key::Enter, |v,_| Some(v.on_event(Event::Key(Key::Down))))

        // 'q' to close dialog, but first see if some component of the dialog
        // (such as a text field) wants this event
        .on_pre_event_inner('q', |v,e| {
            let result = match v.on_event(e.clone()) {
                EventResult::Consumed(cb) => EventResult::Consumed(cb),
                EventResult::Ignored => EventResult::with_cb(|s| {s.pop_layer();}),
            };
            Some(result)
        })

        // Eat these global events
        .on_event_inner('?', |_,_| {
            Some(EventResult::Consumed(None))
        })
        .on_event_inner('T', |_,_| {
            Some(EventResult::Consumed(None))
        })

        .on_event(Key::Esc, |s| {
            s.pop_layer();
        })
}

pub struct FieldDialogBuilder {
    layout: FieldLayout,
    id: &'static str,
    title: Option<&'static str>,
    height: Option<usize>,
}

#[allow(dead_code)]
impl FieldDialogBuilder {

    const DEFAULT_ID: &'static str = "field-dialog";

    pub fn new(labels: &[&str], message: &str) -> Self {
        FieldDialogBuilder {
            layout: FieldLayout::new(labels, message),
            id: Self::DEFAULT_ID,
            title: None,
            height: None,
        }
    }


    pub fn add_edit_view(&mut self, id: &str, width: usize) {
        self.layout.add_edit_view(id, width);
    }

    pub fn edit_view(mut self, id: &str, width: usize) -> Self {
        self.add_edit_view(id, width);
        self
    }

    pub fn add_field<V: View>(&mut self, view: V) {
        self.layout.add_field(view);
    }

    pub fn field<V: View>(mut self, view: V) -> Self {
        self.add_field(view);
        self
    }

    pub fn set_id(&mut self, id: &'static str) {
        self.id = id;
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.set_id(id);
        self
    }

    pub fn set_title(&mut self, title: &'static str) {
        self.title = Some(title);
    }

    pub fn title(mut self, title: &'static str) -> Self {
        self.set_title(title);
        self
    }

    pub fn set_width(&mut self, width: usize) {
        self.layout.set_width(width);
    }

    pub fn width(mut self, width: usize) -> Self {
        self.set_width(width);
        self
    }

    pub fn set_height(&mut self, height: usize) {
        self.height = Some(height);;
    }

    pub fn height(mut self, height: usize) -> Self {
        self.set_height(height);
        self
    }

    pub fn build<F: 'static + Fn(&mut Cursive)>(self, ok_cb: F) -> impl View {
        let content = self.layout.build()
            .padded(2,2,1,2);

        let mut dialog = Dialog::around(content)
            .dismiss_button("Cancel")
            .button("Ok", ok_cb);

        if let Some(title) = self.title {
            dialog.set_title(title);
        }

        let height = self.height.unwrap_or(12);

        dialog.with_id(self.id)
            .min_height(height)
    }
}

pub struct FieldLayout {
    list: ListView,
    message: String,
    labels: Vec<String>,
    index: usize,
    width: usize,
}

#[allow(dead_code)]
impl FieldLayout {
    const DEFAULT_WIDTH: usize = 48;

    pub fn new(labels: &[&str], message: &str) -> FieldLayout {
        let maxlen = labels.iter().fold(0, |max, &s| {
            if s.len() > max { s.len() } else { max }
        });

        let pad_label = |s: &&str| {
            if s.is_empty() {
                " ".repeat(maxlen + 2)
            } else {
                " ".repeat(maxlen - s.len()) + s + ": "
            }
        };

        let labels = labels
            .iter()
            .map(pad_label)
            .collect();

        FieldLayout {
            list: ListView::new(),
            message: message.to_string(),
            labels,
            index: 0,
            width: Self::DEFAULT_WIDTH,
        }
    }


    pub fn add_edit_view(&mut self, id: &str, width: usize) {
        self.add_field(EditView::new()
            .style(ColorStyle::tertiary())
            .filler(" ")
            .with_id(id)
            .fixed_width(width))
    }

    pub fn edit_view(mut self, id: &str, width: usize) -> Self {
        self.add_edit_view(id, width);
        self
    }

    pub fn add_field<V: View>(&mut self, view: V) {
        let field = LinearLayout::horizontal()
            .child(view)
            .child(DummyView);


        let idx = self.index;
        self.index += 1;

        if idx > 0 {
            self.list.add_delimiter();
        }
        self.list.add_child(self.labels[idx].as_str(), field);
    }

    pub fn field<V: View>(mut self, view: V) -> Self {
        self.add_field(view);
        self
    }

    pub fn set_width(&mut self, width: usize) {
        self.width = width;
    }

    pub fn width(mut self, width: usize) -> Self {
        self.set_width(width);
        self
    }

    pub fn build(self) -> impl View {
        LinearLayout::vertical()
            .child(TextView::new(self.message).fixed_width(self.width))
            .child(DummyView.fixed_height(2))
            .child(self.list)
            .child(DummyView)
            .scrollable()
    }
}


pub trait DialogButtonAdapter: Finder+ViewWrapper {

    fn inner_id(&self) -> &'static str;

    fn call_on_dialog<F,R>(&mut self, cb: F) -> R
        where F: FnOnce(&mut Dialog) -> R
    {
        let id = self.inner_id();
        self.call_on_id(id, cb)
            .unwrap_or_else(|| panic!("failed call_on_id({})", id))
    }

    fn button_enabled(&mut self, button: usize) -> bool {
        self.call_on_dialog(|d| {
            d.buttons_mut()
                .nth(button)
                .map(|b| b.is_enabled())
                .unwrap_or(false)
        })
    }

    fn set_button_enabled(&mut self, button: usize, enabled: bool) {
        self.call_on_dialog(|d| {
            if let Some(b) = d.buttons_mut().nth(button) {
                b.set_enabled(enabled);
            }
        })
    }

    fn select_button(&mut self, button: usize) -> EventResult {
        if self.button_enabled(button) {
            self.call_on_dialog(|d| select_dialog_button_index(d, button))
        }
        EventResult::Consumed(None)
    }

    fn navigate_to_button(&mut self, idx: usize) -> EventResult {
        if !self.button_enabled(idx) {
            return EventResult::Ignored;
        }
        self.call_on_dialog(|d| {
            d.take_focus(Direction::down());
            let mut current = d.buttons_len() - 1;
            while current > idx {
                d.on_event(Event::Key(Key::Left));
                current -= 1;
            }
        });
        EventResult::Consumed(None)
    }

    fn handle_char_event(&mut self, button_order: &str, ch: char) -> EventResult {
        if let Some(EventResult::Consumed(cb)) = self.with_view_mut(|v| v.on_event(Event::Char(ch))) {
            EventResult::Consumed(cb)
        } else if ch == 'T' || ch == '?' {
            EventResult::Consumed(None)
        } else if let Some(idx) = button_order.find(|c| c == ch) {
            self.navigate_to_button(idx)
        } else {
            EventResult::Ignored
        }
    }

    fn handle_event(&mut self, button_order: &str, event: Event) -> EventResult {
        match event {
            Event::Char(ch) => self.handle_char_event(button_order, ch),
            event => self.with_view_mut(|v| v.on_event(event)).unwrap()
        }
    }
}

pub trait Padable: View + Sized {
    fn padded(self, left: usize, right: usize, top: usize, bottom: usize) -> PaddedView<Self> {
        PaddedView::new((left,right,top,bottom), self)
    }
}
impl <T: View> Padable for T {}

pub trait Validatable: View+Finder+Sized {
    fn validator<F: 'static + Fn(&str) -> ValidatorResult>(mut self, id: &str, cb: F) -> Self {
        TextValidator::set_validator(&mut self, id, cb);
        self
    }
}

impl <T: View+Finder> Validatable for T {}


pub enum ValidatorResult {
    Allow(Box<dyn Fn(&mut Cursive)>),
    Deny(Box<dyn Fn(&mut Cursive)>),
}

impl ValidatorResult {
    pub fn create<F>(ok: bool, f: F) -> Self
        where F: 'static + Fn(&mut Cursive) {
        if ok {
            Self::allow_with(f)
        } else {
            Self::deny_with(f)
        }
    }

    pub fn allow_with<F>(f: F) -> Self
    where F: 'static + Fn(&mut Cursive)
    {
        ValidatorResult::Allow(Box::new(f))
    }

    pub fn deny_with<F>(f: F) -> Self
        where F: 'static + Fn(&mut Cursive)
    {
        ValidatorResult::Deny(Box::new(f))
    }

    fn process(self, siv: &mut Cursive) {
        match self {
            ValidatorResult::Allow(cb) | ValidatorResult::Deny(cb) => (cb)(siv),
        }
    }

    fn deny_edit(&self) -> bool {
        match self {
            ValidatorResult::Allow(_) => false,
            ValidatorResult::Deny(_) => true,
        }
    }

}

#[derive(Clone)]
pub struct TextValidator {
    id: String,
    is_valid: Rc<Box<Fn(&str) -> ValidatorResult>>,
}

impl TextValidator {

    pub fn set_validator<V: Finder,F: 'static + Fn(&str)->ValidatorResult>(view: &mut V, id: &str, cb: F) {
        let validator = TextValidator{ id: id.to_string(), is_valid: Rc::new(Box::new(cb)) };
        view.call_on_id(id, |v: &mut EditView| {
            v.set_on_edit(move |s,content,cursor| {
                let v = validator.clone();
                v.on_edit(s, content, cursor);
            });
        });
    }

    fn on_edit(&self, siv: &mut Cursive, content: &str, cursor: usize) {
        let result = (self.is_valid)(content);
        if result.deny_edit() {
            self.deny_edit(siv, cursor);
        }
        result.process(siv);
    }

    fn deny_edit(&self, siv: &mut Cursive, cursor: usize) {
        if cursor > 0 {
            let callback = self.call_on_edit(siv, |v| {
                v.set_cursor(cursor - 1);
                v.remove(1)
            });
            (callback)(siv);
        }
    }

    fn call_on_edit<F,R>(&self, siv: &mut Cursive, f: F) -> R
        where F: FnOnce(&mut EditView) -> R {

        siv.call_on_id(&self.id, f)
            .unwrap_or_else(|| panic!("call_on_id({})", self.id))
    }

}

