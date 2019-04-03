use crate::item_list::{ItemListContent, ItemRenderState, Selector, InfoRenderer, ItemList};
use libcitadel::{RealmFS, RealmManager, Result};
use cursive::Printer;
use std::rc::Rc;
use cursive::event::{Event, EventResult, Key};
use std::sync::Arc;
use cursive::theme::{PaletteColor, ColorStyle, Style, Effect};

mod actions;
mod fork_dialog;
pub use self::actions::RealmFSAction;

pub struct RealmFSListContent {
    manager: Arc<RealmManager>,
    show_system: bool,
}

impl RealmFSListContent {
    pub fn new(manager: Arc<RealmManager>) -> Self {
        RealmFSListContent {
            manager,
            show_system: false,
        }
    }

    fn active_color(user: bool, selected: bool, focused: bool) -> ColorStyle {
        let mut base = if selected {
            if focused {
                ColorStyle::highlight()
            } else {
                ColorStyle::highlight_inactive()
            }
        } else {
            ColorStyle::primary()
        };
        if focused {
            if user {
                base.front = PaletteColor::Secondary.into();
            } else {
                base.front = PaletteColor::Tertiary.into();
            }
        }
        base
    }

    fn draw_realmfs(&self, width: usize, printer: &Printer, realmfs: &RealmFS, selected: bool) {
        let name = format!(" {}-realmfs.img", realmfs.name());
        let w = name.len();
        let style = Style::from(Self::active_color(realmfs.is_user_realmfs(), selected, printer.focused));
        if realmfs.is_activated() {
            printer.with_style(style.combine(Effect::Bold), |p| p.print((0,0), &name));
        } else if !realmfs.is_user_realmfs() {
            printer.with_style(style, |p| p.print((0,0), &name));
        } else {
            printer.print((0, 0), &name);
        }
        if width > w {
            printer.print_hline((w, 0), width - w, " ");
        }
    }
}

impl ItemListContent<RealmFS> for RealmFSListContent {
    fn items(&self) -> Vec<RealmFS> {
        if self.show_system {
            self.manager.realmfs_list()
        } else {
            self.manager.realmfs_list()
                .into_iter()
                .filter(|r| r.is_user_realmfs())
                .collect()
        }
    }

    fn reload(&self, selector: &mut Selector<RealmFS>) {
        selector.load_and_keep_selection(self.items(), |r1,r2| r1.name() == r2.name());
    }

    fn draw_item(&self, width: usize, printer: &Printer, item: &RealmFS, selected: bool) {
        self.draw_realmfs(width, printer, item, selected);
    }

    fn update_info(&mut self, realmfs: &RealmFS, state: Rc<ItemRenderState>) {
        RealmFSInfoRender::new(state, realmfs).render();
    }

    fn on_event(&mut self, item: Option<&RealmFS>, event: Event) -> EventResult {
        let (activated,sealed,user) = item.map(|r| (r.is_activated(), r.is_sealed(), r.is_user_realmfs()))
            .unwrap_or((false, false, false));

        match event {
            Event::Key(Key::Enter) => RealmFSAction::activate_realmfs(activated),
            Event::Char('a') => RealmFSAction::autoupdate_realmfs(),
            Event::Char('A') => RealmFSAction::autoupdate_all(),
            Event::Char('d') => RealmFSAction::delete_realmfs(user),
            Event::Char('r') => RealmFSAction::resize_realmfs(),
            Event::Char('u') => RealmFSAction::update_realmfs(),
            Event::Char('n') => RealmFSAction::fork_realmfs(),
            Event::Char('s') => RealmFSAction::seal_realmfs(sealed),
            Event::Char('S') => RealmFSAction::unseal_realmfs(sealed),
            Event::Char('e') => RealmFSAction::edit_notes(),
            Event::Char('.') => {
                self.show_system = !self.show_system;
                EventResult::with_cb(|s| ItemList::<RealmFS>::call_reload("realmfs", s))
            },
            _ => EventResult::Ignored,

        }
    }
}

