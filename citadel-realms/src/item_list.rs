use std::ops::Deref;
use cursive::{Vec2, Printer, Cursive};
use cursive::event::{EventResult, Event, Key};
use cursive::views::{TextContent, Panel, TextView, LinearLayout};
use cursive::traits::{View,Identifiable,Boxable};
use cursive::direction::Direction;
use cursive::utils::markup::StyledString;
use cursive::theme::{Style, PaletteColor, Effect, ColorStyle};
use std::rc::Rc;
use std::cell::RefCell;


pub struct Selector<T> {
    items: Vec<T>,
    current: usize,
}

impl <T> Deref for Selector<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.items[self.current]
    }
}

impl <T: Clone> Selector<T>{

    fn from_vec(items: Vec<T>) -> Self {
        Selector {
            items,
            current: 0,
        }
    }

    fn set(&mut self, idx: usize) {
        if idx > self.max_idx() {
            self.current = self.max_idx();
        } else {
            self.current = idx;
        }
    }

    fn get(&self, idx: usize) -> &T {
        &self.items[idx]
    }

    fn find<P>(&self, pred: P) -> Option<usize>
        where P: Fn(&T) -> bool
    {
        self.items.iter()
            .enumerate()
            .find(|(_,elem)| pred(elem))
            .map(|(idx,_)| idx)
    }

    fn len(&self) -> usize {
        self.items.len()
    }

    pub fn load_items(&mut self, items: Vec<T>) {
        self.items = items;
        self.current = 0;
    }

    pub fn load_and_keep_selection<P>(&mut self, items: Vec<T>, pred: P)
        where P: Fn(&T,&T) -> bool
    {
        let old_item = self.clone();
        self.load_items(items);
        self.current = self.find(|it| pred(&old_item, it)).unwrap_or(0);
    }

    fn up(&mut self, n: usize) {
        self.set(self.current.saturating_sub(n));
    }

    fn max_idx(&self) -> usize {
        if self.items.is_empty() {
            0
        } else {
            self.items.len() - 1
        }
    }

    fn down(&mut self, n: usize) {
        self.set(self.current + n);
    }

    fn current_item(&self) -> Option<&T> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.get(self.current))
        }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

pub trait ItemListContent<T: Clone> {
    fn items(&self) -> Vec<T>;

    fn reload(&self, selector: &mut Selector<T>) {
        selector.load_items(self.items());
    }

    fn draw_item(&self, width: usize, printer: &Printer, item: &T, selected: bool);

    fn update_info(&mut self, item: &T, state: Rc<ItemRenderState>);

    fn on_event(&mut self, item: Option<&T>, event: Event) -> EventResult;
}

pub struct ItemList<T: Clone + 'static> {
    selector: Selector<T>,
    last_size: Vec2,
    info_state: Rc<ItemRenderState>,
    content: Box<ItemListContent<T>>,
}

impl <T: Clone + 'static> ItemList<T> {

    pub fn call_reload(id: &str, s: &mut Cursive) {
        s.call_on_id(id, |v: &mut ItemList<T>| v.reload_items());
    }

    pub fn call_update_info(id: &str, s: &mut Cursive) {
        Self::call(id, s, |v| v.update_info());
    }

    pub fn call<F,R>(id: &str, s: &mut Cursive, f: F) -> R
        where F: FnOnce(&mut ItemList<T>) -> R
    {
        s.call_on_id(id, |v: &mut ItemList<T>| f(v))
            .unwrap_or_else(|| panic!("ItemList::call_on_id({})", id))
    }

    pub fn create<C>(id: &'static str, title: &str, content: C) -> impl View
        where C: ItemListContent<T> + 'static
    {

        let list = ItemList::new(content);
        let text = TextView::new_with_content(list.info_content());

        let left = Panel::new(list.with_id(id))
            .title(title)
            .min_width(30);

        let right = Panel::new(text).full_width();

        LinearLayout::horizontal()
            .child(left)
            .child(right)
            .full_height()
    }


    pub fn new<C>(content: C) -> Self
        where C: ItemListContent<T> + 'static
    {
        let selector = Selector::from_vec(content.items());
        let last_size = Vec2::zero();
        let info_state = ItemRenderState::create();
        let content = Box::new(content);
        let mut list = ItemList { selector, info_state, last_size, content };
        list.update_info();
        list
    }

    pub fn info_content(&self) -> TextContent {
        self.info_state.content()
    }

    pub fn reload_items(&mut self) {
        self.content.reload(&mut self.selector);
        self.update_info();
    }

    pub fn selected_item(&self) -> &T {
        &self.selector
    }

    fn selection_up(&mut self) -> EventResult {
        self.selector.up(1);
        self.update_info();
        EventResult::Consumed(None)
    }

    fn selection_down(&mut self) -> EventResult {
        self.selector.down(1);
        self.update_info();
        EventResult::Consumed(None)
    }

    pub fn update_info(&mut self) {
        self.info_state.clear();
        if !self.selector.is_empty() {
            self.content.update_info(&self.selector, self.info_state.clone());
        }
    }

    fn draw_item_idx(&self, printer: &Printer, idx: usize) {
        let item = self.selector.get(idx);
        let selected = idx == self.selector.current;
        printer.offset((0,idx)).with_selection(selected, |printer| {
            self.content.draw_item(self.last_size.x, printer, item, selected);
        });
    }
}

