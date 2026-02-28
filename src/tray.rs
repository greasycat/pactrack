use std::cell::RefCell;
use std::ffi::{CString, c_char, c_int, c_void};
use std::path::Path;
use std::process::Child;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use glib::ControlFlow;
use gtk::prelude::*;
use libloading::Library;
use log::{debug, error, info};

use crate::commands::{
    DetectedAurHelper, build_details_shell_command, build_upgrade_aur_shell_command,
    build_upgrade_official_shell_command, build_upgrade_shell_command, launch_in_terminal,
    launch_in_terminal_process,
};
use crate::config::EffectiveConfig;
use crate::icons;
use crate::notifier;
use crate::scheduler::{SchedulerCommand, SchedulerUpdate, start_scheduler};
use crate::state::{AppState, Status, UpdateSnapshot};

pub fn run(config: EffectiveConfig) -> Result<(), String> {
    gtk::init().map_err(|e| format!("failed to initialize GTK: {e}"))?;

    let icon_dir = icons::install_fallback_icons()
        .map_err(|e| format!("failed to install fallback icons: {e}"))?;

    let api = Arc::new(AppIndicatorApi::load()?);
    let indicator = AppIndicator::new(api, "pactrack", "software-update-available")?;
    indicator.set_status_active();
    indicator.set_icon_theme_path(&icon_dir);

    let menu = gtk::Menu::new();
    let status_item = gtk::MenuItem::with_label("Status: checking");
    status_item.set_sensitive(false);

    let official_item = gtk::MenuItem::with_label("Official updates: 0");
    official_item.set_sensitive(false);

    let aur_item = gtk::MenuItem::with_label("AUR updates: 0");
    aur_item.set_sensitive(false);

    let checked_item = gtk::MenuItem::with_label("Last check: never");
    checked_item.set_sensitive(false);

    let refresh_item = gtk::MenuItem::with_label("Refresh now");
    let details_item = gtk::MenuItem::with_label("Open details");
    let upgrade_item = gtk::MenuItem::with_label("Upgrade all");
    let upgrade_official_item = gtk::MenuItem::with_label("Upgrade official only");
    let upgrade_aur_item = gtk::MenuItem::with_label("Upgrade AUR only");
    upgrade_aur_item.set_sensitive(false);
    let quit_item = gtk::MenuItem::with_label("Quit");

    menu.append(&status_item);
    menu.append(&official_item);
    menu.append(&aur_item);
    menu.append(&checked_item);
    menu.append(&gtk::SeparatorMenuItem::new());
    menu.append(&refresh_item);
    menu.append(&details_item);
    menu.append(&upgrade_item);
    menu.append(&upgrade_official_item);
    menu.append(&upgrade_aur_item);
    menu.append(&gtk::SeparatorMenuItem::new());
    menu.append(&quit_item);
    menu.show_all();
    indicator.set_menu(&menu);

    let (updates_tx, updates_rx) = mpsc::channel::<SchedulerUpdate>();
    let scheduler_tx = start_scheduler(config.clone(), updates_tx);

    {
        let scheduler_tx = scheduler_tx.clone();
        refresh_item.connect_activate(move |_| {
            if scheduler_tx.send(SchedulerCommand::RefreshNow).is_err() {
                error!("failed to send refresh command to scheduler");
            }
        });
    }

    #[derive(Default)]
    struct RuntimeState {
        previous_total_count: Option<usize>,
        helper: Option<DetectedAurHelper>,
        _snapshot: Option<UpdateSnapshot>,
    }

    let runtime_state = Rc::new(RefCell::new(RuntimeState::default()));

    {
        let runtime_state = Rc::clone(&runtime_state);
        let cfg = config.clone();
        details_item.connect_activate(move |_| {
            let helper = runtime_state.borrow().helper;
            match build_details_shell_command(&cfg, helper)
                .and_then(|command| launch_in_terminal(&cfg, &command))
            {
                Ok(()) => info!("opened details terminal"),
                Err(err) => error!("failed to open details terminal: {err}"),
            }
        });
    }

    {
        let runtime_state = Rc::clone(&runtime_state);
        let cfg = config.clone();
        let scheduler_tx = scheduler_tx.clone();
        upgrade_item.connect_activate(move |_| {
            let helper = runtime_state.borrow().helper;
            let command = build_upgrade_shell_command(&cfg, helper);
            match launch_in_terminal_process(&cfg, &command) {
                Ok(child) => {
                    info!("opened upgrade terminal");
                    queue_refresh_when_process_exits(child, scheduler_tx.clone());
                }
                Err(err) => error!("failed to open upgrade terminal: {err}"),
            }
        });
    }

    {
        let cfg = config.clone();
        let scheduler_tx = scheduler_tx.clone();
        upgrade_official_item.connect_activate(move |_| {
            let command = build_upgrade_official_shell_command();
            match launch_in_terminal_process(&cfg, &command) {
                Ok(child) => {
                    info!("opened official upgrade terminal");
                    queue_refresh_when_process_exits(child, scheduler_tx.clone());
                }
                Err(err) => error!("failed to open official upgrade terminal: {err}"),
            }
        });
    }

    {
        let runtime_state = Rc::clone(&runtime_state);
        let cfg = config.clone();
        let scheduler_tx = scheduler_tx.clone();
        upgrade_aur_item.connect_activate(move |_| {
            let helper = runtime_state.borrow().helper;
            let Some(command) = build_upgrade_aur_shell_command(helper) else {
                error!("cannot run AUR upgrade: AUR helper not detected");
                return;
            };

            match launch_in_terminal_process(&cfg, &command) {
                Ok(child) => {
                    info!("opened AUR upgrade terminal");
                    queue_refresh_when_process_exits(child, scheduler_tx.clone());
                }
                Err(err) => error!("failed to open AUR upgrade terminal: {err}"),
            }
        });
    }

    quit_item.connect_activate(move |_| {
        gtk::main_quit();
    });

    let status_item_ref = status_item.clone();
    let official_item_ref = official_item.clone();
    let aur_item_ref = aur_item.clone();
    let checked_item_ref = checked_item.clone();
    let upgrade_aur_item_ref = upgrade_aur_item.clone();
    let indicator_ref = indicator.clone();
    let notify_enabled = config.notify_on_change;
    let enable_aur = config.enable_aur;

    glib::timeout_add_local(Duration::from_millis(350), move || {
        while let Ok(update) = updates_rx.try_recv() {
            apply_update_to_menu(
                &indicator_ref,
                &status_item_ref,
                &official_item_ref,
                &aur_item_ref,
                &checked_item_ref,
                &update.state,
                &icon_dir,
            );

            let mut rt = runtime_state.borrow_mut();
            rt.helper = update.helper;
            upgrade_aur_item_ref.set_sensitive(enable_aur && rt.helper.is_some());
            if let Some(snapshot) = update.snapshot {
                rt._snapshot = Some(snapshot);
            }

            if notify_enabled {
                if update.state.status != Status::Checking {
                    if let Some(prev) = rt.previous_total_count {
                        if prev != update.state.total_count {
                            notifier::notify_count_change(prev, update.state.total_count);
                        }
                    }
                    rt.previous_total_count = Some(update.state.total_count);
                }
            }
        }
        ControlFlow::Continue
    });

    gtk::main();

    if scheduler_tx.send(SchedulerCommand::Quit).is_err() {
        debug!("scheduler already stopped");
    }

    Ok(())
}

