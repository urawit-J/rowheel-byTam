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

    detected_input_info: String,
    status_message: String,
    show_debug: bool,

    current_state: XboxControllerState,
    /// Latched clutch state, so the engage/release hysteresis survives frames.
    clutch_engaged: bool,
    clutch_travel: f32,
    /// Last gate position decoded from the H-shifter, for the debug panel.
    current_gear: i8,
}

impl RoWheelApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let fonts = egui::FontDefinitions::default();
        cc.egui_ctx.set_fonts(fonts);

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
            detected_input_info: String::new(),
            status_message,
            show_debug: false,
            current_state: XboxControllerState::default(),
            clutch_engaged: false,
            clutch_travel: 0.0,
            current_gear: crate::config::GEAR_NEUTRAL,
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
    fn connect_outputs() -> OutputChannels {
        let (virtual_controller, status_message) = match VirtualXboxController::new() {
            Ok(vc) => (
                Some(Box::new(vc) as Box<dyn VirtualController>),
                "Gamepad connected".to_string(),
            ),
            Err(e) => {
                let msg = format!("Failed to create gamepad: {}", e);
                log::error!("{}", msg);
                (None, msg)
            }
        };

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
                    .unwrap_or_else(|| "Move an input...".to_string());
            } else if calibration.needs_button_detection() {
                self.detected_input_info = calibration
                    .get_detected_button_info()
                    .unwrap_or_else(|| "Press a button...".to_string());
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

                if let Some(ref shift_up) = config.shift_up {
                    if let Some(pressed) = state.get_button(&shift_up.device_id, shift_up.button_code) {
                        xbox_state.buttons.y = pressed;
                    }
                }

                if let Some(ref shift_down) = config.shift_down {
                    if let Some(pressed) = state.get_button(&shift_down.device_id, shift_down.button_code) {
                        xbox_state.buttons.x = pressed;
                    }
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
                        .find(|b| {
                            state
                                .get_button(&b.button.device_id, b.button.button_code)
                                .unwrap_or(false)
                        })
                        .map(|b| b.gear)
                        .unwrap_or(crate::config::GEAR_NEUTRAL);

                    self.current_gear = gear;
                    xbox_state.right_stick_x = crate::config::encode_gear(gear);
                }

                if let Some(ref camera) = config.camera {
                    if let Some(pressed) = state.get_button(&camera.device_id, camera.button_code) {
                        xbox_state.buttons.dpad_down = pressed;
                    }
                }

                if let Some(ref recovery) = config.recovery {
                    if let Some(pressed) = state.get_button(&recovery.device_id, recovery.button_code) {
                        xbox_state.buttons.dpad_left = pressed;
                    }
                }

                if let Some(ref parking_aid) = config.parking_aid {
                    if let Some(pressed) = state.get_button(&parking_aid.device_id, parking_aid.button_code) {
                        xbox_state.buttons.dpad_right = pressed;
                    }
                }

                if let Some(ref gear_mode) = config.gear_mode {
                    if let Some(pressed) = state.get_button(&gear_mode.device_id, gear_mode.button_code) {
                        xbox_state.buttons.dpad_up = pressed;
                    }
                }

                self.current_state = xbox_state.clone();

                if let Some(ref mut vc) = self.virtual_controller {
                    if let Err(e) = vc.update(&xbox_state) {
                        log::error!("Failed to update virtual controller: {}", e);
                    }

                    if let Ok(rumble) = vc.get_rumble() {
                        if rumble.large_motor > 0.01 || rumble.small_motor > 0.01 {
                            log::info!("Rumble from game: large={:.2}, small={:.2}",
                                       rumble.large_motor, rumble.small_motor);
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
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.heading("RoWheel Calibration");
                ui.add_space(30.0);

                if let Some(ref calibration) = self.calibration {
                    let progress = calibration.step.index() as f32 / CalibrationStep::TOTAL_STEPS as f32;

                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                    ui.add_space(20.0);

                    let step_name = calibration.step.title();
                    ui.label(egui::RichText::new(step_name).size(24.0).strong());
                    ui.add_space(15.0);

                    ui.label(egui::RichText::new(calibration.step.instructions()).size(16.0));
                    ui.add_space(20.0);

                    if calibration.needs_axis_detection() || calibration.needs_button_detection() {
                        let needs_button = calibration.needs_button_detection();
                        ui.group(|ui| {
                            ui.label("Detected:");
                            ui.label(egui::RichText::new(&self.detected_input_info).monospace());

                            // Not every H-shifter reports every gate as a plain
                            // button -- some gates report nothing at all. Show
                            // what the hardware is actually sending so a gate
                            // that cannot be bound is visible here rather than
                            // silently skipped.
                            if needs_button {
                                ui.separator();
                                ui.label("Buttons held right now:");
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
                                    ui.label(
                                        egui::RichText::new("(none)").monospace().weak(),
                                    );
                                }
                            }
                        });
                        ui.add_space(20.0);
                    }
                }

                let (is_complete, can_skip) = self.calibration
                    .as_ref()
                    .map(|c| (c.step == CalibrationStep::Complete, c.step.can_skip()))
                    .unwrap_or((false, false));

                ui.horizontal(|ui| {
                    if is_complete {
                        if ui.button(egui::RichText::new("Start").size(18.0)).clicked() {
                            self.finish_calibration();
                        }
                    } else if self.calibration.is_some() {
                        if ui.button(egui::RichText::new("Next").size(18.0)).clicked() {
                            if let Some(ref mut cal) = self.calibration {
                                cal.advance();
                            }
                        }

                        if can_skip {
                            if ui.button(egui::RichText::new("Skip").size(18.0)).clicked() {
                                if let Some(ref mut cal) = self.calibration {
                                    cal.skip();
                                }
                            }
                        }
                    }
                });
            });
        });
    }

    fn render_running_ui(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("RoWheel");
                ui.separator();
                if ui.button("Recalibrate").clicked() {
                    self.start_calibration();
                }
                ui.separator();
                ui.checkbox(&mut self.show_debug, "Debug");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let status = if self.virtual_controller.as_ref().map(|vc| vc.is_connected()).unwrap_or(false) {
                        egui::RichText::new("Connected").color(egui::Color32::GREEN)
                    } else {
                        egui::RichText::new("Disconnected").color(egui::Color32::RED)
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

            ui.heading("Gamepad Output");
            ui.add_space(10.0);

            ui.columns(2, |columns| {
                columns[0].group(|ui| {
                    ui.label("Left Stick");
                    ui.horizontal(|ui| {
                        ui.label(format!("X: {:.2}", self.current_state.left_stick_x));
                        ui.label(format!("Y: {:.2}", self.current_state.left_stick_y));
                    });
                    let stick_x = (self.current_state.left_stick_x + 1.0) / 2.0;
                    ui.add(egui::ProgressBar::new(stick_x).text("Steering"));
                });

                columns[0].add_space(10.0);

                columns[0].group(|ui| {
                    ui.label("Triggers");
                    ui.add(egui::ProgressBar::new(self.current_state.left_trigger).text("Brake (LT)"));
                    ui.add(egui::ProgressBar::new(self.current_state.right_trigger).text("Throttle (RT)"));
                });

                columns[1].group(|ui| {
                    ui.label("Buttons");
                    let lamp = |on: bool, text: &str| {
                        let color = if on { egui::Color32::LIGHT_GREEN } else { egui::Color32::DARK_GRAY };
                        egui::RichText::new(text).color(color)
                    };
                    let b = &self.current_state.buttons;
                    ui.horizontal(|ui| {
                        ui.label(lamp(b.y, "Y Shift Up"));
                        ui.label(lamp(b.x, "X Shift Down"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(lamp(b.dpad_up, "Up Trans"));
                        ui.label(lamp(b.dpad_down, "Down Camera"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(lamp(b.dpad_left, "Left Recovery"));
                        ui.label(lamp(b.dpad_right, "Right Parking"));
                    });
                });

                columns[1].add_space(10.0);

                columns[1].group(|ui| {
                    ui.label("Clutch (R3)");
                    ui.add(
                        egui::ProgressBar::new(self.clutch_travel).text(if self.clutch_engaged {
                            "Clutch IN"
                        } else {
                            "Clutch out"
                        }),
                    );
                });

                columns[1].add_space(10.0);

                columns[1].group(|ui| {
                    ui.label("Shifter (Right Stick)");
                    let gear = match self.current_gear {
                        crate::config::GEAR_REVERSE => "R".to_string(),
                        crate::config::GEAR_NEUTRAL => "N".to_string(),
                        g => g.to_string(),
                    };
                    ui.label(
                        egui::RichText::new(format!("Gear {}", gear))
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
                ui.heading("Debug Info");

                if let Some(ref config) = self.config {
                    ui.collapsing("Configuration", |ui| {
                        if let Some(ref s) = config.steering {
                            let raw_value = self.input_reader.as_ref()
                                .and_then(|r| r.state().get_axis(&s.device_id, s.axis_code));
                            ui.label(format!("Steering: axis {} cal=[{:.6}, {:.6}]",
                                s.axis_code, s.min_value, s.max_value));
                            ui.label(format!("  raw={:.6} out={:.6}",
                                raw_value.unwrap_or(0.0), self.current_state.left_stick_x));
                        }
                        if let Some(ref t) = config.throttle {
                            ui.label(format!("Throttle: {} axis {} [{:.2} - {:.2}]",
                                t.device_name, t.axis_code, t.min_value, t.max_value));
                        }
                        if let Some(ref b) = config.brake {
                            ui.label(format!("Brake: {} axis {} [{:.2} - {:.2}]",
                                b.device_name, b.axis_code, b.min_value, b.max_value));
                        }
                        if let Some(ref c) = config.clutch {
                            ui.label(format!("Clutch: {} axis {} [{:.2} - {:.2}]",
                                c.device_name, c.axis_code, c.min_value, c.max_value));
                        }
                        if let Some(ref su) = config.shift_up {
                            ui.label(format!("Shift Up: {} button {} (Y={})",
                                su.device_name, su.button_code, self.current_state.buttons.y));
                        }
                        if let Some(ref sd) = config.shift_down {
                            ui.label(format!("Shift Down: {} button {} (X={})",
                                sd.device_name, sd.button_code, self.current_state.buttons.x));
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
                                    r.state().get_button(
                                        &g.button.device_id,
                                        g.button.button_code,
                                    )
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
                    ui.collapsing("Connected Devices", |ui| {
                        for (id, device) in reader.devices() {
                            ui.label(format!("{}: {} (FF: {})",
                                id, device.name, device.has_force_feedback));
                        }
                    });

                    ui.collapsing("Raw Button States", |ui| {
                        let state = reader.state();
                        for (device_id, buttons) in &state.buttons {
                            for (code, pressed) in buttons {
                                if *pressed {
                                    ui.label(format!("Device {} Button {}: PRESSED", device_id, code));
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
