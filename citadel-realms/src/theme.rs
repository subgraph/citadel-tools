use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;

use cursive::{
    Cursive, Printer, Vec2,
    event::{Event, EventResult},
    utils::markup::StyledString,
    theme::{Color, Theme, BorderStyle, ColorStyle, ColorType},
    traits::{View,Boxable,Identifiable},
    view::ViewWrapper,
    views::{LinearLayout, TextView, DummyView, PaddedView, Panel, ViewBox},
};

use libcitadel::terminal::{TerminalPalette, Base16Scheme};

use crate::tree::{TreeView, Placement};

#[derive(Clone)]
pub struct ThemeHandler {
    saved_palette: TerminalPalette,
    theme: Theme,
}

impl ThemeHandler {

    fn set_palette_color(theme: &mut Theme, name: &str, rgb: (u16, u16, u16)) {
        theme.palette.set_color(name, Color::Rgb(rgb.0 as u8, rgb.1 as u8, rgb.2 as u8))
    }

    pub fn generate_base16_theme(base16: &Base16Scheme) -> Theme {
        let mut theme = Theme::default();
        theme.shadow = false;
        theme.borders = BorderStyle::Outset;
        let mapping = [
            (0x0, "background"),
            (0x1, "shadow"),
            (0x0, "view"),
            (0x5, "primary"),
            (0xC, "secondary"),
            (0x3, "tertiary"),
            (0x8, "title_primary"),
            (0xA, "title_secondary"),
            (0x2, "highlight"),
            (0x3, "highlight_inactive"),
        ];
        for pair in &mapping {
            Self::set_palette_color(&mut theme, pair.1, base16.color(pair.0).rgb());
        }
        theme
    }

    const SCHEME_CONF_PATH: &'static str = "/storage/citadel-state/realms-base16.conf";
    const DEFAULT_SCHEME: &'static str = "default-dark";

    pub fn save_base16_theme(base16: &Base16Scheme) {
        if let Err(e) = fs::write(Self::SCHEME_CONF_PATH, base16.slug()) {
            warn!("Error writing color scheme file ({}): {}", Self::SCHEME_CONF_PATH, e);
        }
    }

    pub fn load_base16_scheme() -> Option<Base16Scheme> {
        let path = Path::new(Self::SCHEME_CONF_PATH);
        if path.exists() {
            fs::read_to_string(path).ok().and_then(|ref s| Base16Scheme::by_name(s).cloned())
        } else {
            None
        }
    }

    pub fn load_base16_theme() -> Theme {
        let path = Path::new(Self::SCHEME_CONF_PATH);
        let mut scheme = Base16Scheme::by_name(Self::DEFAULT_SCHEME).unwrap();
        if path.exists() {
            if let Ok(scheme_name) = fs::read_to_string(path) {
                if let Some(sch) = Base16Scheme::by_name(&scheme_name) {
                    scheme = sch;
                }
            }
        }
        Self::generate_base16_theme(scheme)
    }
}

pub struct  ThemeChooser {
    inner: ViewBox,
}

impl ThemeChooser {

    pub fn open(s: &mut Cursive) {
        let initial = ThemeHandler::load_base16_scheme();
        let chooser = Self::new(initial, |s,v| {
            ThemeHandler::save_base16_theme(v);
            let theme = ThemeHandler::generate_base16_theme(v);
            s.set_theme(theme);
        });
        s.add_layer(chooser.with_id("theme-chooser"));
    }

    pub fn new<F>(initial: Option<Base16Scheme>, cb: F) -> Self
        where F: 'static + Fn(&mut Cursive, &Base16Scheme)
    {
        let select = Self::create_tree_view(initial.clone(), cb);
        let content = Self::create_content(initial, select);
        let inner = ViewBox::boxed(content);
        ThemeChooser { inner }
    }

