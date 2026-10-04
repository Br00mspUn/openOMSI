//! In-game controller configuration. Edits are saved before installing mappings on the
//! existing controller; opening this UI never creates another hardware connection.
use crate::controllers::{DeviceCfg, Func};
use crate::game_lists::{ListKind, Move, HEADING};
use crate::App;

pub(crate) fn is_controller_list(kind: Option<&ListKind>) -> bool {
    matches!(kind, Some(ListKind::ControllerDevices | ListKind::Controller(_) | ListKind::ControllerButton(..) | ListKind::ControllerCapture(_)))
}

fn configurations(app: &App) -> Vec<DeviceCfg> {
    app.controllers.as_ref().map(|c| c.configuration()).unwrap_or_else(|| crate::controllers::read_cfg(&app.args.root))
}

fn index(devices: &[DeviceCfg], name: &str) -> Option<usize> {
    devices.iter().position(|d| d.name == name)
        .or_else(|| devices.iter().position(|d| crate::controllers::names_match(&d.name, name)))
}

fn event_names(app: &App, device: &DeviceCfg) -> Vec<(String, String)> {
    let names = crate::describe::names(&app.args.root, &app.settings.language);
    let mut events = names.events();
    for action in configurations(app).iter().flat_map(|d| d.buttons.iter().map(|b| b.0.clone()))
        .chain(device.buttons.iter().map(|b| b.0.clone()))
        .chain(crate::game_lists::keyboard_actions(app))
        .chain(app.player.as_ref().into_iter().flat_map(|p| p.vehicle.ty.program.trigger_names()))
        .chain(["kw_s_R_fest", "kw_s_1_fest", "kw_s_2_fest", "kw_s_3_fest", "kw_s_4_fest", "kw_s_5_fest", "kw_s_6_fest", "kw_s_7_fest", "kw_s_8_fest", "kw_s_9_fest", "kw_s_10_fest",
                "gear_up", "gear_down", "view_look_left", "view_look_right", "view_look_up", "view_look_down", "view_toggle_viewpoint", "view_driver", "view_outside", "view_passenger"].into_iter().map(str::to_string)) {
        if !action.is_empty() && !events.iter().any(|(a, _)| a.eq_ignore_ascii_case(&action)) {
            events.push((action.clone(), names.control(&action)));
        }
    }
    events.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then_with(|| a.0.cmp(&b.0)));
    events
}

fn row(name: &str, value: &str, desc: &str, action: String) -> (String, String) {
    (crate::game_lists::row(name, 'a', value, desc, None), action)
}