fn queue_refresh_when_process_exits(child: Child, scheduler_tx: mpsc::Sender<SchedulerCommand>) {
    thread::spawn(move || {
        let mut child = child;
        if let Err(err) = child.wait() {
            error!("failed waiting for terminal process: {err}");
            return;
        }

        if scheduler_tx.send(SchedulerCommand::RefreshNow).is_err() {
            debug!("failed to queue refresh after upgrade completion");
        }
    });
}

fn apply_update_to_menu(
    indicator: &AppIndicator,
    status_item: &gtk::MenuItem,
    official_item: &gtk::MenuItem,
    aur_item: &gtk::MenuItem,
    checked_item: &gtk::MenuItem,
    state: &AppState,
    icon_dir: &Path,
) {
    status_item.set_label(&format!("Status: {}", status_text(state)));
    official_item.set_label(&format!("Official updates: {}", state.official_count));
    aur_item.set_label(&format!("AUR updates: {}", state.aur_count));

    let checked = state
        .last_checked
        .map(|ts| ts.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "never".to_string());
    checked_item.set_label(&format!("Last check: {checked}"));

    indicator.set_icon_theme_path(icon_dir);
    let icon = choose_icon_name(&state.status);
    indicator.set_icon(icon);
}

fn status_text(state: &AppState) -> String {
    match state.status {
        Status::Checking => "checking".to_string(),
        Status::UpToDate => "up to date".to_string(),
        Status::UpdatesAvailable => format!("{} updates available", state.total_count),
        Status::Error => {
            let msg = state
                .last_error
                .as_deref()
                .map(truncate_error)
                .unwrap_or_else(|| "unknown error".to_string());
            format!("error ({msg})")
        }
    }
}