    fn create_content<V: View>(initial: Option<Base16Scheme>, select: V) -> impl View {
        let left = LinearLayout::vertical()
            .child(TextView::new(StyledString::styled("Press Enter to change theme.\n 'q' or Esc to close panel", ColorStyle::tertiary())))
            .child(DummyView)
            .child(PaddedView::new((0,0,1,1),select));


        let mut preview = ThemePreview::new();
        if let Some(ref scheme) = initial {
            preview.set_scheme(scheme.clone());
        }

        let right = Panel::new(PaddedView::new((1,1,0,0), preview.with_id("theme-preview")));//.title("Preview");

        let layout = LinearLayout::horizontal()
            .child(left)//PaddedView::new((0,0,0,2),left))
            .child(DummyView.fixed_width(1))
            .child(right);

        let padded = PaddedView::new((1,1,1,1), layout);
        Panel::new(padded)
            .title("Choose a theme")
    }

    fn create_tree_view<F>(initial: Option<Base16Scheme>, cb: F) -> impl View
        where F: 'static + Fn(&mut Cursive, &Base16Scheme)
    {
        let mut tree = TreeView::new()
            .on_select(Self::on_tree_select)
            .on_collapse(Self::on_tree_collapse)
            .on_submit(move |s,idx| {
                let item = Self::call_on_tree(s, |v| v.borrow_item(idx).cloned());
                if let Some(TreeItem::ColorScheme(ref scheme)) = item {
                    (cb)(s, scheme);
                }
            });

        Self::populate_tree(initial, &mut tree);
        tree.with_id("theme-tree")
    }

    fn populate_tree(initial: Option<Base16Scheme>, tree: &mut TreeView<TreeItem>) {
        let schemes = Base16Scheme::all_schemes();
        let mut category_rows = HashMap::new();
        let mut last_row = 0;
        for scheme in &schemes {
            last_row = Self::add_scheme_to_tree(initial.as_ref(), tree, last_row, scheme, &mut category_rows);
        }
    }

    fn add_scheme_to_tree(initial: Option<&Base16Scheme>, tree: &mut TreeView<TreeItem>, last_row: usize, scheme: &Base16Scheme, category_rows: &mut HashMap<&str,usize>) -> usize {
        let item = TreeItem::scheme(scheme);
        let mut last_row = last_row;
        let is_initial = initial.map(|s| s.slug() == scheme.slug()).unwrap_or(false);

        if let Some(category) = scheme.category() {
            let is_initial_category = initial.map(|sc| sc.category() == scheme.category()).unwrap_or(false);
            let category_row = Self::get_category_row(!is_initial_category, tree, &mut last_row, category, category_rows);
            if let Some(new_row) = tree.insert_item(item, Placement::LastChild, category_row) {
                if is_initial {
                    tree.set_selected_row(new_row);
                    tree.scroll_to(category_row);
                }
            }
        } else {
            last_row = tree.insert_item(item, Placement::After, last_row)
                .expect("newly added colorscheme row is not visible");

            if is_initial {
                tree.set_selected_row(last_row);
            }
        }
        last_row
    }

    fn get_category_row<'a>(collapsed: bool, tree: &mut TreeView<TreeItem>, last_row: &mut usize, category: &'a str, category_rows: &mut HashMap<&'a str, usize>) -> usize {
        let row = category_rows.entry(category).or_insert_with(|| {
            let new_row = tree.insert_item(TreeItem::category(category, collapsed), Placement::After, *last_row)
                .expect("newly added category row is not visible");
            if collapsed {
                tree.collapse_item(new_row);
            }
            *last_row = new_row;
            new_row
        });
        *row
    }


    fn on_tree_select(s: &mut Cursive, idx: usize) {
        let selected = Self::call_on_tree(s, |v| v.borrow_item(idx).cloned());

        if let Some(item) = selected {
            if let TreeItem::ColorScheme(scheme) = item {
                s.call_on_id("theme-preview", |v: &mut ThemePreview| v.set_scheme(scheme));
            }
        }
    }

    fn on_tree_collapse(s: &mut Cursive, row: usize, is_collapsed: bool, _: usize) {
        Self::call_on_tree(s, |v| {
            if let Some(item) = v.borrow_item_mut(row) {
                if let TreeItem::Category(ref _name, ref mut collapsed) = *item {
                    *collapsed = is_collapsed;
                }
            }
        });
    }

    fn call_on_tree<F,R>(s: &mut Cursive, cb:F) -> R
        where F: FnOnce(&mut TreeView<TreeItem>) -> R

    {
        s.call_on_id("theme-tree", cb)
            .expect("call_on_id(theme-tree)")
    }

    fn toggle_expand_item(&self) -> EventResult {
        EventResult::with_cb(|s| {
            Self::call_on_tree(s, |v| {
                if let Some(row) = v.row() {
                    Self::toggle_item_collapsed(v, row);
                }
            })
        })
    }

    fn toggle_item_collapsed(v: &mut TreeView<TreeItem>, row: usize) {
        if let Some(item) = v.borrow_item_mut(row) {
            if let TreeItem::Category(_name, collapsed) = item {
                let was_collapsed = *collapsed;
                *collapsed = !was_collapsed;
                v.set_collapsed(row, !was_collapsed);
            }
        }
    }
}