pub(crate) fn items(app: &App, kind: &ListKind) -> Vec<(String, String)> {
    let devices = configurations(app);
    let connected = app.controllers.as_ref().map(|c| c.connected()).unwrap_or_default();
    let mut out = Vec::new();
    match kind {
        ListKind::ControllerDevices => {
            out.push(crate::game_lists::opens("Keyboard", "Edit keyboard bindings", "keyboard"));
            out.push(crate::game_lists::button("Reload saved controllers", "Reload", "Apply gamectrler.cfg without reconnecting devices", "reload_controllers"));
            out.extend([
                crate::game_lists::switch_row(app, "ff", "Force feedback and vibration", "Enable steering forces and gamepad rumble"),
                crate::game_lists::switch_row(app, "ff_invert", "Invert force feedback by default", "For wheels without a saved direction"),
                crate::game_lists::slider_row(app, "ctrl_deadzone", "Dead zone", "Ignore movement around the centre or at pedal rest", &|v| format!("{:.0} %", v * 100.0)),
                crate::game_lists::slider_row(app, "wheel_range", "Wheel rotation", "Your wheel's rotation from lock to lock", &|v| format!("{v:.0}°")),
                crate::game_lists::slider_row(app, "wheel_lock", "Full lock at", "Rotation for the bus's full lock", &|v| if v < 45.0 { "OMSI".into() } else { format!("{v:.0}°") }),
                crate::game_lists::slider_row(app, "pedal_t", "Throttle pedal strength", "Pedal response", &|v| format!("x{v}")),
                crate::game_lists::slider_row(app, "pedal_b", "Brake pedal strength", "Pedal response", &|v| format!("x{v}")),
            ].into_iter().flatten());
            out.push(("Devices".into(), HEADING.into()));
            for d in &devices {
                let on = connected.iter().any(|c| crate::controllers::names_match(&d.name, &c.name));
                out.push(crate::game_lists::opens(&d.name, if on { "Connected" } else { "Disconnected: saved configuration can still be edited" }, &format!("controller {}", d.name)));
            }
            for c in &connected {
                if index(&devices, &c.name).is_none() {
                    out.push(crate::game_lists::opens(&format!("Set up {}", c.name), "Assign axes, pedals and buttons", &format!("controller {}", c.name)));
                }
            }
            if devices.is_empty() && connected.is_empty() {
                out.push(row("No controller connected", "", "Connect a wheel or gamepad", "noop".into()));
            }
        }
        ListKind::Controller(name) => {
            let d = index(&devices, name).map(|i| devices[i].clone()).unwrap_or_else(|| DeviceCfg { name: name.clone(), second: "0".into(), ..Default::default() });
            let live = connected.iter().find(|c| crate::controllers::names_match(&c.name, name));
            let disabled = app.settings.ctrl_off.split('|').any(|n| crate::controllers::names_match(n, name));
            out.push((crate::game_lists::row("Use this device", 's', if disabled { "off" } else { "on" }, "Enable axes and buttons", None), "device_on".into()));
            let ff = d.ff_scale.unwrap_or((1.0, 1.0));
            out.push(row("Steering force", &format!("{:.0} %", ff.0 * 100.0), "Left/right adjusts in steps of 5 % (0–200 %)", "force".into()));
            out.push(row("Vibration", &format!("{:.0} %", ff.1 * 100.0), "Left/right adjusts in steps of 5 % (0–200 %)", "vibration".into()));
            let invert = d.ff_invert.unwrap_or(app.settings.ff_invert);
            out.push((crate::game_lists::row("Invert force feedback", 's', if invert { "on" } else { "off" }, "Motor direction for this device", None), "force_invert".into()));
            const AXES: [&str; 8] = ["X axis", "Y axis", "Z axis", "X rotation", "Y rotation", "Z rotation", "Slider 1", "Slider 2"];
            out.push(("Axes".into(), HEADING.into()));
            for (a, label) in AXES.iter().enumerate() {
                let function = (Func::code(d.axes[a].map(|x| x.0)) + 1) as usize;
                let position = live.and_then(|c| c.axes.iter().find(|(k, _)| *k == a)).map(|(_, v)| format!(" · {v:+.2}")).unwrap_or_default();
                out.push(row(label, Func::LABELS[function], &format!("Left/right changes the function{position}"), format!("axis {a}")));
                if let Some((_, inv)) = d.axes[a] {
                    out.push((crate::game_lists::row("Reversed", 's', if inv { "on" } else { "off" }, "Reverse this axis", None), format!("reverse {a}")));
                    let curve = crate::controllers::AXIS_SHAPES.iter().find(|s| s.1 == d.axis_flags[a] & (4 | 8 | 0x10)).map(|s| s.0).unwrap_or("Linear");
                    out.push(row("Response curve", curve, "Left/right selects the characteristic", format!("curve {a}")));
                }
            }
            out.push(("Buttons".into(), HEADING.into()));
            out.push(crate::game_lists::opens("Assign a physical button…", "Press a button on this device, then choose its action", "capture_button"));
            let count = d.buttons.iter().rposition(|b| !b.0.is_empty()).map(|b| b + 1).unwrap_or(0)
                .max(live.map(|c| c.buttons).unwrap_or(0)).min(crate::controllers::HAT_BUTTONS + 16);
            let names = crate::describe::names(&app.args.root, &app.settings.language);
            for b in 0..count {
                let label = button_label(b);
                let action = d.buttons.get(b).map(|x| x.0.as_str()).unwrap_or("");
                let value = if action.is_empty() { "<none>".to_string() } else { names.control(action) };
                out.push(row(&label, &value, "Choose an OMSI event or game action", format!("button {b}")));
                out.push((crate::game_lists::row("Latching", 's', if d.latching.contains(&b) { "on" } else { "off" }, "Switch back when a physical switch is released", None), format!("latching {b}")));
            }
            out.push(crate::game_lists::opens("Back to devices", "All successful changes are saved immediately", "back"));
        }
        ListKind::ControllerCapture(name) => {
            out.push((format!("Press a button on {name} (Esc cancels)"), "noop".into()));
            out.push(("Cancel".into(), "back".into()));
        }
        ListKind::ControllerButton(name, b) => {
            let d = index(&devices, name).map(|i| devices[i].clone()).unwrap_or_default();
            out.push((format!("Clear {}", button_label(*b)), "bind ".into()));
            for (action, label) in event_names(app, &d) {
                out.push((format!("{label} · KY_{action}"), format!("bind {action}")));
            }
            out.push(("Back".into(), "back".into()));
        }
        _ => {}
    }
    out
}