#[derive(Clone)]
struct RealmFSInfoRender<'a> {
    state: Rc<ItemRenderState>,
    realmfs: &'a RealmFS,
}

impl <'a> RealmFSInfoRender <'a> {
    fn new(state: Rc<ItemRenderState>, realmfs: &'a RealmFS) -> Self {
        RealmFSInfoRender { state, realmfs }
    }
    fn render(&mut self) {
        self.render_realmfs();
        self.render_image();
        self.render_activation();
        self.render_notes();
    }

    fn render_realmfs(&mut self) {
        let r = self.realmfs;

        if r.is_sealed() && r.is_user_realmfs() {
            self.heading("Sealed RealmFS");
        } else if r.is_sealed() {
            self.heading("System RealmFS");
        } else {
            self.heading("Unsealed RealmFS");
        }

        self.print("  ").render_name();

        if r.is_sealed() && !r.is_user_realmfs() {
            self.print(format!(" (channel={})", r.metainfo().channel()));
        }

        self.newlines(2);
    }

    fn render_name(&self) {
        let r = self.realmfs;
        if r.is_activated() {
            self.activated_style();
        } else if !r.is_user_realmfs() {
            self.dim_style();
        } else {
            self.plain_style();
        }
        self.print(r.name())
            .print("-realmfs-img")
            .pop();
    }

    fn render_image(&mut self) {
        fn sizes(r: &RealmFS) -> Result<(usize,usize)> {
            let free = r.free_size_blocks()?;
            let allocated = r.allocated_size_blocks()?;
            Ok((free,allocated))
        };

        let r = self.realmfs;

        match sizes(r) {
            Ok((free,allocated)) => {
                let size = r.metainfo_nblocks();

                let used = size - free;
                let used_percent = (used as f64 * 100.0) / (size as f64);

                let free = self.format_size(free);
                let _allocated = self.format_size(allocated);
                let size = self.format_size(size);

                self.print("   Free Space: ")
                    .dim_style()
                    .println(format!("{} / {} ({:.1}% used)", free, size, used_percent))
                    .pop();
            },
            Err(e) => {
                self.println(format!("  Error reading size of image free space: {}", e));
            }
        };
        self.newline();
    }

    fn format_size(&mut self, size: usize) -> String {
        let megs = size as f64 / 256.0;
        let gigs = megs / 1024.0;
        if gigs < 1.0 {
            format!("{:.2} mb", megs)
        } else {
            format!("{:.2} gb", gigs)
        }
    }

    fn render_activation(&mut self) {

        let activation = match self.realmfs.activation() {
            Some(activation) => activation,
            None => return,
        };

        let realms = self.realmfs.manager()
            .realms_for_activation(&activation);

        if !realms.is_empty() {
            self.heading("In Use")
                .print("  ")
                .activated_style();

            for realm in realms {
                self.print(realm.name()).print("  ");
            }
            self.pop().newlines(2);
        } else {
            self.heading("Active").newlines(2);
        }

        self.print("   Device : ")
            .dim_style()
            .println(activation.device())
            .pop();

        let mount = if activation.mountpoint_rw().is_some() { "Mounts" } else { "Mount "};
        self.print(format!("   {} : ", mount))
            .dim_style()
            .print(format!("{}", activation.mountpoint()))
            .pop()
            .newline();

        if let Some(rw) = activation.mountpoint_rw() {
            self.print("            ")
                .dim_style()
                .print(format!("{}", rw))
                .pop()
                .newline();
        }
        self.newline();
    }

    fn render_notes(&self) {
        let notes = match self.realmfs.notes() {
            Some(notes) => notes,
            None => return,
        };

        self.heading("Notes").newlines(2).dim_style();

        for line in notes.lines() {
            self.print("      ").println(line);
        }
        self.pop();
    }
}

impl <'a> InfoRenderer for RealmFSInfoRender<'a> {
    fn state(&self) -> Rc<ItemRenderState> {
        self.state.clone()
    }
}
