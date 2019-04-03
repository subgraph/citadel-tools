use std::fs;
use std::ffi::OsStr;
use std::fmt::{Display,self};
use std::sync::{Arc, RwLock, Weak, RwLockWriteGuard, RwLockReadGuard};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self,JoinHandle};
use std::path;

use crate::{RealmManager, Result, Realm};
use super::realms::HasCurrentChanged;
use dbus::{Connection, BusType, ConnectionItem, Message, Path};
use inotify::{Inotify, WatchMask, WatchDescriptor, Event};

pub enum RealmEvent {
    Started(Realm),
    Stopped(Realm),
    New(Realm),
    Removed(Realm),
    Current(Option<Realm>),
}

impl Display for RealmEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RealmEvent::Started(ref realm)   => write!(f, "RealmStarted({})", realm.name()),
            RealmEvent::Stopped(ref realm)   => write!(f, "RealmStopped({})", realm.name()),
            RealmEvent::New(ref realm)       => write!(f, "RealmNew({})", realm.name()),
            RealmEvent::Removed(ref realm)   => write!(f, "RealmRemoved({})", realm.name()),
            RealmEvent::Current(Some(realm)) => write!(f, "RealmCurrent({})", realm.name()),
            RealmEvent::Current(None)        => write!(f, "RealmCurrent(None)"),
        }
    }
}

pub type RealmEventHandler = Fn(&RealmEvent)+Send+Sync;

pub struct RealmEventListener {
    inner: Arc<RwLock<Inner>>,
    running: Arc<AtomicBool>,
    join: Vec<JoinHandle<Result<()>>>,
}

struct Inner {
    manager: Weak<RealmManager>,
    handlers: Vec<Box<RealmEventHandler>>,
    quit: Arc<AtomicBool>,
}

impl Inner {
    fn new() -> Self {
        Inner {
            manager: Weak::new(),
            handlers: Vec::new(),
            quit: Arc::new(AtomicBool::new(false)),
        }
    }

    fn set_manager(&mut self, manager: &Arc<RealmManager>) {
        self.manager = Arc::downgrade(manager);
    }

    pub fn add_handler<F>(&mut self, handler: F)
        where F: Fn(&RealmEvent),
              F: 'static + Send + Sync
    {
        self.handlers.push(Box::new(handler));
    }

    fn send_event(&self, event: RealmEvent) {
        self.handlers.iter().for_each(|cb| (cb)(&event));
    }

    fn quit_flag(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }

    fn set_quit_flag(&self, val: bool) {
        self.quit.store(val, Ordering::SeqCst)
    }

    fn with_manager<F>(&self, f: F)
        where F: Fn(&RealmManager)
    {
        if let Some(manager) = self.manager.upgrade() {
            f(&manager)
        }
    }
}

impl RealmEventListener {

    pub fn new() -> Self {
        RealmEventListener {
            inner: Arc::new(RwLock::new(Inner::new())),
            running: Arc::new(AtomicBool::new(false)),
            join: Vec::new(),
        }
    }

    pub fn set_manager(&self, manager: &Arc<RealmManager>) {
        self.inner_mut().set_manager(manager);
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn set_running(&self, val: bool) -> bool {
        self.running.swap(val, Ordering::SeqCst)
    }

    pub fn add_handler<F>(&self, handler: F)
        where F: Fn(&RealmEvent),
              F: 'static + Send + Sync
    {
        self.inner_mut().add_handler(handler);
    }

    fn inner_mut(&self) -> RwLockWriteGuard<Inner> {
        self.inner.write().unwrap()
    }

    fn inner(&self) -> RwLockReadGuard<Inner> {
        self.inner.read().unwrap()
    }

    pub fn start_event_task(&mut self) -> Result<()> {
        if self.set_running(true) {
            warn!("RealmEventListener already running");
            return Ok(());
        }

        let inotify_handle = match InotifyEventListener::create(self.inner.clone()) {
            Ok(inotify) => inotify.spawn(),
            Err(e) => {
                self.set_running(false);
                return Err(e);
            }
        };
        let dbus_handle = DbusEventListener::new(self.inner.clone()).spawn();

        self.join.clear();
        self.join.push(inotify_handle);
        self.join.push(dbus_handle);

        Ok(())
    }

    fn notify_stop(&self) -> bool {
        let lock = self.inner();

        let can_stop = self.is_running() && !lock.quit_flag();

        if can_stop {
            lock.set_quit_flag(true);
        }
        can_stop
    }

    pub fn stop(&mut self) {
        if !self.notify_stop() {
            return;
        }

        info!("Stopping event listening task");

        if let Err(e) = InotifyEventListener::wake_inotify() {
            warn!("error signaling inotify task by creating a file: {}", e);
        }

        thread::spawn({
            let handles: Vec<_> = self.join.drain(..).collect();
            let running = self.running.clone();
            let quit = self.inner().quit.clone();
            move || {
                for join in handles {
                    if let Err(err) = join.join().unwrap() {
                        warn!("error from event task: {}", err);
                    }
                }
                running.store(false, Ordering::SeqCst);
                quit.store(false, Ordering::SeqCst);
                info!("Event listening task stopped");
            }
        });
    }
}

impl Drop for RealmEventListener {
    fn drop(&mut self) {
        self.inner().set_quit_flag(true);
    }
}

#[derive(Clone)]
struct DbusEventListener {
    inner: Arc<RwLock<Inner>>,
}

impl DbusEventListener {
    fn new(inner: Arc<RwLock<Inner>>) -> Self {
        DbusEventListener { inner }
    }