fn button_label(b: usize) -> String {
    if b >= crate::controllers::HAT_BUTTONS {
        let h = b - crate::controllers::HAT_BUTTONS;
        format!("Hat {} {}", h / 4 + 1, ["up", "right", "down", "left"][h % 4])
    } else {
        format!("Button {}", b + 1)
    }
}

fn step(now: usize, count: usize, mv: Move) -> usize {
    match mv {
        Move::Next => (now + 1) % count,
        Move::Inc => (now + 1).min(count - 1),
        Move::Dec => now.saturating_sub(1),
        Move::To(f) => (f.clamp(0.0, 1.0) * (count - 1) as f32).round() as usize,
    }
}

fn switched(now: bool, mv: Move) -> bool {
    match mv { Move::Next => !now, Move::Inc => true, Move::Dec => false, Move::To(f) => f >= 0.5 }
}

fn save(app: &mut App, devices: Vec<DeviceCfg>) {
    match crate::controllers::save_cfg(&devices) {
        Ok(()) => {
            if let Some(c) = app.controllers.as_mut() { c.install_cfg(devices); }
            app.last_ctl_steer = None;
            app.service_msg = Some(("Controller configuration saved and applied".into(), 3.0));
        }
        Err(e) => app.service_msg = Some((format!("Controller configuration was not saved: {e}"), 6.0)),
    }
}

pub(crate) fn run(app: &mut App, kind: &ListKind, action: &str, mv: Move) -> Option<ListKind> {
    let (verb, arg) = action.split_once(' ').unwrap_or((action, ""));
    if action == "back" {
        return Some(match kind {
            ListKind::ControllerDevices => ListKind::Controls,
            ListKind::Controller(name) => { let _ = name; ListKind::ControllerDevices },
            ListKind::ControllerButton(name, _) | ListKind::ControllerCapture(name) => ListKind::Controller(name.clone()),
            _ => ListKind::Controls,
        });
    }
    if let ListKind::ControllerDevices = kind {
        if crate::game_lists::option_do(app, verb, arg, mv) { return Some(kind.clone()); }
        if !matches!(mv, Move::Next) { return Some(kind.clone()); }
        return Some(match verb {
            "keyboard" => ListKind::Controls,
            "controller" => ListKind::Controller(arg.to_string()),
            "reload_controllers" => {
                match crate::controllers::read_cfg_checked(&app.args.root) {
                    Ok(devices) => {
                        if let Some(c) = app.controllers.as_mut() { c.install_cfg(devices); }
                        app.last_ctl_steer = None;
                        app.service_msg = Some(("Saved controller mappings reloaded".into(), 3.0));
                    }
                    Err(e) => app.service_msg = Some((format!("Controller mappings were not reloaded: {e}"), 6.0)),
                }
                kind.clone()
            }
            _ => kind.clone(),
        });
    }
    let (name, button) = match kind {
        ListKind::Controller(name) => (name, None),
        ListKind::ControllerButton(name, b) => (name, Some(*b)),
        _ => return Some(kind.clone()),
    };
    if matches!(mv, Move::Next) {
        match verb {
            "capture_button" => return Some(ListKind::ControllerCapture(name.clone())),
            "button" => if let Ok(b) = arg.parse::<usize>() {
                if b < crate::controllers::HAT_BUTTONS + 16 { return Some(ListKind::ControllerButton(name.clone(), b)); }
            },
            _ => {}
        }
    }
    if verb == "device_on" {
        let mut off: Vec<String> = app.settings.ctrl_off.split('|').filter(|s| !s.is_empty()).map(str::to_string).collect();
        let on = !off.iter().any(|n| crate::controllers::names_match(n, name));
        off.retain(|n| !crate::controllers::names_match(n, name));
        if !switched(on, mv) { off.push(name.clone()); }
        app.settings.ctrl_off = off.join("|");
        crate::game_lists::remember_setting("ctrl_off", &app.settings.ctrl_off);
        return Some(kind.clone());
    }
    let mut devices = configurations(app);
    let i = index(&devices, name).unwrap_or_else(|| {
        devices.push(DeviceCfg { name: name.clone(), second: "0".into(), ..Default::default() });
        devices.len() - 1
    });
    let d = &mut devices[i];
    let axis = arg.parse::<usize>().ok().filter(|a| *a < 8);
    match verb {
        "axis" => if let Some(a) = axis {
            let current = (Func::code(d.axes[a].map(|x| x.0)) + 1) as usize;
            let to = step(current, Func::LABELS.len(), mv);
            let inv = d.axes[a].map(|x| x.1).unwrap_or(false);
            d.axes[a] = Func::from_code(to as i32 - 1).map(|f| (f, inv));
        } else { return Some(kind.clone()); },
        "reverse" => if let Some(a) = axis {
            if let Some((_, inv)) = &mut d.axes[a] { *inv = switched(*inv, mv); }
        } else { return Some(kind.clone()); },
        "curve" => if let Some(a) = axis {
            let curve = d.axis_flags[a] & (4 | 8 | 0x10);
            let current = crate::controllers::AXIS_SHAPES.iter().position(|s| s.1 == curve).unwrap_or(0);
            let to = step(current, crate::controllers::AXIS_SHAPES.len(), mv);
            d.axis_flags[a] = (d.axis_flags[a] & !(4 | 8 | 0x10)) | crate::controllers::AXIS_SHAPES[to].1;
        } else { return Some(kind.clone()); },
        "force" | "vibration" => {
            let mut ff = d.ff_scale.unwrap_or((1.0, 1.0));
            let v = if verb == "force" { &mut ff.0 } else { &mut ff.1 };
            *v = step((*v * 20.0).round().clamp(0.0, 40.0) as usize, 41, mv) as f32 / 20.0;
            d.ff_scale = Some(ff);
        }
        "force_invert" => d.ff_invert = Some(switched(d.ff_invert.unwrap_or(app.settings.ff_invert), mv)),
        "latching" => if let Some(b) = arg.parse::<usize>().ok().filter(|b| *b < crate::controllers::HAT_BUTTONS + 16) {
            let on = switched(d.latching.contains(&b), mv);
            d.latching.retain(|x| *x != b);
            if on { d.latching.push(b); d.latching.sort_unstable(); }
        } else { return Some(kind.clone()); },
        "bind" if matches!(mv, Move::Next) => {
            if let Some(b) = button {
                if b >= crate::controllers::HAT_BUTTONS + 16 { return Some(kind.clone()); }
                d.buttons.resize(d.buttons.len().max(b + 1), (String::new(), "0".into()));
                d.buttons[b].0 = arg.to_string();
                save(app, devices);
                return Some(ListKind::Controller(name.clone()));
            }
            return Some(kind.clone());
        }
        _ => return Some(kind.clone()),
    }
    save(app, devices);
    Some(kind.clone())
}

