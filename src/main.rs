#![windows_subsystem = "windows"]
// Audio Widget — Rust/egui desktop volume controller. Borderless, semi-transparent, with settings.
// Audio backend = proven AudioSwitch.exe helper (Core Audio), state via %TEMP% file.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Color32, Pos2, Rect, Rounding, Sense, Stroke, Vec2};

const PALETTE: [(&str, Color32); 6] = [
    ("cyan", Color32::from_rgb(77, 208, 225)),
    ("blue", Color32::from_rgb(90, 160, 255)),
    ("green", Color32::from_rgb(80, 220, 140)),
    ("amber", Color32::from_rgb(255, 180, 70)),
    ("violet", Color32::from_rgb(170, 120, 255)),
    ("white", Color32::from_rgb(230, 230, 230)),
];
const SIZES: [[f32; 2]; 3] = [[300.0, 150.0], [340.0, 160.0], [400.0, 190.0]];
const SETTINGS_H: f32 = 268.0;

#[derive(Clone, Debug, Default)]
struct State { devices: Vec<String>, default: usize, vol: i32, mute: bool, ok: bool }

#[derive(Clone, Debug, PartialEq)]
struct Settings { accent: usize, opacity: u8, radius: u8, size: usize, ontop: bool, autostart: bool, dock: bool }
impl Default for Settings {
    fn default() -> Self { Self { accent: 0, opacity: 170, radius: 16, size: 1, ontop: true, autostart: false, dock: false } }
}

enum Cmd { List, Vol(i32), Mute, Set(usize) }

fn helper_exe() -> String { format!("{}\\AudioSwitch\\AudioSwitch.exe", std::env::var("LOCALAPPDATA").unwrap_or_default()) }
fn state_path() -> String { format!("{}\\audioswitch_out.txt", std::env::var("TEMP").unwrap_or_default()) }
fn cfg_path() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_default());
    p.push("AudioWidget"); p.push("settings.ini"); p
}

fn load_settings() -> Settings {
    let mut s = Settings::default();
    if let Ok(txt) = std::fs::read_to_string(cfg_path()) {
        for line in txt.lines() {
            let mut it = line.splitn(2, '=');
            let k = it.next().unwrap_or("").trim();
            let v = it.next().unwrap_or("").trim();
            match k {
                "accent" => s.accent = v.parse().unwrap_or(0),
                "opacity" => s.opacity = v.parse().unwrap_or(170),
                "radius" => s.radius = v.parse().unwrap_or(16),
                "size" => s.size = v.parse().unwrap_or(1),
                "ontop" => s.ontop = v == "1",
                "autostart" => s.autostart = v == "1",
                "dock" => s.dock = v == "1",
                _ => {}
            }
        }
    }
    s.accent = s.accent.min(PALETTE.len() - 1);
    s.size = s.size.min(SIZES.len() - 1);
    s
}
fn save_settings(s: &Settings) {
    let p = cfg_path();
    if let Some(dir) = p.parent() { let _ = std::fs::create_dir_all(dir); }
    let body = format!("accent={}\nopacity={}\nradius={}\nsize={}\nontop={}\nautostart={}\ndock={}\n",
        s.accent, s.opacity, s.radius, s.size, s.ontop as u8, s.autostart as u8, s.dock as u8);
    let _ = std::fs::write(p, body);
}
fn set_autostart(on: bool) {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let exe = std::env::current_exe().unwrap_or_default();
    let key = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    if on {
        let _ = std::process::Command::new("reg").args(["add", key, "/v", "AudioWidget", "/t", "REG_SZ", "/d", &exe.to_string_lossy(), "/f"]).creation_flags(NO_WINDOW).status();
    } else {
        let _ = std::process::Command::new("reg").args(["delete", key, "/v", "AudioWidget", "/f"]).creation_flags(NO_WINDOW).status();
    }
}