    fn spawn(self) -> JoinHandle<Result<()>> {
        thread::spawn(move || {
            if let Err(err) = self.dbus_event_loop() {
                warn!("dbus_event_loop(): {}", err);
            }
            Ok(())
        })
    }

    fn dbus_event_loop(&self) -> Result<()> {
        let connection = Connection::get_private(BusType::System)?;
        connection.add_match("interface='org.freedesktop.machine1.Manager',type='signal'")?;
        for item in connection.iter(1000) {
            if self.inner().quit_flag() {
                break;
            }
            self.handle_item(item);
        }
        info!("Exiting dbus event loop");
        Ok(())
    }

    fn inner(&self) -> RwLockReadGuard<Inner> {
        self.inner.read().unwrap()
    }

    fn handle_item(&self, item: ConnectionItem) {
        if let ConnectionItem::Signal(message) = item {
            if let Some(interface) = message.interface() {
                if &(*interface) == "org.freedesktop.machine1.Manager" {
                    if let Err(e) = self.handle_signal(message) {
                        warn!("Error handling signal: {}", e);
                    }
                }
            }
        }
    }

    fn handle_signal(&self, message: Message) -> Result<()> {

        let member = message.member()
            .ok_or_else(|| format_err!("invalid signal"))?;
        let (name, _path): (String, Path) = message.read2()?;
        if let (Some(interface),Some(member)) = (message.interface(),message.member()) {
            verbose!("DBUS: {}:[{}({})]", interface, member,name);
        }
        match &*member {
            "MachineNew" => self.on_machine_new(&name),
            "MachineRemoved" => self.on_machine_removed(&name),
            _ => {},
        };
        Ok(())
    }

    fn on_machine_new(&self, name: &str) {
        self.inner().with_manager(|m| {
            if let Some(realm) = m.realm_by_name(name) {
                realm.set_active(true);
                self.inner().send_event(RealmEvent::Started(realm))
            }
        });
    }

    fn on_machine_removed(&self, name: &str) {
        self.inner().with_manager(|m| {
            if let Some(realm) = m.on_machine_removed(name) {
                self.inner().send_event(RealmEvent::Stopped(realm))
            }

        });
    }
}

struct InotifyEventListener {
    inner: Arc<RwLock<Inner>>,
    inotify: Inotify,
    realms_watch: WatchDescriptor,
    current_watch: WatchDescriptor,

}

impl InotifyEventListener {

    fn create(inner: Arc<RwLock<Inner>>) -> Result<Self> {
        let mut inotify = Inotify::init()?;
        let realms_watch = inotify.add_watch("/realms", WatchMask::MOVED_FROM|WatchMask::MOVED_TO)?;
        let current_watch = inotify.add_watch("/run/citadel/realms/current", WatchMask::CREATE|WatchMask::MOVED_TO)?;

        Ok(InotifyEventListener { inner, inotify, realms_watch, current_watch, })
    }

    fn wake_inotify() -> Result<()> {
        let path = "/run/citadel/realms/current/stop-events";
        fs::File::create(path)?;
        fs::remove_file(path)?;
        Ok(())
    }

    fn spawn(mut self) -> JoinHandle<Result<()>> {
        thread::spawn(move || self.inotify_event_loop())
    }

    fn inotify_event_loop(&mut self) -> Result<()> {
        let mut buffer = [0; 1024];
        while !self.inner().quit_flag() {
            let events = self.inotify.read_events_blocking(&mut buffer)?;

            if !self.inner().quit_flag() {
                for event in events {
                    self.handle_event(event);
                }
            }
        }
        info!("Exiting inotify event loop");
        Ok(())
    }

    fn handle_event(&self, event: Event<&OsStr>) {
        self.log_event(&event);
        if event.wd == self.current_watch {
            self.handle_current_event();
        } else if event.wd == self.realms_watch {
            self.handle_realm_event();
        }
    }

    fn log_event(&self, event: &Event<&OsStr>) {
        if let Some(name) = event.name {
            let path = path::Path::new("/realms").join(name);
            verbose!("INOTIFY: {} ({:?})", path.display(), event.mask);
        } else {
            verbose!("INOTIFY: ({:?})", event.mask);

        }
    }

    fn inner(&self) -> RwLockReadGuard<Inner> {
        self.inner.read().unwrap()
    }

    fn handle_current_event(&self) {
        self.inner().with_manager(|m| {
            if let HasCurrentChanged::Changed(current) = m.has_current_changed() {
                self.inner().send_event(RealmEvent::Current(current));
            }
        })
    }

    fn handle_realm_event(&self) {
        self.inner().with_manager(|m| {
            let (added,removed) = match m.rescan_realms() {
                Ok(result) => result,
                Err(e) => {
                    warn!("error rescanning realms: {}", e);
                    return;
                }
            };
            for realm in added {
                self.inner().send_event(RealmEvent::New(realm));
            }
            for realm in removed {
                self.inner().send_event(RealmEvent::Removed(realm));
            }
        })
    }
}