fn truncate_error(msg: &str) -> String {
    let max = 72usize;
    if msg.chars().count() <= max {
        msg.to_string()
    } else {
        msg.chars().take(max).collect::<String>() + "..."
    }
}

fn choose_icon_name(status: &Status) -> &'static str {
    let (theme_icon, fallback_icon) = icons::icon_candidates(status);
    if gtk::IconTheme::default()
        .map(|theme| theme.has_icon(theme_icon))
        .unwrap_or(false)
    {
        theme_icon
    } else {
        fallback_icon
    }
}

#[derive(Clone)]
struct AppIndicator {
    api: Arc<AppIndicatorApi>,
    raw: *mut c_void,
}

impl AppIndicator {
    fn new(api: Arc<AppIndicatorApi>, id: &str, icon_name: &str) -> Result<Self, String> {
        let id = CString::new(id).map_err(|_| "invalid tray id".to_string())?;
        let icon_name = CString::new(icon_name).map_err(|_| "invalid icon name".to_string())?;

        let raw = unsafe {
            (api.new)(
                id.as_ptr(),
                icon_name.as_ptr(),
                APP_INDICATOR_CATEGORY_APPLICATION_STATUS,
            )
        };

        if raw.is_null() {
            return Err("app_indicator_new returned null".to_string());
        }

        Ok(Self { api, raw })
    }

    fn set_status_active(&self) {
        unsafe { (self.api.set_status)(self.raw, APP_INDICATOR_STATUS_ACTIVE) };
    }

    fn set_menu(&self, menu: &gtk::Menu) {
        unsafe {
            (self.api.set_menu)(self.raw, menu.as_ptr() as *mut c_void);
        }
    }

    fn set_icon(&self, icon_name: &str) {
        if let Ok(icon) = CString::new(icon_name) {
            unsafe {
                (self.api.set_icon)(self.raw, icon.as_ptr());
            }
        }
    }

    fn set_icon_theme_path(&self, path: &Path) {
        if let Some(path) = path.to_str() {
            if let Ok(path) = CString::new(path) {
                unsafe {
                    (self.api.set_icon_theme_path)(self.raw, path.as_ptr());
                }
            }
        }
    }
}

struct AppIndicatorApi {
    _lib: Library,
    new: unsafe extern "C" fn(*const c_char, *const c_char, c_int) -> *mut c_void,
    set_status: unsafe extern "C" fn(*mut c_void, c_int),
    set_menu: unsafe extern "C" fn(*mut c_void, *mut c_void),
    set_icon: unsafe extern "C" fn(*mut c_void, *const c_char),
    set_icon_theme_path: unsafe extern "C" fn(*mut c_void, *const c_char),
}

impl AppIndicatorApi {
    fn load() -> Result<Self, String> {
        let libraries = ["libayatana-appindicator3.so.1", "libappindicator3.so.1"];

        for name in libraries {
            match unsafe { Library::new(name) } {
                Ok(lib) => {
                    return unsafe { Self::from_library(lib) }
                        .map_err(|e| format!("failed to load symbols from {name}: {e}"));
                }
                Err(_) => continue,
            }
        }

        Err("could not load libayatana-appindicator3.so.1 or libappindicator3.so.1".into())
    }

    unsafe fn from_library(lib: Library) -> Result<Self, libloading::Error> {
        let new = unsafe {
            *lib.get::<unsafe extern "C" fn(*const c_char, *const c_char, c_int) -> *mut c_void>(
                b"app_indicator_new\0",
            )?
        };
        let set_status = unsafe {
            *lib.get::<unsafe extern "C" fn(*mut c_void, c_int)>(b"app_indicator_set_status\0")?
        };
        let set_menu = unsafe {
            *lib.get::<unsafe extern "C" fn(*mut c_void, *mut c_void)>(b"app_indicator_set_menu\0")?
        };
        let set_icon = unsafe {
            *lib.get::<unsafe extern "C" fn(*mut c_void, *const c_char)>(
                b"app_indicator_set_icon\0",
            )?
        };
        let set_icon_theme_path = unsafe {
            *lib.get::<unsafe extern "C" fn(*mut c_void, *const c_char)>(
                b"app_indicator_set_icon_theme_path\0",
            )?
        };

        Ok(Self {
            _lib: lib,
            new,
            set_status,
            set_menu,
            set_icon,
            set_icon_theme_path,
        })
    }
}

const APP_INDICATOR_CATEGORY_APPLICATION_STATUS: c_int = 0;
const APP_INDICATOR_STATUS_ACTIVE: c_int = 1;
