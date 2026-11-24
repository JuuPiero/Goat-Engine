//! Nintendo 3DS platform: the main loop (APT: HOME button, sleep), input (buttons, circle pad,
//! C-stick on New 3DS, touch screen) mapped onto the portable [`Input`](goat_runtime::Input),
//! graphics through [`Citro3dBackend`](goat_gfx_citro3d::Citro3dBackend).
//!
//! ```ignore
//! fn main() { goat_platform_3ds::run(MyGame); }
//! ```
//! Empty on other targets.

#[cfg(target_os = "horizon")]
mod imp {
    use ctru::prelude::*;
    use ctru::services::hid::KeyPad;
    use goat_core::math::Vec2;
    use goat_gfx_citro3d::Citro3dBackend;
    use goat_runtime::{Axis, Button, Engine, Game};
    use std::time::Instant;

    fn log_sink(level: goat_core::log::Level, args: &std::fmt::Arguments<'_>) {
        let line = format!("[{}] {}\n", level.label(), args);
        // SAFETY: svcOutputDebugString takes a pointer + length (shows in Citra's log / debuggers).
        unsafe { let _ = ctru_sys::svcOutputDebugString(line.as_ptr() as *const _, line.len() as i32); }
    }

    /// Panics show on the bottom screen (libctru console) until HOME / START: on hardware and in
    /// emulators there is no other place to read them.
    fn install_panic_screen() {
        std::panic::set_hook(Box::new(|info| {
            let msg = format!("GoatEngine panic:\n{info}\n\nPress START to exit.\n");
            // SAFETY: libctru calls on the main thread; the console takes over the bottom screen.
            unsafe {
                let _ = ctru_sys::svcOutputDebugString(msg.as_ptr() as *const _, msg.len() as i32);
                ctru_sys::consoleInit(ctru_sys::GFX_BOTTOM, std::ptr::null_mut());
                // stdout = the console once it exists (newlib devoptab)
                use std::io::Write;
                let mut out = std::io::stdout();
                let _ = out.write_all(msg.as_bytes());
                let _ = out.flush();
                while ctru_sys::aptMainLoop() {
                    ctru_sys::hidScanInput();
                    if ctru_sys::hidKeysDown() & ctru_sys::KEY_START != 0 {
                        break;
                    }
                    ctru_sys::gfxFlushBuffers();
                    ctru_sys::gfxSwapBuffers();
                    ctru_sys::gspWaitForEvent(ctru_sys::GSPGPU_EVENT_VBlank0, true);
                }
            }
        }));
    }

    /// The system keyboard (swkbd applet); blocks until OK / cancel. None = cancelled.
    fn show_keyboard(apt: &Apt, gfx: &Gfx, req: &goat_runtime::KeyboardRequest) -> Option<String> {
        use ctru::applets::swkbd::{Button as KbButton, ButtonConfig, Kind, PasswordMode, SoftwareKeyboard};
        let mut kb = SoftwareKeyboard::new(if req.numeric { Kind::Numpad } else { Kind::Normal }, ButtonConfig::LeftRight);
        if !req.hint.is_empty() {
            kb.set_hint_text(Some(&req.hint));
        }
        if !req.initial.is_empty() {
            kb.set_initial_text(Some(std::borrow::Cow::Owned(req.initial.clone())));
        }
        if req.max_length > 0 {
            kb.set_max_text_len(req.max_length.min(u16::MAX as u32) as u16);
        }
        if req.password {
            kb.set_password_mode(PasswordMode::HideDelay);
        }
        match kb.launch(apt, gfx) {
            Ok((text, KbButton::Right)) => Some(text),
            Ok(_) => None,
            Err(e) => {
                goat_core::warn!("keyboard: {e:?}");
                None
            }
        }
    }

    /// Runs a game until it quits or the system closes it.
    pub fn run(game: impl Game) {
        goat_core::log::set_sink(log_sink);
        install_panic_screen();
        let apt = Apt::new().expect("APT");
        let mut hid = Hid::new().expect("HID");
        let ctru_gfx = Gfx::new().expect("GFX");
        // New 3DS: 804 MHz + the bigger L2 cache (no effect on the original 3DS)
        // SAFETY: plain libctru call after APT init.
        unsafe {
            let mut new3ds = false;
            let _ = ctru_sys::APT_CheckNew3DS(&mut new3ds);
            ctru_sys::osSetSpeedupEnable(true);
            goat_core::info!("{} 3DS{}", if new3ds { "New" } else { "Original" }, if new3ds { ": 804 MHz mode" } else { "" });
        }
        // the game's assets folder, packed into the .3dsx (goat-assets reads romfs:/)
        let _romfs = ctru::services::romfs::RomFS::new().map_err(|e| goat_core::warn!("no romfs: {e}")).ok();
        let mut gfx = match Citro3dBackend::new() {
            Ok(g) => g,
            Err(e) => {
                goat_core::error!("graphics: {e}");
                return;
            }
        };
        let mut engine = Engine::new(game);
        // text boxes open the system keyboard
        engine.resources.insert(goat_runtime::SoftwareKeyboard::default());
        let mut last = Instant::now();
        let map = [
            (Button::A, KeyPad::A),
            (Button::B, KeyPad::B),
            (Button::X, KeyPad::X),
            (Button::Y, KeyPad::Y),
            (Button::L, KeyPad::L),
            (Button::R, KeyPad::R),
            (Button::ZL, KeyPad::ZL),
            (Button::ZR, KeyPad::ZR),
            (Button::Start, KeyPad::START),
            (Button::Select, KeyPad::SELECT),
            (Button::Up, KeyPad::DPAD_UP),
            (Button::Down, KeyPad::DPAD_DOWN),
            (Button::Left, KeyPad::DPAD_LEFT),
            (Button::Right, KeyPad::DPAD_RIGHT),
        ];
        while apt.main_loop() {
            hid.scan_input();
            let held = hid.keys_held();
            let input = &mut engine.input;
            input.begin_frame();
            for (b, k) in map {
                input.set(b, held.contains(k));
            }
            let (cx, cy) = hid.circlepad_position();
            let dead = |v: i16| if v.abs() < 15 { 0.0 } else { v as f32 / 156.0 };
            input.set_axis(Axis::LeftX, dead(cx));
            input.set_axis(Axis::LeftY, dead(cy));
            if held.contains(KeyPad::TOUCH) {
                let (tx, ty) = hid.touch_position();
                input.pointer = Some(Vec2::new(tx as f32, ty as f32));
                input.pointer_down = true;
            } else {
                input.pointer = None;
                input.pointer_down = false;
            }
            let now = Instant::now();
            let dt = (now - last).as_secs_f32();
            last = now;
            engine.frame(dt, &mut gfx);
            if engine.should_quit() {
                break;
            }
            let request = engine.resources.get_mut::<goat_runtime::SoftwareKeyboard>().and_then(|k| k.request.take());
            if let Some(req) = request {
                let result = show_keyboard(&apt, &ctru_gfx, &req);
                if let Some(k) = engine.resources.get_mut::<goat_runtime::SoftwareKeyboard>() {
                    k.result = Some(result);
                }
                // the applet took a while: don't count it as a frame
                last = Instant::now();
            }
        }
    }
}

#[cfg(target_os = "horizon")]
pub use imp::run;