impl ViewWrapper for ThemeChooser {
    type V = View;
    fn with_view<F: FnOnce(&Self::V) -> R, R>(&self, f: F) -> Option<R> { Some(f(&*self.inner)) }
    fn with_view_mut<F: FnOnce(&mut Self::V) -> R, R>(&mut self, f: F) -> Option<R> { Some(f(&mut *self.inner)) }

    fn wrap_on_event(&mut self, event: Event) -> EventResult {
        match event {
            Event::Char(' ') => self.toggle_expand_item(),
            Event::Char('o') => self.toggle_expand_item(),
            event => self.inner.on_event(event)
        }
    }
}

struct PreviewHelper<'a,'b> {
    printer: Printer<'a,'b>,
    scheme: Base16Scheme,
    offset: Vec2,
}

impl <'a,'b> PreviewHelper<'a,'b> {
    fn new(printer: Printer<'a,'b>, scheme: Base16Scheme) -> Self {
        PreviewHelper {
            printer, scheme, offset: Vec2::zero()
        }

    }
    fn color(&self, idx: usize) -> ColorType {
        let (r,g,b) = self.scheme.terminal_palette_color(idx).rgb();
        ColorType::Color(Color::Rgb(r as u8, g as u8, b as u8))
    }

    fn color_fg(&self) -> ColorType {
        let (r,g,b) = self.scheme.terminal_foreground().rgb();
        ColorType::Color(Color::Rgb(r as u8, g as u8, b as u8))
    }
    fn color_bg(&self) -> ColorType {
        let (r,g,b) = self.scheme.terminal_background().rgb();
        ColorType::Color(Color::Rgb(r as u8, g as u8, b as u8))
    }

    fn draw(mut self, color: ColorType, text: &str) -> Self {
        let style = ColorStyle::new(color,self.color_bg());

        self.printer.with_color(style, |printer| {
            printer.print(self.offset, text);
        });
        self.offset.x += text.len();

        self
    }

    fn vtype(self, text: &str) -> Self {
        let color = self.color(3);
        self.draw(color, text)
    }

    fn konst(self, text: &str) -> Self {
        let color = self.color(1);
        self.draw(color, text)
    }


    fn func(self, text: &str) -> Self {
        let color = self.color(4);
        self.draw(color, text)
    }

    fn string(self, text: &str) -> Self {
        let color = self.color(2);
        self.draw(color, text)
    }

    fn keyword(self, text: &str) -> Self {
        let color = self.color(5);
        self.draw(color, text)
    }

    fn comment(self, text: &str) -> Self {
        let color = self.color(8);
        self.draw(color, text)
    }

    fn text(self, text: &str) -> Self {
        let color = self.color_fg();
        self.draw(color, text)
    }

    fn nl(mut self) -> Self {
        self.offset.x = 0;
        self.offset.y += 1;
        self
    }
}

struct ThemePreview {
    last_size: Vec2,
    scheme: Option<Base16Scheme>,
}

impl ThemePreview {
    fn new() -> ThemePreview {
        ThemePreview { scheme: None, last_size: Vec2::zero() }
    }

