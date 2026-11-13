#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod history;
mod net;
mod ping;
mod update;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use slint::{
    ComponentHandle, LogicalPosition, Model, ModelRc, PhysicalPosition, SharedString, Timer, TimerMode, VecModel,
};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

slint::include_modules!();

struct Worker {
    name: String,
    host: String,
    shared: ping::Shared,
}

/// Remove the window's taskbar button (tool-window style); the tray icon is the only entry point.
fn hide_from_taskbar() {
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::w;
    unsafe {
        if let Ok(hwnd) = FindWindowW(None, w!("NetPulse")) {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let want = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
            if want != ex {
                // Style change only takes effect on the taskbar after a hide/show cycle.
                let _ = ShowWindow(hwnd, SW_HIDE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
                let _ = ShowWindow(hwnd, SW_SHOWNA);
            }
        }
    }
}

fn tray_icon_image() -> Icon {
    // 32x32 RGBA generated alongside the exe icon (assets/make_icon.py output).
    Icon::from_rgba(include_bytes!("../assets/tray32.rgba").to_vec(), 32, 32).expect("tray icon")
}

fn build_view(w: &Worker, s: &ping::Stats) -> TargetView {
    let shown_host = match (&s.adapter, s.resolved) {
        (Some(a), Some(ip)) => format!("{a} · {ip}"),
        (None, Some(ip)) if ip.to_string() != w.host => format!("{} · {ip}", w.host),
        (_, Some(_)) => w.host.clone(),
        (_, None) if w.host.eq_ignore_ascii_case("gateway") => "no gateway found".to_string(),
        (_, None) => format!("{} · unresolved", w.host),
    };

    let samples: Vec<Option<u32>> = s.history.iter().copied().collect();
    let ok: Vec<u32> = samples.iter().flatten().copied().collect();
    let (latency, status) = match samples.last() {
        None => ("…".to_string(), 0),
        Some(None) => ("timeout".to_string(), 3),
        Some(Some(0)) => ("<1 ms".to_string(), 1),
        Some(Some(ms)) => (format!("{ms} ms"), if *ms < 80 { 1 } else if *ms < 200 { 2 } else { 3 }),
    };

    let loss = if samples.is_empty() {
        0.0
    } else {
        100.0 * (samples.len() - ok.len()) as f32 / samples.len() as f32
    };
    let avg = if ok.is_empty() { 0.0 } else { ok.iter().sum::<u32>() as f32 / ok.len() as f32 };
    let jitter = if ok.len() < 2 {
        0.0
    } else {
        ok.windows(2).map(|p| (p[1] as f32 - p[0] as f32).abs()).sum::<f32>() / (ok.len() - 1) as f32
    };
    let live = if samples.is_empty() {
        String::new()
    } else {
        format!("avg {avg:.0} ms · jitter {jitter:.0} · loss {loss:.0}%")
    };

    // Sparkline in a 100x24 viewbox; timeouts break the line. Auto-scaled, floor of 20 ms.
    let peak = ok.iter().copied().max().unwrap_or(0).max(20) as f32;
    let mut spark = String::new();
    let mut pen_down = false;
    for (i, v) in samples.iter().enumerate() {
        match v {
            Some(ms) => {
                let x = 100.0 * i as f32 / (ping::HISTORY - 1) as f32;
                let y = 22.0 - 20.0 * (*ms as f32 / peak);
                spark.push_str(&format!("{} {x:.1} {y:.1} ", if pen_down { "L" } else { "M" }));
                pen_down = true;
            }
            None => pen_down = false,
        }
    }

    let buckets: Vec<history::Bucket> = s.buckets.iter().copied().collect();
    let sum = history::summarize(&buckets);
    let summary24 = if sum.sent == 0 {
        "24 h: no data".to_string()
    } else {
        format!("24 h: {:.1}% up · max {} ms", sum.uptime_pct, sum.max_ms)
    };

    TargetView {
        name: w.name.clone().into(),
        host: shown_host.into(),
        latency: latency.into(),
        live: live.into(),
        summary24: summary24.into(),
        status,
        spark: spark.into(),
        hours: ModelRc::new(VecModel::from(sum.hours)),
    }
}

fn snapshot_history(workers: &[Worker]) -> HashMap<String, Vec<history::Bucket>> {
    workers
        .iter()
        .map(|w| (history::key(&w.name, &w.host), w.shared.lock().unwrap().buckets.iter().copied().collect()))
        .collect()
}

fn main() -> Result<(), slint::PlatformError> {
    let cfg = config::load();
    let stop = Arc::new(AtomicBool::new(false));

    let interval = Duration::from_millis(cfg.interval_ms.max(200));
    let mut saved = history::load();
    let workers: Rc<Vec<Worker>> = Rc::new(
        cfg.targets
            .iter()
            .map(|t| Worker {
                name: t.name.clone(),
                host: t.host.clone(),
                shared: ping::spawn(
                    t.host.clone(),
                    interval,
                    cfg.timeout_ms,
                    stop.clone(),
                    saved.remove(&history::key(&t.name, &t.host)).unwrap_or_default(),
                ),
            })
            .collect(),
    );

    let ui = MainWindow::new()?;
    ui.set_on_top(cfg.always_on_top);
    ui.set_locked(cfg.locked);
    if let (Some(x), Some(y)) = (cfg.x, cfg.y) {
        ui.window().set_position(PhysicalPosition::new(x, y));
    }

    // Drag the frameless window by its header (ignored while locked).
    {
        let w = ui.as_weak();
        ui.on_drag(move |dx, dy| {
            if let Some(ui) = w.upgrade() {
                if ui.get_locked() {
                    return;
                }
                let win = ui.window();
                let sf = win.scale_factor();
                let p = win.position();
                win.set_position(PhysicalPosition::new(p.x + (dx * sf) as i32, p.y + (dy * sf) as i32));
            }
        });
    }

    // Tray icon + menu.
    let show_item = MenuItem::new("Show / Hide (minimize to tray)", true, None);
    let top_item = CheckMenuItem::new("Always on top", true, cfg.always_on_top, None);
    let lock_item = CheckMenuItem::new("Lock position", true, cfg.locked, None);
    let edit_item = MenuItem::new("Edit destinations...", true, None);
    let reload_item = MenuItem::new("Reload config", true, None);
    let update_item = MenuItem::new(format!("Check for updates (v{})", update::VERSION), true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &show_item,
        &PredefinedMenuItem::separator(),
        &top_item,
        &lock_item,
        &PredefinedMenuItem::separator(),
        &edit_item,
        &reload_item,
        &update_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])
    .ok();
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("NetPulse")
        .with_icon(tray_icon_image())
        .build()
        .expect("tray icon");

    let visible = Rc::new(Cell::new(true));
    let quit = {
        let w = ui.as_weak();
        let stop = stop.clone();
        let workers = workers.clone();
        Rc::new(move || {
            if let Some(ui) = w.upgrade() {
                let p = ui.window().position();
                let mut c = config::load();
                c.x = Some(p.x);
                c.y = Some(p.y);
                config::save(&c);
            }
            history::save(&snapshot_history(&workers));
            stop.store(true, Ordering::Relaxed);
            let _ = slint::quit_event_loop();
        })
    };
    let toggle = {
        let w = ui.as_weak();
        let visible = visible.clone();
        Rc::new(move || {
            if let Some(ui) = w.upgrade() {
                if visible.get() {
                    let _ = ui.hide();
                } else {
                    let _ = ui.show();
                    hide_from_taskbar();
                }
                visible.set(!visible.get());
            }
        })
    };
    let set_top: Rc<dyn Fn(bool)> = {
        let w = ui.as_weak();
        let item = top_item.clone();
        Rc::new(move |on| {
            if let Some(ui) = w.upgrade() {
                ui.set_on_top(on);
            }
            item.set_checked(on);
            config::update(|c| c.always_on_top = on);
        })
    };
    let set_lock: Rc<dyn Fn(bool)> = {
        let w = ui.as_weak();
        let item = lock_item.clone();
        Rc::new(move |on| {
            if let Some(ui) = w.upgrade() {
                ui.set_locked(on);
            }
            item.set_checked(on);
            config::update(|c| c.locked = on);
        })
    };
    let edit_config: Rc<dyn Fn()> = Rc::new(|| {
        config::load(); // make sure the file exists
        let _ = std::process::Command::new("notepad").arg(config::path()).spawn();
    });
    let reload: Rc<dyn Fn()> = {
        let q = quit.clone();
        Rc::new(move || {
            q(); // saves position + history first
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).spawn();
            }
        })
    };
    {
        let t = toggle.clone();
        ui.on_hide_clicked(move || t());
        let q = quit.clone();
        ui.on_quit_clicked(move || q());
        let (w, f) = (ui.as_weak(), set_top.clone());
        ui.on_toggle_top(move || {
            if let Some(ui) = w.upgrade() {
                f(!ui.get_on_top());
            }
        });
        let (w, f) = (ui.as_weak(), set_lock.clone());
        ui.on_toggle_lock(move || {
            if let Some(ui) = w.upgrade() {
                f(!ui.get_locked());
            }
        });
        let f = edit_config.clone();
        ui.on_edit_config(move || f());
        let f = reload.clone();
        ui.on_reload_config(move || f());
        ui.on_check_update(|| update::check_async(true));
        ui.set_version(update::VERSION.into());
    }
    if cfg.check_updates {
        Timer::single_shot(Duration::from_secs(10), || update::check_async(false));
    }

    // Persist the 24 h history once a minute.
    let save_timer = Timer::default();
    {
        let workers = workers.clone();
        save_timer.start(TimerMode::Repeated, Duration::from_secs(60), move || {
            history::save(&snapshot_history(&workers));
        });
    }

    // Poll stats + tray events.
    let model = Rc::new(VecModel::<TargetView>::default());
    ui.set_targets(ModelRc::from(model.clone()));
    let timer = Timer::default();
    {
        let w = ui.as_weak();
        let (toggle, quit, workers) = (toggle.clone(), quit.clone(), workers.clone());
        let (show_id, quit_id) = (show_item.id().clone(), quit_item.id().clone());
        let (top_id, lock_id) = (top_item.id().clone(), lock_item.id().clone());
        let (edit_id, reload_id) = (edit_item.id().clone(), reload_item.id().clone());
        let update_id = update_item.id().clone();
        timer.start(TimerMode::Repeated, Duration::from_millis(400), move || {
            if update::QUIT_REQUESTED.load(Ordering::Relaxed) {
                quit();
                return;
            }
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                if ev.id == update_id {
                    update::check_async(true);
                }
                if ev.id == show_id {
                    toggle();
                } else if ev.id == quit_id {
                    quit();
                } else if ev.id == top_id {
                    set_top(top_item.is_checked());
                } else if ev.id == lock_id {
                    set_lock(lock_item.is_checked());
                } else if ev.id == edit_id {
                    edit_config();
                } else if ev.id == reload_id {
                    reload();
                }
            }
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                    toggle();
                }
            }

            let mut views = Vec::with_capacity(workers.len());
            let mut worst = 0;
            let mut network = String::from("no network gateway");
            for wk in workers.iter() {
                let s = wk.shared.lock().unwrap();
                if wk.host.eq_ignore_ascii_case("gateway") {
                    if let (Some(a), Some(ip)) = (&s.adapter, s.resolved) {
                        network = format!("{a} · gateway {ip}");
                    }
                }
                let v = build_view(wk, &s);
                worst = worst.max(v.status);
                views.push(v);
            }
            if let Some(ui) = w.upgrade() {
                ui.set_network(network.into());
                ui.set_overall(worst);
                ui.set_status_text(SharedString::from(match worst {
                    0 => "STARTING",
                    1 => "ALL GOOD",
                    2 => "DEGRADED",
                    _ => "PROBLEM",
                }));
                if model.row_count() != views.len() {
                    model.set_vec(views);
                } else {
                    for (i, v) in views.into_iter().enumerate() {
                        model.set_row_data(i, v);
                    }
                }
            }
        });
    }

    ui.show()?;
    hide_from_taskbar();
    Timer::single_shot(Duration::from_millis(300), hide_from_taskbar);
    if cfg.x.is_none() {
        ui.window().set_position(LogicalPosition::new(40.0, 40.0));
    }
    slint::run_event_loop_until_quit()
}