/// Called after the existing controller's single poll for this frame.
pub(crate) fn frame(app: &mut App) {
    if matches!(app.list_kind, Some(ListKind::ControllerDevices | ListKind::Controller(_))) {
        thread_local! {
            static LAST_REFRESH: std::cell::RefCell<std::time::Instant> = std::cell::RefCell::new(std::time::Instant::now());
        }
        let refresh = LAST_REFRESH.with(|last| {
            let mut last = last.borrow_mut();
            if last.elapsed().as_millis() < 250 { return false; }
            *last = std::time::Instant::now();
            true
        });
        if refresh { app.refresh_list(); }
        return;
    }
    let Some(ListKind::ControllerCapture(name)) = app.list_kind.clone() else { return };
    let pressed = app.controllers.as_ref().and_then(|c| c.raw_buttons.iter().find(|(n, b, down)|
        *down && *b < crate::controllers::HAT_BUTTONS + 16 && crate::controllers::names_match(n, &name)).map(|(_, b, _)| *b));
    if let Some(button) = pressed {
        app.open_list(ListKind::ControllerButton(name, button));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controller_menu_steps_clamp_and_wrap() {
        assert_eq!(step(0, 8, Move::Dec), 0);
        assert_eq!(step(7, 8, Move::Inc), 7);
        assert_eq!(step(7, 8, Move::Next), 0);
        assert_eq!(step(0, 41, Move::To(1.0)), 40);
        assert_eq!(button_label(0), "Button 1");
        assert_eq!(button_label(128), "Hat 1 up");
        assert!(is_controller_list(Some(&ListKind::ControllerCapture("Wheel".into()))));
        assert!(!is_controller_list(Some(&ListKind::Events)));
    }
}