    fn set_scheme(&mut self, scheme: Base16Scheme) {
        self.scheme = Some(scheme);
    }

    fn color(&self, idx: usize) -> ColorType {
        let (r,g,b) = self.scheme.as_ref().unwrap().color(idx).rgb();
        ColorType::Color(Color::Rgb(r as u8, g as u8, b as u8))
    }

    fn terminal_color(&self, idx: usize) -> ColorType {
        let (r,g,b) = self.scheme.as_ref().unwrap().terminal_palette_color(idx).rgb();
        ColorType::Color(Color::Rgb(r as u8, g as u8, b as u8))
    }

    fn terminal_style(&self, fg: usize, bg: usize) -> ColorStyle {
        ColorStyle::new(self.terminal_color(fg), self.terminal_color(bg))
    }

    fn style(&self, fg: usize, bg: usize) -> ColorStyle {
        ColorStyle::new(self.color(fg), self.color(bg))
    }

    fn draw_background(&self, printer: &Printer) {
        let color = self.terminal_style(5, 0);
        printer.with_color(color, |printer| {
            for i in 0..self.last_size.y {
                printer.print_hline((0,i), self.last_size.x, " ");
            }
        });
    }

    fn draw_colorbar(&self, printer: &Printer) {
        let text_color = self.style(3, 0);
        for i in 0..16 {
            let color = self.style(3, i);
            printer.with_color(text_color, |printer| {
                printer.print((i*3, 0), &format!(" {:X} ", i));
            });
            printer.with_color(color, |printer| {
                printer.print((i*3, 1), "   ");
            });
        }
        for i in 8..16 {
            let color = self.terminal_style(5, i);
            let x = (i - 8) * 6;
            printer.with_color(color, |printer| {
                printer.print_hline((x, 2), 6, " ");
            });

        }
    }

    fn draw_text(&self, printer: &Printer) {
        let scheme = self.scheme.as_ref().unwrap().clone();
        let name = scheme.name().to_owned();
        let printer = printer.offset((4, 5));
        PreviewHelper::new(printer, scheme)
            .comment("/**").nl()
            .comment(" *  An example of how this color scheme").nl()
            .comment(" *  might look in a text editor with syntax").nl()
            .comment(" *  highlighting.").nl()
            .comment(" */").nl()
            .nl()
            .func("#include ").string("<stdio.h>").nl()
            .func("#include ").string("<stdlib.h>").nl()
            .nl()
            .vtype("static char").text(" theme[] = ").string(&format!("\"{}\"", name)).text(";").nl()
            .nl()
            .vtype("int").text(" main(").vtype("int").text(" argc, ").vtype("char").text(" **argv) {").nl()
            .text("    printf(").string("\"Hello, ").keyword("%s").text("!").keyword("\\n").string("\"").text(", theme);").nl()
            .text("    exit(").konst("0").text(");").nl()
            .text("}")
            .nl()
            .nl();
    }

}


impl View for ThemePreview {
    fn draw(&self, printer: &Printer) {
        if self.scheme.is_some() {
            self.draw_background(&printer);
            self.draw_colorbar(&printer.offset((2, 1)));
            self.draw_text(&printer);
        }
    }

    fn layout(&mut self, size: Vec2) {
        self.last_size = size;
    }

    fn required_size(&mut self, _constraint: Vec2) -> Vec2 {
        Vec2::new(52, 30)
    }
}

#[derive(Clone,Debug)]
enum TreeItem {
    Category(String, bool),
    ColorScheme(Base16Scheme),
}

impl fmt::Display for TreeItem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            TreeItem::Category(ref s, _) => s.as_str(),
            TreeItem::ColorScheme(ref scheme) => scheme.name(),
        })
    }
}

impl TreeItem {
    fn category(name: &str, collapsed: bool) -> Self {
        TreeItem::Category(name.to_string(), collapsed)
    }
    fn scheme(scheme: &Base16Scheme) -> Self {
        TreeItem::ColorScheme(scheme.clone())
    }
}