impl <T: 'static + Clone> View for ItemList<T> {

    fn draw(&self, printer: &Printer) {
        for i in 0..self.selector.len() {
            self.draw_item_idx(printer, i);
        }
    }

    fn layout(&mut self, size: Vec2) {
        self.last_size = size;
    }

    fn on_event(&mut self, event: Event) -> EventResult {
        match event {
            Event::Key(Key::Up) | Event::Char('k') => self.selection_up(),
            Event::Key(Key::Down) | Event::Char('j') => self.selection_down(),
            ev => self.content.on_event(self.selector.current_item(), ev),
        }
    }

    fn take_focus(&mut self, _source: Direction) -> bool {
        true
    }
}


pub struct ItemRenderState {
    inner: RefCell<Inner>,
}

struct Inner {
    content: TextContent,
    styles: Vec<Style>,
}

impl ItemRenderState {
    pub fn create() -> Rc<ItemRenderState> {
        let state = ItemRenderState {
            inner: RefCell::new(Inner::new())
        };
        Rc::new(state)
    }

    pub fn content(&self) -> TextContent {
        self.inner.borrow().content.clone()
    }

    pub fn clear(&self) {
        self.inner.borrow_mut().clear();
    }

    pub fn append(&self, s: StyledString)  {
        self.inner.borrow_mut().append(s);
    }

    pub fn push_style<S: Into<Style>>(&self, style: S) {
        self.inner.borrow_mut().push_style(style.into());
    }

    pub fn pop_style(&self) -> Style {
        self.inner.borrow_mut().pop_style()
    }
}

impl Inner {
    fn new() -> Self {
        Inner {
            content: TextContent::new(""),
            styles: vec![Style::none()],
        }
    }

    fn clear(&mut self) {
        self.content.set_content("");
        self.styles.clear();
        self.styles.push(Style::none());
    }

    fn push_style(&mut self, style: Style) {
        self.styles.push(style);
    }

    fn pop_style(&mut self) -> Style {
        self.styles.pop().unwrap_or_default()
    }

    fn append(&mut self, s: StyledString) {
        self.content.append(s);
    }
}

pub trait InfoRenderer: Clone {

    fn state(&self) -> Rc<ItemRenderState>;


    fn push(&self, style: Style)-> &Self {
        self.state().push_style(style);
        self
    }

    fn pop_style(&self) -> Style {
        self.state().pop_style()
    }


    fn append(&self, s: StyledString) -> &Self {
        self.state().append(s);
        self
    }

    fn pop(&self) -> &Self {
        self.state().pop_style();
        self
    }

    fn plain_style(&self) -> &Self {
        self.push(Style::none())
    }

    fn activated_style(&self) -> &Self {
        self.push(Style::from(ColorStyle::secondary())
            .combine(Effect::Bold))
    }

    fn heading_style(&self, underline: bool) -> &Self {

        let style = Style::from(PaletteColor::TitleSecondary);
        if underline {
            self.push(style.combine(Effect::Underline))
        } else {
            self.push(style)
        }
    }

    fn alert_style(&self) ->  &Self {
        self.push(Style::from(PaletteColor::TitlePrimary))
    }

    fn dim_style(&self) -> &Self {
        self.push(Style::from(ColorStyle::tertiary()))
    }

    fn dim_bold_style(&self) -> &Self {
        self.push(Style::from(ColorStyle::tertiary())
            .combine(Effect::Bold))
    }

    fn underlined(&self) -> &Self {
        self.push(Style::from(Effect::Underline))
    }

    fn print<S: Into<String>>(&self, s: S) -> &Self {
        let style = self.pop_style();
        self.append(StyledString::styled(s, style));
        self.push(style)
    }

    fn println<S: Into<String>>(&self, s: S) -> &Self {
        self.print(s);
        self.newlines(1)
    }

    fn newlines(&self, count: usize) -> &Self {
        (0..count).for_each(|_| { self.print("\n");} );
        self
    }

    fn newline(&self) -> &Self {
        self.newlines(1)
    }

    fn heading<S: Into<String>>(&self, name: S) -> &Self {
        self.heading_style(true)
            .print(name)
            .pop()
            .heading_style(false)
            .print(":")
            .pop()
    }

}
