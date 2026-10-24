//! Roteiro de testes de interface (somente desenvolvimento).
//!
//! Com `MM_SCRIPT=arquivo`, injeta eventos sintéticos de mouse e teclado na entrada do egui.
//! Uma instrução por linha:
//!
//! ```text
//! wait 500            # espera em ms
//! move 400 300        # move o ponteiro (pontos lógicos)
//! click 400 300       # também: dclick, rclick, shiftclick
//! key ctrl+End        # modificadores: ctrl, shift, alt
//! type texto livre
//! scroll -300         # rolagem vertical
//! drag 100 50 200 50  # arrasta de (x1,y1) até (x2,y2)
//! shot                # screenshot (requer MM_SHOT_DIR)
//! quit
//! ```

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Vec2};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

enum Batch {
    Events(Vec<Event>),
    Wait(Duration),
}

pub struct Script {
    queue: VecDeque<Batch>,
    resume_at: Instant,
    pub quit: bool,
}

fn parse_mods(spec: &str) -> (Modifiers, String) {
    let mut mods = Modifiers::NONE;
    let parts: Vec<&str> = spec.split('+').collect();
    for p in &parts[..parts.len() - 1] {
        match p.to_lowercase().as_str() {
            "ctrl" => {
                mods.ctrl = true;
                mods.command = true;
            }
            "shift" => mods.shift = true,
            "alt" => mods.alt = true,
            _ => {}
        }
    }
    (mods, parts[parts.len() - 1].to_string())
}

fn button(pos: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers,
    }
}

impl Script {
    pub fn load(path: &str) -> Option<Self> {
        let src = std::fs::read_to_string(path).ok()?;
        let mut queue = VecDeque::new();
        let frame = || Batch::Wait(Duration::from_millis(40));
        for line in src.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
            let nums: Vec<f32> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
            let pos = || Pos2::new(nums.first().copied().unwrap_or(0.0), nums.get(1).copied().unwrap_or(0.0));
            match cmd {
                "wait" => queue.push_back(Batch::Wait(Duration::from_millis(nums.first().copied().unwrap_or(100.0) as u64))),
                "move" => {
                    queue.push_back(Batch::Events(vec![Event::PointerMoved(pos())]));
                    queue.push_back(frame());
                }
                "click" | "rclick" | "shiftclick" | "dclick" => {
                    let b = if cmd == "rclick" { PointerButton::Secondary } else { PointerButton::Primary };
                    let m = if cmd == "shiftclick" { Modifiers::SHIFT } else { Modifiers::NONE };
                    let p = pos();
                    queue.push_back(Batch::Events(vec![Event::PointerMoved(p)]));
                    queue.push_back(frame());
                    let times = if cmd == "dclick" { 2 } else { 1 };
                    for _ in 0..times {
                        let mut ev = Vec::new();
                        if m.shift {
                            ev.push(Event::ModifiersChanged(m));
                        }
                        ev.push(button(p, b, true, m));
                        queue.push_back(Batch::Events(ev));
                        queue.push_back(frame());
                        queue.push_back(Batch::Events(vec![button(p, b, false, m)]));
                        queue.push_back(frame());
                    }
                    if m.shift {
                        queue.push_back(Batch::Events(vec![Event::ModifiersChanged(Modifiers::NONE)]));
                    }
                }
                "drag" => {
                    let from = pos();
                    let to = Pos2::new(nums.get(2).copied().unwrap_or(0.0), nums.get(3).copied().unwrap_or(0.0));
                    queue.push_back(Batch::Events(vec![Event::PointerMoved(from)]));
                    queue.push_back(frame());
                    queue.push_back(Batch::Events(vec![button(from, PointerButton::Primary, true, Modifiers::NONE)]));
                    queue.push_back(frame());
                    for i in 1..=10 {
                        let t = i as f32 / 10.0;
                        queue.push_back(Batch::Events(vec![Event::PointerMoved(from + (to - from) * t)]));
                        queue.push_back(frame());
                    }
                    queue.push_back(Batch::Events(vec![button(to, PointerButton::Primary, false, Modifiers::NONE)]));
                    queue.push_back(frame());
                }
                "key" => {
                    let (m, name) = parse_mods(rest.trim());
                    let Some(key) = Key::from_name(&name) else {
                        eprintln!("script: tecla desconhecida {name}");
                        continue;
                    };
                    let press = |pressed| Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: m,
                    };
                    queue.push_back(Batch::Events(vec![Event::ModifiersChanged(m), press(true)]));
                    queue.push_back(frame());
                    queue.push_back(Batch::Events(vec![press(false), Event::ModifiersChanged(Modifiers::NONE)]));
                    queue.push_back(frame());
                }
                "type" => {
                    queue.push_back(Batch::Events(vec![Event::Text(rest.to_string())]));
                    queue.push_back(frame());
                }
                "scroll" => {
                    queue.push_back(Batch::Events(vec![Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0.0, nums.first().copied().unwrap_or(0.0)),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Wheel,
                        modifiers: Modifiers::NONE,
                    }]));
                    queue.push_back(frame());
                }
                "shot" => {
                    let press = |pressed| Event::Key {
                        key: Key::F12,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    };
                    queue.push_back(Batch::Events(vec![press(true)]));
                    queue.push_back(frame());
                    queue.push_back(Batch::Events(vec![press(false)]));
                    queue.push_back(Batch::Wait(Duration::from_millis(300)));
                }
                "quit" => queue.push_back(Batch::Events(vec![])),
                other => eprintln!("script: comando desconhecido {other}"),
            }
        }
        Some(Self {
            queue,
            resume_at: Instant::now(),
            quit: false,
        })
    }

    pub fn inject(&mut self, ctx: &egui::Context, raw: &mut RawInput) {
        ctx.request_repaint_after(Duration::from_millis(15));
        if Instant::now() < self.resume_at {
            return;
        }
        match self.queue.pop_front() {
            Some(Batch::Wait(d)) => self.resume_at = Instant::now() + d,
            Some(Batch::Events(ev)) => {
                if std::env::var_os("MM_SCRIPT_DEBUG").is_some() {
                    eprintln!("script: {ev:?}");
                }
                raw.events.extend(ev)
            }
            None => self.quit = true,
        }
    }
}
