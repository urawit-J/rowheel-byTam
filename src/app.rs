use crate::calibration::{CalibrationStep, CalibrationWizard};
use crate::config::WheelConfig;
use crate::force_feedback::{ForceFeedback, ForceFeedbackDevice};
use crate::input::InputReader;
use crate::virtual_controller::{VirtualController, VirtualXboxController, XboxControllerState};
use eframe::egui;

/// What `connect_outputs` hands back: the virtual pad, the force-feedback
/// device, and a status line for the UI.
type OutputChannels = (
    Option<Box<dyn VirtualController>>,
    Option<Box<dyn ForceFeedback>>,
    String,
);

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Calibrating,
    Running,
}

pub struct RoWheelApp {
    mode: AppMode,
    config: Option<WheelConfig>,
    input_reader: Option<InputReader>,
    virtual_controller: Option<Box<dyn VirtualController>>,
    force_feedback: Option<Box<dyn ForceFeedback>>,
    calibration: Option<CalibrationWizard>,
    /// The G29 photo the wizard rings. `None` only if the asset failed to
    /// decode, in which case calibration still works, just with words alone.
    wheel_photo: Option<egui::TextureHandle>,

    detected_input_info: String,
    status_message: String,
    show_debug: bool,

    current_state: XboxControllerState,
    /// Latched clutch state, so the engage/release hysteresis survives frames.
    clutch_engaged: bool,
    clutch_travel: f32,
    /// Last gate position decoded from the H-shifter, for the debug panel.
    current_gear: i8,
    /// Alternated so the gear axis keeps producing change events; see
    /// `GEAR_AXIS_DITHER`.
    gear_dither: bool,
    gear_dither_at: std::time::Instant,
}

/// Amber reads as "look here" against both the black wheel and the white
/// background the photo is cut out on.
const HIGHLIGHT: egui::Color32 = egui::Color32::from_rgb(255, 170, 0);
const HIGHLIGHT_FILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(90, 60, 0, 90);

/// Windows fonts that carry Thai, best first. Leelawadee UI is the system UI
/// face and sets Latin alongside Thai without the two looking like different
/// documents; Tahoma is the fallback that has shipped with Windows for decades.
const THAI_FONTS: [&str; 2] = [
    r"C:\Windows\Fonts\leelawui.ttf",
    r"C:\Windows\Fonts\tahoma.ttf",
];