fn run_helper(args: &[&str]) {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new(helper_exe()).args(args).creation_flags(NO_WINDOW).status();
}
fn read_state() -> State {
    let bytes = match std::fs::read(state_path()) { Ok(b) => b, Err(_) => return State::default() };
    let mut s = String::new(); let mut i = 0;
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE { i = 2; }
    while i + 1 < bytes.len() {
        let u = u16::from_le_bytes([bytes[i], bytes[i + 1]]); i += 2;
        if u == 0 { continue; }
        s.push(char::from_u32(u as u32).unwrap_or(' '));
    }
    let mut st = State::default();
    for line in s.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("DEV|") {
            let p: Vec<&str> = rest.splitn(2, '|').collect();
            if p.len() == 2 { st.devices.push(p[1].to_string()); }
        } else if let Some(rest) = line.strip_prefix("DEFAULT|") { st.default = rest.trim().parse().unwrap_or(0); }
        else if let Some(rest) = line.strip_prefix("VOLUME|") {
            let p: Vec<&str> = rest.split('|').collect();
            if p.len() >= 2 { st.vol = p[0].trim().parse().unwrap_or(-1); st.mute = p[1].trim() == "1"; st.ok = st.vol >= 0; }
        }
    }
    st
}
fn worker(cmd_rx: Receiver<Cmd>, state_tx: Sender<State>) {
    run_helper(&["list"]); let _ = state_tx.send(read_state());
    let mut last = Instant::now();
    loop {
        match cmd_rx.recv_timeout(Duration::from_millis(600)) {
            Ok(cmd) => {
                match cmd { Cmd::List => run_helper(&["list"]), Cmd::Vol(v) => run_helper(&["vol", &v.to_string()]), Cmd::Mute => run_helper(&["mute"]), Cmd::Set(i) => run_helper(&["set", &i.to_string()]) }
                let _ = state_tx.send(read_state()); last = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {
                if last.elapsed() > Duration::from_millis(1200) { run_helper(&["list"]); let _ = state_tx.send(read_state()); last = Instant::now(); }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

// --- sidebar docking: find the 8GadgetPack gadget column and sit at its bottom ---
#[repr(C)] #[derive(Default, Clone, Copy)] struct WRECT { l: i32, t: i32, r: i32, b: i32 }
extern "system" {
    fn EnumWindows(cb: extern "system" fn(isize, isize) -> i32, lp: isize) -> i32;
    fn GetWindowRect(h: isize, r: *mut WRECT) -> i32;
    fn GetClassNameW(h: isize, buf: *mut u16, n: i32) -> i32;
    fn IsWindowVisible(h: isize) -> i32;
}
static mut DOCK: Option<(i32, i32)> = None;
extern "system" fn dock_cb(h: isize, _: isize) -> i32 {
    unsafe {
        if IsWindowVisible(h) == 0 { return 1; }
        let mut buf = [0u16; 64];
        let n = GetClassNameW(h, buf.as_mut_ptr(), 64);
        if n <= 0 { return 1; }
        let cls = String::from_utf16_lossy(&buf[..n as usize]);
        if cls != "BasicWindow" { return 1; }
        let mut r = WRECT::default();
        if GetWindowRect(h, &mut r) == 0 { return 1; }
        if (r.r - r.l) < 40 || (r.b - r.t) < 20 { return 1; }
        let cur = DOCK.unwrap_or((r.l, r.b));
        // keep the column of the LOWEST gadget, so we sit directly below that stack
        if r.b >= cur.1 { DOCK = Some((r.l, r.b)); } else { DOCK = Some(cur); }
        1
    }
}
fn dock_target() -> Option<(i32, i32)> {
    unsafe { DOCK = None; EnumWindows(dock_cb, 0); DOCK }
}

// --- system-tray (notification area, right side): icon only, no taskbar button ---
use std::sync::atomic::{AtomicU8, Ordering};
#[repr(C)]
struct NOTIFYICONDATAW { cbSize: u32, hWnd: isize, uID: u32, uFlags: u32, uCallbackMessage: u32, hIcon: isize, szTip: [u16; 128] }
#[repr(C)]
struct WNDCLASSW { style: u32, lpfnWndProc: extern "system" fn(isize, u32, usize, isize) -> isize, cbClsExtra: i32, cbWndExtra: i32, hInstance: isize, hIcon: isize, hCursor: isize, hbrBackground: isize, lpszMenuName: *const u16, lpszClassName: *const u16 }
#[link(name = "shell32")]
extern "system" { fn Shell_NotifyIconW(msg: u32, data: *mut NOTIFYICONDATAW) -> i32; }
#[link(name = "user32")]
extern "system" {
    fn LoadImageW(h: isize, name: *const u16, typ: u32, cx: i32, cy: i32, flags: u32) -> isize;
    fn RegisterClassW(wc: *const WNDCLASSW) -> u16;
    fn CreateWindowExW(ex: u32, cls: *const u16, name: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: isize, menu: isize, hinst: isize, param: *mut std::ffi::c_void) -> isize;
    fn DefWindowProcW(h: isize, msg: u32, w: usize, l: isize) -> isize;
    fn DestroyIcon(h: isize) -> i32;
    fn GetModuleHandleW(name: *const u16) -> isize;
}
static TRAY_CMD: AtomicU8 = AtomicU8::new(0); // 1=toggle show, 2=exit
static mut TRAY_HWND: isize = 0;
static mut TRAY_ICON: isize = 0;
const WM_TRAY: u32 = 0x0400 + 1;
extern "system" fn tray_wndproc(h: isize, msg: u32, w: usize, l: isize) -> isize {
    if msg == WM_TRAY {
        let ev = (l as u32) & 0xffff;
        if ev == 0x0203 { TRAY_CMD.store(1, Ordering::SeqCst); }        // left click: show/hide
        else if ev == 0x0205 { TRAY_CMD.store(2, Ordering::SeqCst); }   // right click: exit
        return 0;
    }
    unsafe { DefWindowProcW(h, msg, w, l) }
}
fn tray_init() {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let cls: Vec<u16> = "AWTrayCls".encode_utf16().chain(Some(0)).collect();
        let wc = WNDCLASSW { style: 0, lpfnWndProc: tray_wndproc, cbClsExtra: 0, cbWndExtra: 0, hInstance: hinst, hIcon: 0, hCursor: 0, hbrBackground: 0, lpszMenuName: std::ptr::null(), lpszClassName: cls.as_ptr() };
        RegisterClassW(&wc);
        TRAY_HWND = CreateWindowExW(0, cls.as_ptr(), cls.as_ptr(), 0, 0, 0, 0, 0, -3, 0, hinst, std::ptr::null_mut());
        let exe = std::env::current_exe().unwrap_or_default();
        let ico = exe.parent().map(|p| p.join("widget.ico")).unwrap_or_default();
        let wide: Vec<u16> = ico.to_string_lossy().encode_utf16().chain(Some(0)).collect();
        TRAY_ICON = LoadImageW(0, wide.as_ptr(), 1, 0, 0, 0x10);
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = TRAY_HWND; nid.uID = 1;
        nid.uFlags = 0x1 | 0x2 | 0x4;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = TRAY_ICON;
        let tip: Vec<u16> = "Audio Widget".encode_utf16().chain(Some(0)).collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(0, &mut nid);
    }
}
fn tray_remove() {
    unsafe {
        if TRAY_HWND != 0 {
            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = TRAY_HWND; nid.uID = 1;
            Shell_NotifyIconW(2, &mut nid);
        }
        if TRAY_ICON != 0 { DestroyIcon(TRAY_ICON); TRAY_ICON = 0; }
    }
}

struct App {
    state: State, cmd_tx: Sender<Cmd>, state_rx: Receiver<State>,
    wheel_acc: f32, show_devices: bool, show_settings: bool,
    set: Settings, applied: Option<(usize, bool, bool)>,
    dock_pos: Option<(i32, i32)>, dock_t: Instant,
    tray_show: bool, hide_cnt: u32,
    hold: Option<Instant>, last_sent: i32,
}
impl App {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (cmd_tx, cmd_rx) = channel(); let (state_tx, state_rx) = channel();
        std::thread::spawn(move || worker(cmd_rx, state_tx));
        tray_init();
        Self { state: State::default(), cmd_tx, state_rx, wheel_acc: 0.0, show_devices: false, show_settings: std::env::var("AW_SETTINGS").is_ok(), set: load_settings(), applied: None, dock_pos: None, dock_t: Instant::now(), tray_show: false, hide_cnt: 0, hold: None, last_sent: -1 }
    }
    fn send(&self, c: Cmd) { let _ = self.cmd_tx.send(c); }
    fn set_vol(&mut self, v: i32) {
        let v = v.clamp(0, 100);
        self.state.vol = v;
        if v == self.last_sent { return; }
        self.last_sent = v;
        self.hold = Some(Instant::now());
        self.send(Cmd::Vol(v));
    }
    fn win_h(&self) -> f32 { SIZES[self.set.size][1] + if self.show_settings { SETTINGS_H } else { 0.0 } }
}

fn paint_speaker(p: &egui::Painter, r: Rect, c: Color32, muted: bool) {
    let (w, h, x, y) = (r.width(), r.height(), r.left(), r.top());
    p.rect_filled(Rect::from_min_size(Pos2::new(x + w * 0.10, y + h * 0.34), Vec2::new(w * 0.18, h * 0.32)), Rounding::same(2), c);
    p.add(egui::Shape::convex_polygon(vec![
        Pos2::new(x + w * 0.28, y + h * 0.34), Pos2::new(x + w * 0.50, y + h * 0.16),
        Pos2::new(x + w * 0.50, y + h * 0.84), Pos2::new(x + w * 0.28, y + h * 0.66)], c, Stroke::NONE));
    if muted {
        p.line_segment([Pos2::new(x + w * 0.60, y + h * 0.30), Pos2::new(x + w * 0.90, y + h * 0.70)], Stroke::new(2.5, c));
        p.line_segment([Pos2::new(x + w * 0.90, y + h * 0.30), Pos2::new(x + w * 0.60, y + h * 0.70)], Stroke::new(2.5, c));
    } else {
        for rad in [0.16f32, 0.28] {
            let (cx, cy, rr) = (x + w * 0.56, y + h * 0.5, h * rad);
            let pts: Vec<Pos2> = (0..=12).map(|k| { let a = -0.9 + 1.8 * (k as f32 / 12.0); Pos2::new(cx + rr * a.cos() * 1.4, cy + rr * a.sin()) }).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.0, c)));
        }
    }
}
fn paint_list(p: &egui::Painter, r: Rect, c: Color32) {
    for i in 0..3 {
        let y = r.top() + r.height() * (0.25 + 0.25 * i as f32);
        p.circle_filled(Pos2::new(r.left() + r.width() * 0.18, y), 2.0, c);
        p.line_segment([Pos2::new(r.left() + r.width() * 0.34, y), Pos2::new(r.right() - r.width() * 0.12, y)], Stroke::new(2.0, c));
    }
}
fn paint_min(p: &egui::Painter, r: Rect, c: Color32) {
    let y = r.center().y + r.height() * 0.18;
    p.line_segment([Pos2::new(r.left() + r.width() * 0.26, y), Pos2::new(r.right() - r.width() * 0.26, y)], Stroke::new(2.0, c));
}
fn paint_x(p: &egui::Painter, r: Rect, c: Color32) {
    let m = r.width() * 0.28;
    p.line_segment([Pos2::new(r.left() + m, r.top() + m), Pos2::new(r.right() - m, r.bottom() - m)], Stroke::new(2.0, c));
    p.line_segment([Pos2::new(r.right() - m, r.top() + m), Pos2::new(r.left() + m, r.bottom() - m)], Stroke::new(2.0, c));
}
fn paint_gear(p: &egui::Painter, r: Rect, c: Color32) {
    let ctr = r.center(); let ro = r.width() * 0.36; let ri = r.width() * 0.16;
    for k in 0..8 {
        let a = std::f32::consts::TAU * (k as f32 / 8.0);
        let d = Vec2::new(a.cos(), a.sin());
        p.line_segment([ctr + d * (ri + 1.0), ctr + d * ro], Stroke::new(3.0, c));
    }
    p.circle_stroke(ctr, ri + 1.0, Stroke::new(2.5, c));
}
fn icon_button(ui: &mut egui::Ui, rect: Rect, id: &str, draw: impl FnOnce(&egui::Painter, Rect, Color32)) -> egui::Response {
    let resp = ui.interact(rect, egui::Id::new(id), Sense::click());
    let hov = resp.hovered();
    ui.painter().rect_filled(rect, Rounding::same(8), if hov { Color32::from_white_alpha(30) } else { Color32::from_white_alpha(12) });
    draw(ui.painter(), rect.shrink(5.0), if hov { Color32::WHITE } else { Color32::from_gray(225) });
    resp
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(180));
        match TRAY_CMD.swap(0, Ordering::SeqCst) {
            1 => { self.tray_show = !self.tray_show; ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(!self.tray_show)); if self.tray_show { ctx.send_viewport_cmd(egui::ViewportCommand::Focus); } }
            2 => { tray_remove(); ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
            _ => {}
        }
        self.hide_cnt += 1;
        if self.hide_cnt == 3 && std::env::var("AW_SHOW").is_err() { ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)); }
        while let Ok(s) = self.state_rx.try_recv() {
            // while the user is adjusting, keep our local vol/mute so stale helper reads don't yank the slider
            let guarding = match self.hold { Some(t) => t.elapsed() < Duration::from_millis(900), None => false };
            if !guarding { self.hold = None; }
            let (kv, km) = (self.state.vol, self.state.mute);
            self.state = s;
            if guarding { self.state.vol = kv; self.state.mute = km; }
        }
        ctx.style_mut(|s| { s.visuals.panel_fill = Color32::TRANSPARENT; s.visuals.window_fill = Color32::TRANSPARENT; s.visuals.extreme_bg_color = Color32::TRANSPARENT; });

        // apply viewport size / ontop when changed
        let want = (self.set.size, self.set.ontop, self.show_settings);
        if self.applied != Some(want) {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(SIZES[self.set.size][0], self.win_h())));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.set.ontop { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal }));
            self.applied = Some(want);
        }

        // dock into the open 8GadgetPack sidebar column (bottom of the gadget stack)
        if self.set.dock {
            if self.dock_t.elapsed() > Duration::from_millis(700) {
                self.dock_t = Instant::now();
                if let Some((x, y)) = dock_target() {
                    let target = (x, y + 6);
                    if self.dock_pos != Some(target) {
                        self.dock_pos = Some(target);
                        let ppp = ctx.pixels_per_point();
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(target.0 as f32 / ppp, target.1 as f32 / ppp)));
                    }
                }
            }
        } else { self.dock_pos = None; }

        let dy = ctx.input(|i| i.smooth_scroll_delta.y);
        self.wheel_acc += dy;
        while self.wheel_acc >= 40.0 { self.wheel_acc -= 40.0; let v = (self.state.vol.max(0) + 3).min(100); self.set_vol(v); }
        while self.wheel_acc <= -40.0 { self.wheel_acc += 40.0; let v = (self.state.vol.max(0) - 3).max(0); self.set_vol(v); }

        let before = self.set.clone();
        let (w, h) = (SIZES[self.set.size][0], self.win_h());
        let accent = PALETTE[self.set.accent].1;
        let radius = self.set.radius;
        let alpha = self.set.opacity;

        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::TRANSPARENT)).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            ui.painter().rect_filled(screen, Rounding::ZERO, Color32::from_rgba_unmultiplied(0, 0, 0, 1));
            let p = ui.painter().clone();
            // borderless semi-transparent card filling the whole window (no margin, no border, no shadow)
            let card = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h));
            p.rect_filled(card, Rounding::same(radius), Color32::from_rgba_unmultiplied(18, 22, 34, alpha));

            let txt = Color32::from_gray(240); let dim = Color32::from_gray(160);
            let base_h = SIZES[self.set.size][1];

            let spk = Rect::from_min_size(Pos2::new(14.0, 12.0), Vec2::new(24.0, 24.0));
            paint_speaker(&p, spk, if self.state.mute { Color32::from_rgb(255, 107, 107) } else { accent }, self.state.mute);
            let name = if self.state.default > 0 && self.state.default <= self.state.devices.len() { self.state.devices[self.state.default - 1].clone() } else { "No device".to_string() };
            let maxc = ((w - 120.0) / 8.0) as usize;
            let name_show: String = if name.chars().count() > maxc { name.chars().take(maxc.saturating_sub(1)).collect::<String>() + "…" } else { name.clone() };
            p.text(Pos2::new(46.0, 14.0), egui::Align2::LEFT_TOP, name_show, egui::FontId::proportional(15.0), txt);
            let sub = if self.state.ok { format!("output {} / {}", self.state.default, self.state.devices.len()) } else { "connecting…".to_string() };
            p.text(Pos2::new(46.0, 34.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(10.0), dim);

            let drag = ui.interact(Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(w - 110.0, 50.0)), egui::Id::new("drag"), Sense::drag());
            if drag.dragged() { ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag); }

            let min_r = Rect::from_min_size(Pos2::new(w - 82.0, 12.0), Vec2::new(20.0, 20.0));
            if icon_button(ui, min_r, "min", |pp, r, c| paint_min(pp, r, c)).clicked() { self.tray_show = false; ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)); }
            let gear_r = Rect::from_min_size(Pos2::new(w - 56.0, 12.0), Vec2::new(20.0, 20.0));
            if icon_button(ui, gear_r, "gear", |pp, r, c| paint_gear(pp, r, c)).clicked() { self.show_settings = !self.show_settings; }
            let close_r = Rect::from_min_size(Pos2::new(w - 30.0, 12.0), Vec2::new(20.0, 20.0));
            if icon_button(ui, close_r, "close", |pp, r, c| paint_x(pp, r, c)).clicked() { tray_remove(); ctx.send_viewport_cmd(egui::ViewportCommand::Close); }

            let track = Rect::from_min_size(Pos2::new(16.0, base_h * 0.45), Vec2::new(w - 76.0, 10.0));
            let vol = self.state.vol.max(0) as f32;
            p.rect_filled(track, Rounding::same(5), Color32::from_white_alpha(30));
            let fw = track.width() * (vol / 100.0);
            if fw > 0.5 { p.rect_filled(Rect::from_min_size(track.min, Vec2::new(fw, track.height())), Rounding::same(5), accent); }
            let knob = Pos2::new(track.left() + fw, track.center().y);
            p.circle_filled(knob, 7.0, Color32::WHITE); p.circle_stroke(knob, 7.0, Stroke::new(1.5, accent));
            let sl = ui.interact(track.expand(6.0), egui::Id::new("slider"), Sense::click_and_drag());
            if sl.dragged() || sl.clicked() {
                if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                    let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
                    self.set_vol((t * 100.0).round() as i32);
                }
            }
            let pct = if self.state.vol >= 0 { format!("{}%", self.state.vol) } else { "--".to_string() };
            p.text(Pos2::new(track.right() + 12.0, track.center().y - 8.0), egui::Align2::LEFT_TOP, pct, egui::FontId::proportional(14.0), txt);

            let by = base_h - 40.0;
            let mr = Rect::from_min_size(Pos2::new(16.0, by), Vec2::new(28.0, 28.0));
            if icon_button(ui, mr, "mute", |pp, r, c| paint_speaker(pp, r, c, true)).clicked() { self.state.mute = !self.state.mute; self.hold = Some(Instant::now()); self.send(Cmd::Mute); }
            let cr = Rect::from_min_size(Pos2::new(52.0, by), Vec2::new(28.0, 28.0));
            if icon_button(ui, cr, "cycle", |pp, r, c| paint_speaker(pp, r, c, false)).clicked() && !self.state.devices.is_empty() {
                let n = self.state.default % self.state.devices.len() + 1; self.send(Cmd::Set(n));
            }
            let lr = Rect::from_min_size(Pos2::new(88.0, by), Vec2::new(28.0, 28.0));
            if icon_button(ui, lr, "list", |pp, r, c| paint_list(pp, r, c)).clicked() { self.show_devices = !self.show_devices; }
            p.text(Pos2::new(128.0, by + 8.0), egui::Align2::LEFT_TOP, "wheel / drag = volume", egui::FontId::proportional(10.0), dim);

            if self.show_devices {
                let pos = Pos2::new(88.0, by + 32.0);
                egui::Area::new(egui::Id::new("devpick")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
                    egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(16, 20, 32, 250)).rounding(Rounding::same(10)).stroke(Stroke::new(1.0, Color32::from_white_alpha(50))).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                        ui.set_min_width(200.0);
                        for (i, d) in self.state.devices.iter().enumerate() {
                            let on = i + 1 == self.state.default;
                            let label = if d.chars().count() > 28 { d.chars().take(27).collect::<String>() + "…" } else { d.clone() };
                            if ui.add(egui::Button::new(egui::RichText::new(label).color(if on { accent } else { txt })).fill(if on { Color32::from_white_alpha(20) } else { Color32::TRANSPARENT })).clicked() {
                                self.send(Cmd::Set(i + 1)); self.show_devices = false;
                            }
                        }
                        if self.state.devices.is_empty() { ui.label(egui::RichText::new("no devices").color(dim)); }
                    });
                });
                if ctx.input(|i| i.pointer.any_click()) {
                    if let Some(hp) = ctx.input(|i| i.pointer.interact_pos()) {
                        let pr = Rect::from_min_size(pos, Vec2::new(212.0, 30.0 * self.state.devices.len().max(1) as f32 + 12.0));
                        if !pr.contains(hp) && !lr.contains(hp) { self.show_devices = false; }
                    }
                }
            }

            if self.show_settings {
                let sr = Rect::from_min_size(Pos2::new(10.0, base_h + 4.0), Vec2::new(w - 20.0, SETTINGS_H - 10.0));
                p.rect_filled(sr, Rounding::same(10), Color32::from_white_alpha(10));
                ui.allocate_ui_at_rect(sr.shrink(10.0), |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                    ui.label(egui::RichText::new("SETTINGS").color(dim).size(11.0));
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Accent").color(txt).size(12.0));
                        for (i, (_, c)) in PALETTE.iter().enumerate() {
                            let b = egui::Button::new("").fill(*c).rounding(Rounding::same(5));
                            let r = ui.add_sized([20.0, 20.0], b);
                            if i == self.set.accent { ui.painter().rect_stroke(r.rect.expand(2.0), Rounding::same(6), Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside); }
                            if r.clicked() { self.set.accent = i; }
                        }
                    });
                    ui.add(egui::Slider::new(&mut self.set.opacity, 40..=255).text("Opacity"));
                    ui.add(egui::Slider::new(&mut self.set.radius, 0..=28).text("Corner"));
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Size").color(txt).size(12.0));
                        for (i, n) in ["S", "M", "L"].iter().enumerate() {
                            if ui.add(egui::Button::new(egui::RichText::new(*n).color(if i == self.set.size { accent } else { txt })).fill(if i == self.set.size { Color32::from_white_alpha(25) } else { Color32::from_white_alpha(8) })).clicked() { self.set.size = i; }
                        }
                    });
                    ui.checkbox(&mut self.set.ontop, "Always on top");
                    ui.checkbox(&mut self.set.autostart, "Start with Windows");
                    ui.checkbox(&mut self.set.dock, "Dock to sidebar (8GadgetPack)");
                });
            }
        });

        if self.set != before {
            if self.set.autostart != before.autostart { set_autostart(self.set.autostart); }
            save_settings(&self.set);
            self.applied = None;
        }
    }
}

fn main() -> eframe::Result<()> {
    let s = load_settings();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([SIZES[s.size][0], SIZES[s.size][1]])
            .with_decorations(false).with_transparent(true).with_always_on_top().with_resizable(false).with_visible(true).with_taskbar(false),
        renderer: eframe::Renderer::Glow, vsync: true, ..Default::default()
    };
    eframe::run_native("Audio Widget", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
