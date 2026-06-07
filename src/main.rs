#![allow(static_mut_refs)]

use std::cell::{RefCell, UnsafeCell};
use std::io::Write;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use std::{env, mem};

use clap::{Args, Command, Parser, Subcommand, command};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use winapi::shared::basetsd::UINT_PTR;
use winapi::shared::minwindef::{HINSTANCE, LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HBRUSH, HWND, POINT, RECT, SIZE};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::{CreateSolidBrush, DeleteObject, RGB};
use winapi::um::winuser::*;

const TIMER_ID: UINT_PTR = 1;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct PointAndDt {
    x: i32,
    y: i32,
    dt: i32,
}

#[derive(Debug, Clone, Copy)]
struct RectWH {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl RectWH {
    fn contains(&self, point: Point) -> bool {
        (self.x..(self.x + self.w)).contains(&point.x)
            && (self.y..(self.y + self.h)).contains(&point.y)
    }
}

struct State {
    lastCursorPos: Point,
    lastTime: Option<Instant>,
    boxPos: RectWH,
    screenSize: SIZE,
    wHandle: Option<HWND>,
    recordedMovement: Vec<(Point, Duration)>,
    needMovement: Option<(Point, Point)>,
    currMovementTrail: Vec<PointAndDt>,
    curIdx: usize,
    needClick: bool,
    needUp: bool,
}

static mut state: State = State {
    lastCursorPos: Point { x: 0, y: 0 },
    lastTime: None,
    boxPos: RectWH {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    },
    screenSize: SIZE { cx: 0, cy: 0 },
    wHandle: None,
    recordedMovement: Vec::new(),
    needMovement: None,
    currMovementTrail: Vec::new(),
    curIdx: 0,
    needClick: false,
    needUp: false,
};

fn randBox() -> RectWH {
    unsafe {
        let mut trng = rand::rng();
        let w = state.boxPos.w;
        let h = state.boxPos.h;
        loop {
            let newx = trng.random_range(0i32..(state.screenSize.cx - w));
            let newy = trng.random_range(0i32..(state.screenSize.cy - h));
            if state.boxPos.contains(Point { x: newx, y: newy }) {
                continue;
            }
            break RectWH {
                x: newx,
                y: newy,
                w,
                h,
            };
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                if state.needMovement.is_none() {
                    let mut cursor_pos: POINT = mem::zeroed();
                    GetCursorPos(&mut cursor_pos);
                    state.lastTime = Some(Instant::now());
                    state.lastCursorPos = Point {
                        x: cursor_pos.x,
                        y: cursor_pos.y,
                    };
                    SetTimer(hwnd, TIMER_ID, 10, None);
                } else {
                    SetTimer(hwnd, TIMER_ID, 1, None);
                }
                0
            }
            WM_TIMER => {
                let timer_id = wparam as UINT_PTR;
                if timer_id == TIMER_ID {
                    if state.needMovement.is_none() {
                        let mut cursor_pos: POINT = mem::zeroed();
                        let now = Instant::now();
                        GetCursorPos(&mut cursor_pos);
                        let cursor_pos = Point {
                            x: cursor_pos.x,
                            y: cursor_pos.y,
                        };
                        let last_cursor_pos = state.lastCursorPos;
                        let delta_t = now - state.lastTime.unwrap();
                        let b = state.boxPos;
                        if cursor_pos != last_cursor_pos {
                            println!(
                                "{} {} (+{}ms)",
                                cursor_pos.x,
                                cursor_pos.y,
                                delta_t.as_millis()
                            );
                            state.lastCursorPos = cursor_pos;
                            state.lastTime = Some(now);
                            state.recordedMovement.push((cursor_pos, delta_t));
                        } else if b.contains(cursor_pos) && delta_t.as_millis() > 200 {
                            state.boxPos = randBox();
                            storeRecordedMovement();
                            state.recordedMovement.clear();
                            println!("New box");
                            let b = state.boxPos;
                            MoveWindow(state.wHandle.unwrap(), b.x, b.y, b.w, b.h, 1);
                        }
                    } else {
                        KillTimer(hwnd, TIMER_ID);
                        if state.needUp {
                            let mut input = INPUT {
                                type_: INPUT_MOUSE,
                                u: mem::zeroed(),
                            };
                            input.u.mi_mut().dwFlags = MOUSEEVENTF_LEFTUP;
                            SendInput(1, &mut input, mem::size_of_val(&input) as _);
                            DestroyWindow(hwnd);
                            return 0;
                        }
                        let this_movement = state.currMovementTrail.get(state.curIdx);
                        if this_movement.is_none() {
                            DestroyWindow(hwnd);
                            return 0;
                        }
                        let this_movement = this_movement.unwrap();
                        SetCursorPos(this_movement.x, this_movement.y);
                        boxTrackPos(Point {
                            x: this_movement.x,
                            y: this_movement.y,
                        });
                        let b = state.boxPos;
                        MoveWindow(state.wHandle.unwrap(), b.x, b.y, b.w, b.h, 1);
                        let next_movement = state.currMovementTrail.get(state.curIdx + 1);
                        if next_movement.is_none() {
                            if state.needClick {
                                let mut input = INPUT {
                                    type_: INPUT_MOUSE,
                                    u: mem::zeroed(),
                                };
                                input.u.mi_mut().dwFlags = MOUSEEVENTF_LEFTDOWN;
                                SendInput(1, &mut input, mem::size_of_val(&input) as _);
                                state.needUp = true;
                                SetTimer(
                                    hwnd,
                                    TIMER_ID,
                                    rand::rng().random_range(100i32..400i32) as _,
                                    None,
                                );
                            } else {
                                DestroyWindow(hwnd);
                            }
                            return 0;
                        }
                        state.curIdx += 1;
                        SetTimer(hwnd, TIMER_ID, next_movement.unwrap().dt as u32, None);
                    }
                }
                0
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rc);
                let brush: HBRUSH = CreateSolidBrush(RGB(255, 0, 0));
                FillRect(hdc, &rc, brush);
                DeleteObject(brush as _);
                EndPaint(hwnd, &ps);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn to_wstring(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn movementsDir() -> PathBuf {
    std::env::current_dir().unwrap().join("movements")
}

fn storeRecordedMovement() {
    unsafe {
        let movements = &state.recordedMovement[..];
        if movements.len() < 2 {
            return;
        }
        let mut out = Vec::new();
        for (p, dt) in movements {
            out.push(PointAndDt {
                x: p.x,
                y: p.y,
                dt: dt.as_millis() as i32,
            });
        }
        let dir = movementsDir();
        if !std::fs::exists(&dir).unwrap() {
            std::fs::create_dir(&dir).unwrap();
        }
        let first = movements.first().unwrap();
        let last = movements.last().unwrap();
        let file = format!("{}-{}-{}-{}.json", first.0.x, first.0.y, last.0.x, last.0.y);
        let fpath = dir.join(file);
        println!("Writing to {:?}", fpath);
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(fpath)
            .unwrap();
        serde_json::to_writer_pretty(&mut f, &out).unwrap();
        f.flush().unwrap();
    }
}

fn findBestTrail(from: Point, to: Point) {
    let dx = (from.x - to.x).abs();
    let dy = (from.y - to.y).abs();
    let dir = movementsDir();
    let mut best: Option<(Point, Point, String)> = None;
    for n in dir.read_dir().unwrap() {
        let filename = n.unwrap().file_name().to_string_lossy().into_owned();
        if let Some(n) = filename.strip_suffix(".json")
            && let parts = n.split("-").collect::<Vec<_>>()
            && parts.len() == 4
            && parts.iter().all(|x| i32::from_str(x).is_ok())
        {
            let parsed = parts
                .into_iter()
                .map(|x| i32::from_str(x).unwrap())
                .collect::<Vec<_>>();
            let thisfrom = Point {
                x: parsed[0],
                y: parsed[1],
            };
            let thisto = Point {
                x: parsed[2],
                y: parsed[3],
            };
            if best.is_none() {
                best = Some((thisfrom, thisto, filename));
            } else {
                let b = best.as_ref().unwrap();
                let bdx = (b.0.x - b.1.x).abs();
                let bdy = (b.0.y - b.1.y).abs();
                let tdx = (thisfrom.x - thisto.x).abs();
                let tdy = (thisfrom.y - thisto.y).abs();
                let bthres = (bdx - dx).pow(2) + (bdy - dy).pow(2);
                let tthres = (tdx - dx).pow(2) + (tdy - dy).pow(2);
                if tthres < bthres {
                    best = Some((thisfrom, thisto, filename));
                }
            }
        }
    }
    if best.is_none() {
        eprintln!("No movements");
        std::process::exit(1);
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .open(dir.join(&best.unwrap().2))
        .unwrap();
    let mut trail: Vec<PointAndDt> = serde_json::from_reader(f).unwrap();
    let read_from = *trail.first().unwrap();
    let read_to = *trail.last().unwrap();
    let xoff = from.x - read_from.x;
    let yoff = from.y - read_from.y;
    let xscale = (to.x - from.x) as f64 / (read_to.x - read_from.x) as f64;
    let yscale = (to.y - from.y) as f64 / (read_to.y - read_from.y) as f64;
    for t in trail.iter_mut() {
        t.x = read_from.x + xoff + ((t.x - read_from.x) as f64 * xscale) as i32;
        t.y = read_from.y + yoff + ((t.y - read_from.y) as f64 * yscale) as i32;
    }
    unsafe {
        state.currMovementTrail = trail;
    }
}

#[derive(Parser, Debug)]
#[command()]
struct Cmd {
    #[command(subcommand)]
    command: Subcmd,
}

#[derive(Debug, Subcommand)]
enum Subcmd {
    #[command(subcommand = "moveto")]
    MoveTo {
        #[arg()]
        x: i32,
        #[arg()]
        y: i32,
        #[arg(short = 'r', long)]
        relative_to_win: bool,
        #[arg(short = 'c', long)]
        need_click: bool,
    },
    #[command(subcommand = "train")]
    Train,
}

fn boxTrackPos(point: Point) {
    unsafe {
        state.boxPos.x = point.x - state.boxPos.w / 2;
        state.boxPos.y = point.y - state.boxPos.h / 2;
    }
}

fn main() {
    let args = Cmd::parse();
    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(null_mut());
        let class_name = to_wstring("TransparentRedRect");
        let screen_width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
        let screen_height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null_mut(),
            lpszClassName: class_name.as_ptr(),
        };

        RegisterClassW(&wc);

        state.screenSize.cx = screen_width;
        state.screenSize.cy = screen_height;
        match args.command {
            Subcmd::Train => {
                state.boxPos.w = 200;
                state.boxPos.h = 70;
                state.boxPos = randBox();
            }
            Subcmd::MoveTo {
                x,
                y,
                relative_to_win,
                need_click,
            } => {
                state.boxPos.w = 20;
                state.boxPos.h = 20;
                let mut from_pos: POINT = mem::zeroed();
                GetCursorPos(&mut from_pos);
                let from_pos = Point {
                    x: from_pos.x,
                    y: from_pos.y,
                };
                let mut to_pos = Point { x, y };
                if relative_to_win {
                    let fg = unsafe { GetForegroundWindow() };
                    if fg.is_null() {
                        eprintln!("No foreground window");
                        std::process::exit(1);
                    }
                    let mut rect: RECT = unsafe { std::mem::zeroed() };
                    if unsafe { GetWindowRect(fg, &mut rect) } == 0 {
                        eprintln!("Failed to GetWindowRect");
                        std::process::exit(1);
                    }
                    to_pos.x += rect.left;
                    to_pos.y += rect.top;
                }
                state.needMovement = Some((from_pos, to_pos));
                boxTrackPos(from_pos);
                println!(
                    "Moving from {}, {} to {}, {}",
                    from_pos.x, from_pos.y, to_pos.x, to_pos.y
                );
                findBestTrail(from_pos, to_pos);
                state.needClick = need_click;
            }
        }

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
            class_name.as_ptr(),
            to_wstring("RedRect").as_ptr(),
            WS_POPUP | WS_VISIBLE,
            state.boxPos.x,
            state.boxPos.y,
            state.boxPos.w,
            state.boxPos.h,
            null_mut(),
            null_mut(),
            hinstance,
            null_mut(),
        );

        if hwnd.is_null() {
            eprintln!("Failed to create window");
            std::process::exit(1);
        }

        state.wHandle = Some(hwnd);

        // Set 50% transparency (128 out of 255)
        SetLayeredWindowAttributes(hwnd, 0, 128, LWA_ALPHA);

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