impl RoWheelApp {
    /// The UI is Thai, and egui's bundled faces have no Thai glyphs at all --
    /// without this every label renders as empty boxes. The font is read from
    /// the system rather than embedded: this only ever runs on the booth PC,
    /// and 400KB of font does not belong in the repository.
    ///
    /// If neither face is there the app still starts, in English-only glyphs,
    /// rather than refusing to run.
    fn fonts() -> egui::FontDefinitions {
        let mut fonts = egui::FontDefinitions::default();
        for path in THAI_FONTS {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            fonts
                .font_data
                .insert("thai".to_owned(), egui::FontData::from_owned(data).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                // Ahead of the default face, so Thai is drawn by the font that
                // has it and everything else still falls through as before.
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, "thai".to_owned());
            }
            return fonts;
        }
        log::warn!("no Thai font found on this machine; labels will not render");
        fonts
    }

    /// The photo is baked into the binary rather than shipped beside it: the
    /// booth PC gets one file, and a wizard whose pictures can go missing is
    /// exactly the wizard that fails on event morning.
    fn load_wheel_photo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let decoded = match image::load_from_memory(include_bytes!("../assets/g29.jpg")) {
            Ok(decoded) => decoded,
            Err(e) => {
                log::error!("could not decode the wheel photo: {}", e);
                return None;
            }
        };
        // The asset is 2000x2000 because the labels on the wheel had to be
        // legible while the spots were measured off it. Nothing ever draws it
        // larger than a few hundred points, and the full size would sit in
        // video memory as 16MB of RGBA.
        const TEXTURE_SIZE: u32 = 800;
        let decoded = decoded
            .resize(
                TEXTURE_SIZE,
                TEXTURE_SIZE,
                image::imageops::FilterType::CatmullRom,
            )
            .to_rgba8();
        let size = [decoded.width() as usize, decoded.height() as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, decoded.as_raw());
        Some(ctx.load_texture("g29", image, egui::TextureOptions::LINEAR))
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_fonts(Self::fonts());

        let config = WheelConfig::load();
        let has_config = config.as_ref().map(|c| c.is_complete()).unwrap_or(false);

        let input_reader = match InputReader::new() {
            Ok(reader) => Some(reader),
            Err(e) => {
                log::error!("Failed to initialize input reader: {}", e);
                None
            }
        };

        let mode = if has_config {
            AppMode::Running
        } else {
            AppMode::Calibrating
        };

        let calibration = if mode == AppMode::Calibrating {
            Some(CalibrationWizard::new(config.clone()))
        } else {
            None
        };

        let mut app_outputs = None;
        if mode == AppMode::Running {
            app_outputs = Some(Self::connect_outputs());
        }
        let (virtual_controller, force_feedback, status_message) =
            app_outputs.unwrap_or((None, None, String::new()));

        Self {
            mode,
            config,
            input_reader,
            virtual_controller,
            force_feedback,
            calibration,
            wheel_photo: Self::load_wheel_photo(&cc.egui_ctx),
            detected_input_info: String::new(),
            status_message,
            show_debug: false,
            current_state: XboxControllerState::default(),
            clutch_engaged: false,
            clutch_travel: 0.0,
            current_gear: crate::config::GEAR_NEUTRAL,
            gear_dither: false,
            gear_dither_at: std::time::Instant::now(),
        }
    }

    fn start_calibration(&mut self) {
        self.mode = AppMode::Calibrating;
        self.calibration = Some(CalibrationWizard::new(self.config.clone()));
        self.virtual_controller = None;
    }

    /// Bring up the virtual pad and the force-feedback device.
    ///
    /// Both startup and the end of calibration need this. Force feedback used
    /// to be initialised only from `finish_calibration`, so once the config
    /// persisted and the app booted straight into Running it would silently
    /// never start.
    fn connect_pad() -> (Option<Box<dyn VirtualController>>, String) {
        match VirtualXboxController::new() {
            Ok(vc) => (
                Some(Box::new(vc) as Box<dyn VirtualController>),
                "เชื่อมต่อจอยเสมือนแล้ว".to_string(),
            ),
            Err(e) => {
                let msg = format!("Failed to create gamepad: {}", e);
                log::error!("{}", msg);
                (None, msg)
            }
        }
    }

    /// Take the virtual pad away, or bring it back.
    ///
    /// Roblox treats whichever device is reporting as the active one and flips
    /// its UI to match, and this pad never stops reporting: the gear axis is
    /// dithered on purpose so a held gate keeps producing change events (see
    /// `GEAR_AXIS_DITHER`). Clicking into the game while that is going on is a
    /// fight, and closing rowheel and reopening it afterwards was the way round
    /// it. This is the same thing without losing the window.
    ///
    /// The target is dropped rather than merely ignored, so Windows sees the
    /// pad disappear exactly as it would if the process had exited -- ignoring
    /// it would leave an idle pad present, which is still a pad for Roblox to
    /// switch to. Force feedback is left alone: it belongs to the real wheel
    /// and has nothing to do with what Roblox is looking at.
    fn set_output_active(&mut self, active: bool) {
        if !active {
            self.virtual_controller = None;
            self.status_message =
                "หยุดส่งสัญญาณแล้ว — คลิกเข้าเกมให้เรียบร้อย แล้วค่อยกดเริ่มส่งสัญญาณ".to_string();
            return;
        }
        let (virtual_controller, status_message) = Self::connect_pad();
        self.virtual_controller = virtual_controller;
        self.status_message = status_message;
    }

    fn connect_outputs() -> OutputChannels {
        let (virtual_controller, status_message) = Self::connect_pad();

        let force_feedback = match ForceFeedbackDevice::new(None) {
            Ok(ff) if ff.is_available() => {
                log::info!("Force feedback initialized");
                Some(Box::new(ff) as Box<dyn ForceFeedback>)
            }
            Ok(_) => None,
            Err(e) => {
                log::warn!("Force feedback not available: {}", e);
                None
            }
        };

        (virtual_controller, force_feedback, status_message)
    }

    fn finish_calibration(&mut self) {
        if let Some(ref calibration) = self.calibration {
            let config = calibration.config.clone();

            // The only place the config is written. The wizard's `Complete`
            // arm never runs: `advance()` is driven by the Next button, which
            // is not rendered on the final step.
            let save_error = config.save().err().map(|e| {
                log::error!("Failed to save config: {}", e);
                format!("Failed to save config: {}", e)
            });
            self.config = Some(config);

            let (vc, ff, status) = Self::connect_outputs();
            self.virtual_controller = vc;
            self.force_feedback = ff;
            self.status_message = save_error.unwrap_or(status);
        }

        self.calibration = None;
        self.mode = AppMode::Running;
    }

    /// Leave calibration without walking to the end, for rebinding one control
    /// without redoing the other twenty-six.
    ///
    /// Nothing special is needed to keep the rest: `start_calibration` seeds the
    /// wizard from the config that is already loaded, so every step not reached
    /// still holds the binding it had, and saving writes those back unchanged.
    fn exit_calibration(&mut self) {
        let unusable = self
            .calibration
            .as_ref()
            .is_some_and(|c| !c.config.is_complete());

        self.finish_calibration();

        // Writing a half-filled config is safe -- the startup gate is
        // `is_complete()`, so it reopens the wizard next launch rather than
        // booting past it -- but the screen it lands on now would otherwise
        // look like a working rig that simply ignores the wheel.
        if unusable {
            self.status_message = format!(
                "ยังตั้งค่าไม่ครบ: พวงมาลัย แป้นเหยียบ หรือแป้นเกียร์ ยังไม่ได้ผูกปุ่ม · {}",
                self.status_message
            );
        }
    }

    fn process_inputs(&mut self) {
        let Some(ref mut reader) = self.input_reader else {
            return;
        };

        let events = reader.poll();

        if let Some(ref mut calibration) = self.calibration {
            for event in &events {
                calibration.process_event(event);
            }

            if calibration.needs_axis_detection() {
                self.detected_input_info = calibration
                    .get_detected_axis_info()
                    .unwrap_or_else(|| "ขยับอุปกรณ์ที่ต้องการ...".to_string());
            } else if calibration.needs_button_detection() {
                self.detected_input_info = calibration
                    .get_detected_button_info()
                    .unwrap_or_else(|| "กดปุ่มที่ต้องการ...".to_string());
            }
        }

        if self.mode == AppMode::Running {
            if let Some(ref config) = self.config {
                let state = reader.state();

                let mut xbox_state = XboxControllerState::default();

                if let Some(ref steering) = config.steering {
                    if let Some(value) = state.get_axis(&steering.device_id, steering.axis_code) {
                        xbox_state.left_stick_x = steering.normalize(value);
                    }
                }

                if let Some(ref throttle) = config.throttle {
                    if let Some(value) = state.get_axis(&throttle.device_id, throttle.axis_code) {
                        xbox_state.right_trigger = throttle.normalize_trigger(value);
                    }
                }

                if let Some(ref brake) = config.brake {
                    if let Some(value) = state.get_axis(&brake.device_id, brake.axis_code) {
                        xbox_state.left_trigger = brake.normalize_trigger(value);
                    }
                }

                // A-Chassis has no analog clutch -- `ContlrClutch` is a plain
                // hold. Threshold the pedal with hysteresis so a foot resting
                // near the bite point does not chatter the button.
                if let Some(ref clutch) = config.clutch {
                    if let Some(value) = state.get_axis(&clutch.device_id, clutch.axis_code) {
                        let travel = clutch.normalize_trigger(value);
                        self.clutch_engaged = if self.clutch_engaged {
                            travel > config.clutch_release
                        } else {
                            travel >= config.clutch_engage
                        };
                        self.clutch_travel = travel;
                    }
                } else {
                    self.clutch_engaged = false;
                    self.clutch_travel = 0.0;
                }
                // ButtonR3 is the only digital button free in both the driving
                // and the booth-menu maps; the stock ButtonR1 collides with
                // BoothInput.Next, which a held clutch would jam.
                xbox_state.buttons.right_thumb = self.clutch_engaged;

                // The paddles do double duty: ContlrShiftUp/ContlrShiftDown
                // while driving, BoothInput.Next/Previous in the booth menu.
                // The wheel D-pad cannot walk the menu -- the adapter reads it
                // as a POV hat and never sees its RIGHT direction -- and Drive
                // stands down while a menu owns input, so the two never clash.
                if let Some(ref shift_up) = config.shift_up {
                    xbox_state.buttons.y = shift_up.is_pressed(state);
                }

                if let Some(ref shift_down) = config.shift_down {
                    xbox_state.buttons.x = shift_down.is_pressed(state);
                }

                // H-pattern gear. The HGP reports every gate position as its own
                // button, and no button pressed means the lever is in neutral.
                //
                // `config.gears` is kept sorted ascending, so reverse is examined
                // first and wins: shifters that report reverse as "gear 7 plus a
                // reverse button" hold two buttons at once, and reverse is the
                // one the driver means. With no shifter configured both axes stay
                // at rest, which is the game-side signal to ignore them.
                if !config.gears.is_empty() {
                    let gear = config
                        .gears
                        .iter()
                        .find(|b| b.button.is_pressed(state))
                        .map(|b| b.gear)
                        .unwrap_or(crate::config::GEAR_NEUTRAL);

                    self.current_gear = gear;
                    if self.gear_dither_at.elapsed() >= std::time::Duration::from_millis(500) {
                        self.gear_dither_at = std::time::Instant::now();
                        self.gear_dither = !self.gear_dither;
                    }
                    let dither = if self.gear_dither {
                        crate::config::GEAR_AXIS_DITHER
                    } else {
                        0.0
                    };
                    xbox_state.right_stick_x = crate::config::encode_gear(gear) + dither;
                }

                if let Some(ref camera) = config.camera {
                    xbox_state.buttons.dpad_down = camera.is_pressed(state);
                }

                if let Some(ref recovery) = config.recovery {
                    xbox_state.buttons.dpad_left = recovery.is_pressed(state);
                }

                if let Some(ref mirror_camera) = config.mirror_camera {
                    xbox_state.buttons.dpad_right = mirror_camera.is_pressed(state);
                }

                // The booth menu draws these two as the wheel's cross and circle,
                // and BoothInput.Confirm/Cancel already accept them.
                if let Some(ref menu_confirm) = config.menu_confirm {
                    xbox_state.buttons.a = menu_confirm.is_pressed(state);
                }

                if let Some(ref menu_cancel) = config.menu_cancel {
                    xbox_state.buttons.b = menu_cancel.is_pressed(state);
                }

                if let Some(ref gear_mode) = config.gear_mode {
                    xbox_state.buttons.dpad_up = gear_mode.is_pressed(state);
                }

                self.current_state = xbox_state.clone();

                if let Some(ref mut vc) = self.virtual_controller {
                    if let Err(e) = vc.update(&xbox_state) {
                        log::error!("Failed to update virtual controller: {}", e);
                    }

                    if let Ok(rumble) = vc.get_rumble() {
                        if rumble.large_motor > 0.01 || rumble.small_motor > 0.01 {
                            log::info!(
                                "Rumble from game: large={:.2}, small={:.2}",
                                rumble.large_motor,
                                rumble.small_motor
                            );
                        }
                        if let Some(ref mut ff) = self.force_feedback {
                            if let Err(e) = ff.apply_rumble(&rumble) {
                                log::error!("Failed to apply force feedback: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    fn render_calibration_ui(&mut self, ctx: &egui::Context) {
        // Claims its height before the content does, so Next can never be
        // pushed out of reach.
        self.render_calibration_controls(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            // And whatever is left still scrolls, for a window too short even
            // for the photo.
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.heading("ตั้งค่าพวงมาลัย RoWheel");
                    ui.add_space(30.0);

                    if let Some(ref calibration) = self.calibration {
                        let progress =
                            calibration.step.index() as f32 / CalibrationStep::TOTAL_STEPS as f32;

                        ui.add(egui::ProgressBar::new(progress).show_percentage());
                        ui.add_space(20.0);

                        let step_name = calibration.step.title();
                        ui.label(egui::RichText::new(step_name).size(24.0).strong());
                        ui.add_space(15.0);

                        ui.label(egui::RichText::new(calibration.step.instructions()).size(16.0));
                        ui.add_space(14.0);

                        // The control this step wants, ringed on the photo. Whoever
                        // sets the booth up on the day did not choose the mapping,
                        // and "L2" is only a name until you can see which one it is.
                        let spots = calibration.step.spots();
                        if let (false, Some(photo)) = (spots.is_empty(), self.wheel_photo.as_ref())
                        {
                            // Sized off the window rather than fixed: the booth PC
                            // is not the machine this was written on.
                            let side = (ui.available_width() - 40.0).clamp(160.0, 340.0);
                            let shown =
                                egui::load::SizedTexture::new(photo.id(), egui::vec2(side, side));
                            let rect = ui.add(egui::Image::from_texture(shown)).rect;
                            let painter = ui.painter();
                            for spot in spots {
                                let marked = egui::Rect::from_min_size(
                                    rect.min
                                        + egui::vec2(spot.x * rect.width(), spot.y * rect.height()),
                                    egui::vec2(spot.w * rect.width(), spot.h * rect.height()),
                                )
                                .expand(3.0);
                                // Filled as well as ringed: a thin outline on a
                                // photo of a black wheel is easy to miss.
                                painter.rect_filled(marked, 6.0, HIGHLIGHT_FILL);
                                painter.rect_stroke(
                                    marked,
                                    6.0,
                                    egui::Stroke::new(3.0_f32, HIGHLIGHT),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            ui.add_space(14.0);
                        }
                        ui.add_space(6.0);

                        if calibration.needs_axis_detection()
                            || calibration.needs_button_detection()
                        {
                            let needs_button = calibration.needs_button_detection();
                            ui.group(|ui| {
                                ui.label("ตรวจพบ:");
                                ui.label(
                                    egui::RichText::new(&self.detected_input_info).monospace(),
                                );

                                // Not every H-shifter reports every gate as a plain
                                // button -- some gates report nothing at all. Show
                                // what the hardware is actually sending so a gate
                                // that cannot be bound is visible here rather than
                                // silently skipped.
                                if needs_button {
                                    ui.separator();
                                    ui.label("ปุ่มที่กดค้างอยู่ตอนนี้:");
                                    let mut any = false;
                                    if let Some(ref reader) = self.input_reader {
                                        for (device_id, buttons) in &reader.state().buttons {
                                            let name = reader
                                                .devices()
                                                .get(device_id)
                                                .map(|d| d.name.as_str())
                                                .unwrap_or(device_id.as_str());
                                            for (code, pressed) in buttons {
                                                if *pressed {
                                                    any = true;
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "{} - button {}",
                                                            name, code
                                                        ))
                                                        .monospace(),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    if !any {
                                        ui.label(egui::RichText::new("(none)").monospace().weak());
                                    }
                                }
                            });
                            ui.add_space(20.0);
                        }
                    }
                });
            });
        });
    }

    /// The step controls, in a panel of their own.
    ///
    /// They used to sit under the content, and on the booth PC's 600px-tall
    /// window the photo pushed Next clean off the bottom with no way to reach
    /// it. A bottom panel is laid out first and keeps its height whatever the
    /// step above is showing.
    fn render_calibration_controls(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("calibration_controls").show(ctx, |ui| {
            ui.add_space(10.0);
            let (is_complete, can_skip, can_go_back) = self
                .calibration
                .as_ref()
                .map(|c| {
                    (
                        c.step == CalibrationStep::Complete,
                        c.step.can_skip(),
                        c.step != CalibrationStep::Welcome,
                    )
                })
                .unwrap_or((false, false, false));

            ui.horizontal(|ui| {
                // Offered on Complete too: the last step is the one most
                // worth a second look before the config is written.
                if can_go_back
                    && ui
                        .button(egui::RichText::new("ย้อนกลับ").size(18.0))
                        .clicked()
                {
                    if let Some(ref mut cal) = self.calibration {
                        cal.back();
                    }
                }

                if is_complete {
                    if ui
                        .button(egui::RichText::new("เริ่มใช้งาน").size(18.0))
                        .clicked()
                    {
                        self.finish_calibration();
                    }
                } else if self.calibration.is_some() {
                    if ui.button(egui::RichText::new("ถัดไป").size(18.0)).clicked() {
                        if let Some(ref mut cal) = self.calibration {
                            cal.advance();
                        }
                    }

                    if can_skip {
                        if ui.button(egui::RichText::new("ข้าม").size(18.0)).clicked() {
                            if let Some(ref mut cal) = self.calibration {
                                cal.skip();
                            }
                        }
                    }

                    // Set apart from the step controls: this one ends the
                    // wizard rather than moving through it.
                    ui.add_space(24.0);
                    if ui
                        .button(egui::RichText::new("ออกจากการตั้งค่า").size(18.0))
                        .on_hover_text(
                            "ออกตอนนี้โดยเก็บค่าที่ตั้งไปแล้วไว้\n\
                                 ขั้นที่ยังไม่ได้ทำ จะใช้ค่าเดิมที่เคยตั้งไว้",
                        )
                        .clicked()
                    {
                        self.exit_calibration();
                    }
                }
            });
            ui.add_space(10.0);
        });
    }

    fn render_running_ui(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("RoWheel");
                ui.separator();
                if ui.button("ตั้งค่าใหม่").clicked() {
                    self.start_calibration();
                }
                ui.separator();
                let sending = self.virtual_controller.is_some();
                if ui
                    .button(if sending {
                        "หยุดส่งสัญญาณ"
                    } else {
                        "เริ่มส่งสัญญาณ"
                    })
                    .on_hover_text(
                        "ปิดไว้ตอนคลิกเข้าเกม Roblox แล้วค่อยเปิดอีกที\n\
                         จอยเสมือนจะหายไปจากเครื่องเหมือนตอนปิดโปรแกรม",
                    )
                    .clicked()
                {
                    self.set_output_active(!sending);
                }
                ui.separator();
                ui.checkbox(&mut self.show_debug, "ข้อมูลดีบัก");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let status = if self
                        .virtual_controller
                        .as_ref()
                        .map(|vc| vc.is_connected())
                        .unwrap_or(false)
                    {
                        egui::RichText::new("เชื่อมต่อแล้ว").color(egui::Color32::GREEN)
                    } else {
                        egui::RichText::new("ไม่ได้เชื่อมต่อ").color(egui::Color32::RED)
                    };
                    ui.label(status);
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.status_message.is_empty() {
                ui.label(&self.status_message);
                ui.add_space(10.0);
            }

            ui.heading("สัญญาณที่ส่งออก");
            ui.add_space(10.0);

            ui.columns(2, |columns| {
                columns[0].group(|ui| {
                    ui.label("แกนพวงมาลัย");
                    ui.horizontal(|ui| {
                        ui.label(format!("X: {:.2}", self.current_state.left_stick_x));
                        ui.label(format!("Y: {:.2}", self.current_state.left_stick_y));
                    });
                    let stick_x = (self.current_state.left_stick_x + 1.0) / 2.0;
                    ui.add(egui::ProgressBar::new(stick_x).text("พวงมาลัย"));
                });

                columns[0].add_space(10.0);

                columns[0].group(|ui| {
                    ui.label("แป้นเหยียบ");
                    ui.add(egui::ProgressBar::new(self.current_state.left_trigger).text("เบรก"));
                    ui.add(egui::ProgressBar::new(self.current_state.right_trigger).text("คันเร่ง"));
                });

                columns[1].group(|ui| {
                    ui.label("ปุ่ม");
                    let lamp = |on: bool, text: &str| {
                        let color = if on {
                            egui::Color32::LIGHT_GREEN
                        } else {
                            egui::Color32::DARK_GRAY
                        };
                        egui::RichText::new(text).color(color)
                    };
                    let b = &self.current_state.buttons;
                    // One to a row: these two names side by side ran off the
                    // edge of the panel.
                    ui.label(lamp(b.y, "paddle shift ขวา · เปลี่ยนเกียร์ขึ้น"));
                    ui.label(lamp(b.x, "paddle shift ซ้าย · เปลี่ยนเกียร์ลง"));
                    ui.horizontal(|ui| {
                        ui.label(lamp(b.dpad_up, "R2 · โหมดเกียร์"));
                        ui.label(lamp(b.dpad_down, "R3 · มุมกล้อง"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(lamp(b.dpad_left, "L3 · กู้รถ"));
                        ui.label(lamp(b.dpad_right, "L2 · กระจกมองข้าง"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(lamp(b.a, "× · ยืนยัน"));
                        ui.label(lamp(b.b, "○ · ย้อนกลับ"));
                    });
                });

                columns[1].add_space(10.0);

                columns[1].group(|ui| {
                    ui.label("คลัตช์");
                    ui.add(egui::ProgressBar::new(self.clutch_travel).text(
                        if self.clutch_engaged {
                            "เหยียบอยู่"
                        } else {
                            "ปล่อย"
                        },
                    ));
                });

                columns[1].add_space(10.0);

                columns[1].group(|ui| {
                    ui.label("คันเกียร์");
                    let gear = match self.current_gear {
                        crate::config::GEAR_REVERSE => "R".to_string(),
                        crate::config::GEAR_NEUTRAL => "N".to_string(),
                        g => g.to_string(),
                    };
                    ui.label(
                        egui::RichText::new(format!("เกียร์ {}", gear))
                            .size(20.0)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "x={:.2} y={:.2}",
                            self.current_state.right_stick_x, self.current_state.right_stick_y
                        ))
                        .monospace(),
                    );
                });
            });

            if self.show_debug {
                ui.add_space(20.0);
                ui.separator();
                ui.heading("ข้อมูลดีบัก");

                if let Some(ref config) = self.config {
                    ui.collapsing("ค่าที่ตั้งไว้", |ui| {
                        if let Some(ref s) = config.steering {
                            let raw_value = self
                                .input_reader
                                .as_ref()
                                .and_then(|r| r.state().get_axis(&s.device_id, s.axis_code));
                            ui.label(format!(
                                "Steering: axis {} cal=[{:.6}, {:.6}]",
                                s.axis_code, s.min_value, s.max_value
                            ));
                            ui.label(format!(
                                "  raw={:.6} out={:.6}",
                                raw_value.unwrap_or(0.0),
                                self.current_state.left_stick_x
                            ));
                        }
                        if let Some(ref t) = config.throttle {
                            ui.label(format!(
                                "Throttle: {} axis {} [{:.2} - {:.2}]",
                                t.device_name, t.axis_code, t.min_value, t.max_value
                            ));
                        }
                        if let Some(ref b) = config.brake {
                            ui.label(format!(
                                "Brake: {} axis {} [{:.2} - {:.2}]",
                                b.device_name, b.axis_code, b.min_value, b.max_value
                            ));
                        }
                        if let Some(ref c) = config.clutch {
                            ui.label(format!(
                                "Clutch: {} axis {} [{:.2} - {:.2}]",
                                c.device_name, c.axis_code, c.min_value, c.max_value
                            ));
                        }
                        if let Some(ref su) = config.shift_up {
                            ui.label(format!(
                                "Shift Up: {} button {} (Y={})",
                                su.device_name, su.button_code, self.current_state.buttons.y
                            ));
                        }
                        if let Some(ref sd) = config.shift_down {
                            ui.label(format!(
                                "Shift Down: {} button {} (X={})",
                                sd.device_name, sd.button_code, self.current_state.buttons.x
                            ));
                        }
                        for g in &config.gears {
                            let label = match g.gear {
                                crate::config::GEAR_REVERSE => "R".to_string(),
                                n => n.to_string(),
                            };
                            let pressed = self
                                .input_reader
                                .as_ref()
                                .and_then(|r| {
                                    r.state()
                                        .get_button(&g.button.device_id, g.button.button_code)
                                })
                                .unwrap_or(false);
                            ui.label(format!(
                                "Gear {}: {} button {} ({})",
                                label,
                                g.button.device_name,
                                g.button.button_code,
                                if pressed { "IN" } else { "-" }
                            ));
                        }
                    });
                }

                if let Some(ref reader) = self.input_reader {
                    ui.collapsing("อุปกรณ์ที่ต่ออยู่", |ui| {
                        for (id, device) in reader.devices() {
                            ui.label(format!(
                                "{}: {} (FF: {})",
                                id, device.name, device.has_force_feedback
                            ));
                        }
                    });

                    ui.collapsing("สถานะปุ่มดิบ", |ui| {
                        let state = reader.state();
                        for (device_id, buttons) in &state.buttons {
                            for (code, pressed) in buttons {
                                if *pressed {
                                    ui.label(format!(
                                        "Device {} Button {}: PRESSED",
                                        device_id, code
                                    ));
                                }
                            }
                        }
                    });
                }
            }
        });
    }
}

impl eframe::App for RoWheelApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_inputs();
        ctx.request_repaint();

        match self.mode {
            AppMode::Calibrating => self.render_calibration_ui(ctx),
            AppMode::Running => self.render_running_ui(ctx),
        }
    }
}
