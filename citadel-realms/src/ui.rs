
use cursive::{Cursive, event::{Event, Key, EventResult}, traits::View, views::LinearLayout, CbSink, ScreenId};

use libcitadel::{Result, RealmFS, Logger, LogLevel, Realm, RealmManager,RealmEvent};

use crate::backend::Backend;
use crate::logview::LogView;
use crate::help::{help_panel};
use crate::theme::{ThemeHandler, ThemeChooser};
use crate::terminal::TerminalTools;
use crate::logview::TextContentLogOutput;
use std::sync::{Arc,RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{mem, io};
use crate::item_list::ItemList;
use crate::realm::RealmListContent;
use crate::realmfs::RealmFSListContent;
use std::io::Write;

#[derive(Clone)]
pub enum DeferredAction {
    None,
    RealmShell(Realm, bool),
    UpdateRealmFS(RealmFS),
}

pub struct GlobalState {
    deferred: DeferredAction,
    log_output: TextContentLogOutput,
}

impl GlobalState {
    fn new(log_output: TextContentLogOutput) -> Self {
        GlobalState { log_output, deferred: DeferredAction::None }
    }

    pub fn set_deferred(&mut self, deferred: DeferredAction) {
        self.deferred = deferred;
    }

    fn take_deferred(&mut self) -> DeferredAction {
        mem::replace(&mut self.deferred, DeferredAction::None)
    }

    pub fn log_output(&self) -> &TextContentLogOutput {
        &self.log_output
    }
}


#[derive(Clone)]
pub struct RealmUI {
    manager: Arc<RealmManager>,
    inner: Arc<RwLock<Inner>>,
    log_output: TextContentLogOutput,
}

struct Inner {
    termtools: TerminalTools,
    sink: Option<CbSink>,
    screen: ScreenId,
}

impl Inner {
    fn new() -> Self {
        let termtools = TerminalTools::new();
        Inner {
            termtools,
            sink: None,
            screen: RealmUI::SCREEN_REALM,
        }
    }
}

impl RealmUI {
    const SCREEN_REALMFS: ScreenId = 0;
    const SCREEN_REALM  : ScreenId = 1;

    pub fn create() -> Result<Self> {

        let log_output = TextContentLogOutput::new();
        Logger::set_log_level(LogLevel::Debug);
        log_output.set_as_log_output();

        let manager = RealmManager::load()?;
        let inner = Arc::new(RwLock::new(Inner::new()));

        Ok(RealmUI{ manager, inner, log_output })
    }

    fn inner(&self) -> RwLockReadGuard<Inner> {
        self.inner.read().unwrap()
    }

    fn inner_mut(&self) -> RwLockWriteGuard<Inner> {
        self.inner.write().unwrap()
    }

    fn with_termtools<F>(&self, f: F)
        where F: Fn(&mut TerminalTools)
    {
        let mut inner = self.inner_mut();
        f(&mut inner.termtools)
    }

    fn setup(&self) {
        self.with_termtools(|tt| {
            tt.push_window_title();
            tt.save_palette();
            tt.set_window_title("Realm Manager");
        });


        self.manager.add_event_handler({
            let ui = self.clone();
            move |ev| ui.handle_event(ev)
        });

        if let Err(e) = self.manager.start_event_task() {
            warn!("error starting realm manager event task: {}", e);
        }
    }


    pub fn start(&self) {
        self.setup();
        loop {
            match self.run_ui() {
                DeferredAction::RealmShell(ref realm, root) => {
//                    self.inner_mut().screen = Self::SCREEN_REALM;
                    self.log_output.set_default_enabled(true);
                    if let Err(e) = self.run_realm_shell(realm, root) {
                        println!("Error running shell: {}", e);
                    }
                    self.with_termtools(|tt| {
                        tt.set_window_title("Realm Manager");
                        tt.restore_palette();
                    });
                },
                DeferredAction::UpdateRealmFS(ref realmfs) => {
//                    self.inner_mut().screen = Self::SCREEN_REALMFS;
                    self.log_output.set_default_enabled(true);
                    if let Err(e) = self.run_realmfs_update(realmfs) {
                        println!("Error running shell: {}", e);
                        self.with_termtools(|tt| tt.pop_window_title());
                        return;
                    }

                },
                DeferredAction::None => {
                    self.with_termtools(|tt| tt.pop_window_title());
                    return;
                },
            }

        }
    }

    fn handle_event(&self, ev: &RealmEvent) {
        info!("event: {}", ev);
        match ev {
            _ => self.send_sink(|s| {
                if s.active_screen() == Self::SCREEN_REALM {
                    ItemList::<Realm>::call_reload("realms", s);
                } else {
                    ItemList::<RealmFS>::call_reload("realmfs", s);
                }
            }),
        }
    }

    fn send_sink<F>(&self, f: F)
        where F: Fn(&mut Cursive)+'static+Send+Sync
    {
        let inner = self.inner();
        if let Some(ref sink) = inner.sink {
            if let Err(e) = sink.send(Box::new(f)) {
                warn!("error sending message to ui event sink: {}", e);
            }
        }
    }

    fn set_sink(&self, sink: CbSink) {
        self.inner_mut().sink = Some(sink);
    }

    fn clear_sink(&self) {
        self.inner_mut().sink = None;
    }

    fn run_ui(&self) -> DeferredAction {
        self.log_output.set_default_enabled(false);
        let mut siv = Cursive::try_new(Backend::init).unwrap();

        siv.set_user_data(GlobalState::new(self.log_output.clone()));

        siv.set_theme(ThemeHandler::load_base16_theme());

        Self::setup_global_callbacks(&mut siv);

        let content = RealmFSListContent::new(self.manager.clone());
        siv.add_fullscreen_layer(LinearLayout::vertical()
            .child(ItemList::create("realmfs", "RealmFS Images", content))
            .child(LogView::create(self.log_output.text_content())));

        siv.add_active_screen();

        let content = RealmListContent::new(self.manager.clone());
        siv.add_fullscreen_layer(LinearLayout::vertical()
            .child(ItemList::create("realms", "Realms", content))
            .child(LogView::create(self.log_output.text_content())));

        self.set_sink(siv.cb_sink().clone());

        siv.set_screen(self.inner().screen);

        siv.run();

        self.inner_mut().screen = siv.active_screen();

        self.clear_sink();

        match siv.user_data::<GlobalState>() {
            Some(gs) => gs.take_deferred(),
            None => DeferredAction::None,
        }

    }

    fn setup_global_callbacks(siv: &mut Cursive) {

        fn inject_event(s: &mut Cursive, event: Event) {
            if let EventResult::Consumed(Some(callback)) = s.screen_mut().on_event(event) {
                (callback)(s);
            }
        }

        fn is_top_layer(s: &Cursive) -> bool {
            let sizes = s.screen().layer_sizes();
            sizes.len() == 1
        }

        siv.add_global_callback('q', |s| {
            if is_top_layer(s) {
                s.quit();
            } else {
                s.pop_layer();
            }
        });
        siv.add_global_callback(Key::Esc, |s| {
            if !is_top_layer(s) {
                s.pop_layer();
            }
        });

        siv.add_global_callback('j', |s| inject_event(s, Event::Key(Key::Down)));
        siv.add_global_callback('k', |s| inject_event(s, Event::Key(Key::Up)));
        siv.add_global_callback('l', |s| inject_event(s, Event::Key(Key::Right)));
        siv.add_global_callback('h', |s| {
            if is_top_layer(s) {
                s.add_layer(help_panel(s.active_screen()))
            } else {
                inject_event(s, Event::Key(Key::Left));
            }
        });

        siv.add_global_callback('?', |s| {
            if is_top_layer(s) {
                s.add_layer(help_panel(s.active_screen()))
            }
        });

        siv.add_global_callback('l', |s| {
            if is_top_layer(s) {
                s.call_on_id("log", |log: &mut LogView| {
                    log.toggle_hidden();
                });
            }
        });

        siv.add_global_callback('L', |s| {
            if is_top_layer(s) {
                LogView::open_popup(s);
            }
        });

        siv.add_global_callback('T', |s| {
            if is_top_layer(s) {
                ThemeChooser::open(s);
            }
        });

        siv.add_global_callback(' ', |s| {
            if !is_top_layer(s) {
                return;
            }
            if s.active_screen() == Self::SCREEN_REALMFS {
                s.set_screen(Self::SCREEN_REALM);
            } else {
                s.set_screen(Self::SCREEN_REALMFS);
            }
        });
    }

    fn run_realm_shell(&self, realm: &Realm, rootshell: bool) -> Result<()> {
        self.with_termtools(|tt| {
            tt.apply_base16_by_slug(realm.config().terminal_scheme()
                .unwrap_or("default-dark"));
            tt.set_window_title(format!("realm-{}", realm.name()));
            tt.clear_screen();

        });

        if !realm.is_active() {
            self.manager.start_realm(realm)?;
        }

        let shelltype = if rootshell { "root" } else { "user" };
        println!();
        println!("Opening {} shell in realm '{}'", shelltype, realm.name());
        println!();
        println!("Exit shell with ctrl-d or 'exit' to return to realm manager");
        println!();
        self.manager.launch_shell(realm, rootshell)?;
        Ok(())
    }

    fn run_realmfs_update(&self, realmfs: &RealmFS) -> Result<()> {
        self.with_termtools(|tt| {
            tt.apply_base16_by_slug("icy");
            tt.set_window_title(format!("Update {}-realmfs.img", realmfs.name()));
            tt.clear_screen();
        });

        let mut update = realmfs.update();
        update.setup()?;

        if let Some(size) = update.auto_resize_size() {
            println!("Resizing image to {} gb", size.size_in_gb());
            update.apply_resize(size)?;
        }

        println!();
        println!("Opening update shell for '{}-realmfs.img'", realmfs.name());
        println!();
        println!("Exit shell with ctrl-d or 'exit' to return to realm manager");
        println!();
        update.open_update_shell()?;

        if realmfs.is_sealed() {
            if self.prompt_user("Apply changes?", true)? {
                update.apply_update()
            } else {
                update.cleanup()
            }
        } else {
            update.apply_update()?;
            if !realmfs.is_activated() && self.prompt_user("Seal RealmFS?", true)? {
                realmfs.seal(None)?;
            }
            Ok(())
        }
    }

    fn prompt_user(&self, prompt: &str, default_y: bool) -> Result<bool> {
        let yn = if default_y { "(Y/n)" } else { "(y/N)" };
        print!("{} {} : ", prompt, yn);
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;

        let yes = match line.trim().chars().next() {
            Some(c) => c == 'Y' || c == 'y',
            None => default_y,
        };
        Ok(yes)
    }

}
