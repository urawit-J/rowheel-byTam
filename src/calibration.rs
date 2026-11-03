use crate::config::{AxisBinding, ButtonBinding, WheelConfig, GEAR_MAX, GEAR_REVERSE};
use crate::input::InputEvent;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalibrationStep {
    Welcome,
    SteeringLeft,
    SteeringRight,
    ThrottlePressed,
    ThrottleReleased,
    BrakePressed,
    BrakeReleased,
    ClutchPressed,
    ClutchReleased,
    ShiftUp,
    ShiftDown,
    /// One H-pattern gate position: `GEAR_REVERSE`, then 1..=GEAR_MAX.
    Gear(i8),
    Camera,
    Recovery,
    MirrorCamera,
    GearMode,
    /// Cross and circle: the two choices on the post-race board.
    MenuConfirm,
    MenuCancel,
    Complete,
}

impl CalibrationStep {
    // Spartan cabinets drive an automatic on the paddles, so the wizard walks
    // the wheel, the two pedals and the two paddles only. The clutch, gate and
    // IC booth-button steps below are kept compiled -- they are simply no longer
    // reachable from `Welcome` -- so this branch can be merged back or
    // re-expanded without rebuilding the capture logic.
    pub const TOTAL_STEPS: usize = 10;

    pub fn index(&self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::SteeringLeft => 1,
            Self::SteeringRight => 2,
            Self::ThrottlePressed => 3,
            Self::ThrottleReleased => 4,
            Self::BrakePressed => 5,
            Self::BrakeReleased => 6,
            Self::ShiftUp => 7,
            Self::ShiftDown => 8,
            Self::Complete => 9,
            // Off the walked path, so these never reach the progress counter.
            // They stay numbered past `Complete` and clear of each other so the
            // uniqueness test still means something if a step is wired back in.
            Self::ClutchPressed => 10,
            Self::ClutchReleased => 11,
            // Reverse is asked last, so the forward gates establish which device
            // the shifter is before the hardest gate to engage comes up.
            Self::Gear(GEAR_REVERSE) => 19,
            Self::Gear(g) => 11 + (*g as usize),
            Self::Camera => 20,
            Self::Recovery => 21,
            Self::MirrorCamera => 22,
            Self::GearMode => 23,
            Self::MenuConfirm => 24,
            Self::MenuCancel => 25,
        }
    }

    pub fn title(&self) -> String {
        match self {
            Self::Welcome => "Welcome".to_string(),
            Self::SteeringLeft | Self::SteeringRight => "Steering".to_string(),
            Self::ThrottlePressed | Self::ThrottleReleased => "Throttle".to_string(),
            Self::BrakePressed | Self::BrakeReleased => "Brake".to_string(),
            Self::ClutchPressed | Self::ClutchReleased => "Clutch".to_string(),
            Self::ShiftUp => "Shift Up".to_string(),
            Self::ShiftDown => "Shift Down".to_string(),
            Self::Gear(GEAR_REVERSE) => "Shifter - Reverse".to_string(),
            Self::Gear(g) => format!("Shifter - Gear {}", g),
            Self::Camera => "Driver View".to_string(),
            Self::Recovery => "Recovery".to_string(),
            Self::MirrorCamera => "Look Back".to_string(),
            Self::GearMode => "Transmission Mode".to_string(),
            Self::MenuConfirm => "Play Again".to_string(),
            Self::MenuCancel => "Race Next Booth".to_string(),
            Self::Complete => "Complete".to_string(),
        }
    }

    pub fn instructions(&self) -> String {
        match self {
            Self::Welcome => "Make sure your wheel and pedals are connected".to_string(),
            Self::SteeringLeft => {
                "Turn the steering wheel all the way to the LEFT, then continue".to_string()
            }
            Self::SteeringRight => {
                "Turn the steering wheel all the way to the RIGHT, then continue".to_string()
            }
            Self::ThrottlePressed => {
                "Press the THROTTLE pedal all the way down, then continue".to_string()
            }
            Self::ThrottleReleased => {
                "Release the THROTTLE pedal completely, then continue".to_string()
            }
            Self::BrakePressed => "Press the BRAKE pedal all the way down, then continue".to_string(),
            Self::BrakeReleased => "Release the BRAKE pedal completely, then continue".to_string(),
            Self::ClutchPressed => {
                "Press the CLUTCH pedal all the way down, then continue.\n(Or skip if you have no clutch)"
                    .to_string()
            }
            Self::ClutchReleased => {
                "Release the CLUTCH pedal completely, then continue".to_string()
            }
            Self::ShiftUp => "Press the SHIFT UP paddle, then continue".to_string(),
            Self::ShiftDown => "Press the SHIFT DOWN paddle, then continue".to_string(),
            Self::Gear(GEAR_REVERSE) => {
                "Engage REVERSE on the H-shifter, then continue.\n(Skip any gate your shifter does not have)"
                    .to_string()
            }
            Self::Gear(g) => format!(
                "Engage GEAR {} on the H-shifter, then continue.\n(Skip any gate your shifter does not have)",
                g
            ),
            Self::Camera => "Press the DRIVER VIEW button (wheel R3), then continue".to_string(),
            Self::Recovery => "Press the RECOVERY button (wheel L3), then continue".to_string(),
            Self::MirrorCamera => {
                "Press the LOOK BACK button (wheel L2), then continue".to_string()
            }
            Self::GearMode => {
                "Press the TRANSMISSION MODE button (wheel R2), then continue".to_string()
            }
            Self::MenuConfirm => {
                "Press the CONFIRM button (wheel cross), then continue".to_string()
            }
            Self::MenuCancel => {
                "Press the BACK button (wheel circle), then continue".to_string()
            }
            Self::Complete => "Successfully calibrated".to_string(),
        }
    }

    pub fn can_skip(&self) -> bool {
        matches!(
            self,
            Self::ClutchPressed | Self::ClutchReleased | Self::Gear(_)
        )
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Welcome => Self::SteeringLeft,
            Self::SteeringLeft => Self::SteeringRight,
            Self::SteeringRight => Self::ThrottlePressed,
            Self::ThrottlePressed => Self::ThrottleReleased,
            Self::ThrottleReleased => Self::BrakePressed,
            Self::BrakePressed => Self::BrakeReleased,
            // Spartan: straight from the brake to the paddles, and done after
            // them. The clutch and the gates have no hardware on these cabinets.
            Self::BrakeReleased => Self::ShiftUp,
            Self::ClutchPressed => Self::ClutchReleased,
            Self::ClutchReleased => Self::ShiftUp,
            Self::ShiftUp => Self::ShiftDown,
            Self::ShiftDown => Self::Complete,
            Self::Gear(GEAR_REVERSE) => Self::Camera,
            Self::Gear(g) if *g >= GEAR_MAX => Self::Gear(GEAR_REVERSE),
            Self::Gear(g) => Self::Gear(g + 1),
            Self::Camera => Self::Recovery,
            Self::Recovery => Self::MirrorCamera,
            Self::MirrorCamera => Self::GearMode,
            Self::GearMode => Self::MenuConfirm,
            Self::MenuConfirm => Self::MenuCancel,
            Self::MenuCancel => Self::Complete,
            Self::Complete => Self::Complete,
        }
    }

    /// Where the Skip button lands. Skipping the clutch drops both of its
    /// steps; skipping a gate just moves on to the next one.
    pub fn skip_target(&self) -> Self {
        if *self == Self::ClutchPressed || *self == Self::ClutchReleased {
            Self::ShiftUp
        } else {
            self.next()
        }
    }
}

// Tracker for axis movement
#[derive(Debug, Clone)]
struct AxisTracker {
    device_id: String,
    device_name: String,
    axis_code: u32,
    initial_value: f32,
    current_value: f32,
}

impl AxisTracker {
    fn movement(&self) -> f32 {
        (self.current_value - self.initial_value).abs()
    }
}

pub struct CalibrationWizard {
    pub step: CalibrationStep,
    pub config: WheelConfig,

    axis_trackers: HashMap<(String, u32), AxisTracker>,
    captured_axis: Option<(String, String, u32, f32)>, // device_id, device_name, axis_code, value

    captured_button: Option<(String, String, u32)>, // device_id, device_name, button_code
}

impl CalibrationWizard {
    pub fn new(existing_config: Option<WheelConfig>) -> Self {
        Self {
            step: CalibrationStep::Welcome,
            config: existing_config.unwrap_or_default(),
            axis_trackers: HashMap::new(),
            captured_axis: None,
            captured_button: None,
        }
    }

    pub fn process_event(&mut self, event: &InputEvent) {
        match event {
            InputEvent::AxisMoved { device_id, device_name, axis_code, value } => {
                let key = (device_id.clone(), *axis_code);

                if let Some(tracker) = self.axis_trackers.get_mut(&key) {
                    tracker.current_value = *value;
                } else {
                    self.axis_trackers.insert(key, AxisTracker {
                        device_id: device_id.clone(),
                        device_name: device_name.clone(),
                        axis_code: *axis_code,
                        initial_value: *value,
                        current_value: *value,
                    });
                }
            }
            InputEvent::ButtonPressed { device_id, device_name, button_code } => {
                // Once one gate is bound, every other gate has to come from the
                // same shifter. Without this a stray wheel press during a gate
                // step binds that gear to the wheel, and because a gate that
                // reports nothing looks identical to one nobody touched, the
                // mistake is silent -- which is how reverse ended up on the G29.
                if let Some(ref want) = self.shifter_device() {
                    if device_id != want {
                        return;
                    }
                }
                self.captured_button = Some((
                    device_id.clone(),
                    device_name.clone(),
                    *button_code,
                ));
            }
            _ => {}
        }
    }

    /// The device the H-shifter is on, once a gate has established it.
    ///
    /// Only constrains gate steps; every other button step is free to come from
    /// whichever device the driver presses.
    fn shifter_device(&self) -> Option<String> {
        if !matches!(self.step, CalibrationStep::Gear(_)) {
            return None;
        }
        self.config.gears.first().map(|g| g.button.device_id.clone())
    }

    /// The device a pedal or steering axis should be read from.
    ///
    /// On Windows every analog button arrives as a pseudo-axis, so slotting the
    /// H-shifter mid-step can out-move a pedal and hijack the binding. Once
    /// steering is bound, prefer that device for the remaining axis steps.
    fn preferred_axis_device(&self) -> Option<&str> {
        self.config.steering.as_ref().map(|s| s.device_id.as_str())
    }

    fn get_most_moved_axis(&self) -> Option<&AxisTracker> {
        let best = |device: Option<&str>| {
            self.axis_trackers
                .values()
                .filter(|a| device.map(|want| a.device_id == want).unwrap_or(true))
                .max_by(|a, b| a.movement().partial_cmp(&b.movement()).unwrap())
                .filter(|a| a.movement() > 0.025)
        };

        best(self.preferred_axis_device()).or_else(|| best(None))
    }

    pub fn capture_axis_position(&mut self) {
        if let Some(tracker) = self.get_most_moved_axis() {
            self.captured_axis = Some((
                tracker.device_id.clone(),
                tracker.device_name.clone(),
                tracker.axis_code,
                tracker.current_value,
            ));
        }
    }

    pub fn advance(&mut self) {
        self.capture_axis_position();

        match self.step {
            CalibrationStep::SteeringLeft => {
                if let Some((device_id, device_name, axis_code, value)) = self.captured_axis.take() {
                    self.config.steering = Some(AxisBinding {
                        device_id,
                        device_name,
                        axis_code,
                        min_value: value,
                        max_value: value, // Will be updated in SteeringRight so not rn
                        inverted: false,
                    });
                }
            }
            CalibrationStep::SteeringRight => {
                if let (Some(ref mut steering), Some((_, _, _, value))) =
                    (&mut self.config.steering, self.captured_axis.take())
                {
                    steering.max_value = value;
                    // Inverted? (left should be less than right)
                    if steering.min_value > steering.max_value {
                        std::mem::swap(&mut steering.min_value, &mut steering.max_value);
                        steering.inverted = true;
                    }
                }
            }
            CalibrationStep::ThrottlePressed => {
                if let Some((device_id, device_name, axis_code, value)) = self.captured_axis.take() {
                    self.config.throttle = Some(AxisBinding {
                        device_id,
                        device_name,
                        axis_code,
                        min_value: value, // Will be swapped if needed
                        max_value: value,
                        inverted: false,
                    });
                }
            }
            CalibrationStep::ThrottleReleased => {
                if let (Some(ref mut throttle), Some((_, _, _, value))) =
                    (&mut self.config.throttle, self.captured_axis.take())
                {
                    let pressed_value = throttle.max_value;
                    throttle.min_value = value;
                    throttle.max_value = pressed_value;

                    if throttle.min_value > throttle.max_value {
                        std::mem::swap(&mut throttle.min_value, &mut throttle.max_value);
                        throttle.inverted = true;
                    }
                }
            }
            CalibrationStep::BrakePressed => {
                if let Some((device_id, device_name, axis_code, value)) = self.captured_axis.take() {
                    self.config.brake = Some(AxisBinding {
                        device_id,
                        device_name,
                        axis_code,
                        min_value: value,
                        max_value: value,
                        inverted: false,
                    });
                }
            }
            CalibrationStep::BrakeReleased => {
                if let (Some(ref mut brake), Some((_, _, _, value))) =
                    (&mut self.config.brake, self.captured_axis.take())
                {
                    let pressed_value = brake.max_value;
                    brake.min_value = value;
                    brake.max_value = pressed_value;

                    if brake.min_value > brake.max_value {
                        std::mem::swap(&mut brake.min_value, &mut brake.max_value);
                        brake.inverted = true;
                    }
                }
            }
            CalibrationStep::ClutchPressed => {
                if let Some((device_id, device_name, axis_code, value)) = self.captured_axis.take() {
                    self.config.clutch = Some(AxisBinding {
                        device_id,
                        device_name,
                        axis_code,
                        min_value: value,
                        max_value: value,
                        inverted: false,
                    });
                }
            }
            CalibrationStep::ClutchReleased => {
                if let (Some(ref mut clutch), Some((_, _, _, value))) =
                    (&mut self.config.clutch, self.captured_axis.take())
                {
                    let pressed_value = clutch.max_value;
                    clutch.min_value = value;
                    clutch.max_value = pressed_value;

                    if clutch.min_value > clutch.max_value {
                        std::mem::swap(&mut clutch.min_value, &mut clutch.max_value);
                        clutch.inverted = true;
                    }
                }
            }
            CalibrationStep::ShiftUp => {
                if let Some((device_id, device_name, button_code)) = self.captured_button.take() {
                    self.config.shift_up = Some(ButtonBinding {
                        device_id,
                        device_name,
                        button_code,
                    });
                }
            }
            CalibrationStep::ShiftDown => {
                if let Some((device_id, device_name, button_code)) = self.captured_button.take() {
                    self.config.shift_down = Some(ButtonBinding {
                        device_id,
                        device_name,
                        button_code,
                    });
                }
            }
            CalibrationStep::Gear(gear) => {
                // The wizard walks every gate on every run, so a gate is exactly
                // what was captured this time. Leaving a stale binding in place
                // when nothing was captured is how a wrong gear survives a
                // recalibration that was meant to correct it.
                match self.captured_button.take() {
                    Some((device_id, device_name, button_code)) => {
                        self.config.set_gear(gear, ButtonBinding {
                            device_id,
                            device_name,
                            button_code,
                        });
                    }
                    None => self.config.clear_gear(gear),
                }
            }
            CalibrationStep::Camera => {
                if let Some((device_id, device_name, button_code)) = self.captured_button.take() {
                    self.config.camera = Some(ButtonBinding {
                        device_id,
                        device_name,
                        button_code,
                    });
                }
            }
            CalibrationStep::Recovery => {
                if let Some((device_id, device_name, button_code)) = self.captured_button.take() {
                    self.config.recovery = Some(ButtonBinding {
                        device_id,
                        device_name,
                        button_code,
                    });
                }
            }
            CalibrationStep::MirrorCamera => {
                if let Some((device_id, device_name, button_code)) = self.captured_button.take() {
                    self.config.mirror_camera = Some(ButtonBinding {
                        device_id,
                        device_name,
                        button_code,
                    });
                }
            }
            CalibrationStep::GearMode => {
                if let Some((device_id, device_name, button_code)) = self.captured_button.take() {
                    self.config.gear_mode = Some(ButtonBinding {
                        device_id,
                        device_name,
                        button_code,
                    });
                }
            }
            CalibrationStep::MenuConfirm => {
                if let Some((device_id, device_name, button_code)) = self.captured_button.take() {
                    self.config.menu_confirm = Some(ButtonBinding {
                        device_id,
                        device_name,
                        button_code,
                    });
                }
            }
            CalibrationStep::MenuCancel => {
                if let Some((device_id, device_name, button_code)) = self.captured_button.take() {
                    self.config.menu_cancel = Some(ButtonBinding {
                        device_id,
                        device_name,
                        button_code,
                    });
                }
            }
            // The config is written by `RoWheelApp::finish_calibration`, which
            // owns the Start button. `advance()` never reaches `Complete`.
            _ => {}
        }

        // Reset all the trackers for next step
        self.axis_trackers.clear();
        self.captured_axis = None;
        self.captured_button = None;

        self.step = self.step.next();
    }

    pub fn skip(&mut self) {
        // Skipping a gate means "my shifter does not have this one", so drop any
        // binding it had rather than keeping one the driver just disowned.
        if let CalibrationStep::Gear(gear) = self.step {
            self.config.clear_gear(gear);
        }

        self.axis_trackers.clear();
        self.captured_axis = None;
        self.captured_button = None;

        self.step = self.step.skip_target();
    }

    /// Get info about the detected axis for ui
    pub fn get_detected_axis_info(&self) -> Option<String> {
        self.get_most_moved_axis().map(|tracker| {
            format!(
                "{} - Axis {} (movement: {:.4})",
                tracker.device_name,
                tracker.axis_code,
                tracker.movement()
            )
        })
    }

    /// Get info about the detected button for ui
    pub fn get_detected_button_info(&self) -> Option<String> {
        self.captured_button.as_ref().map(|(_, name, code)| {
            format!("{} - Button {}", name, code)
        })
    }

    /// Are we in a step that needs axis detection?
    pub fn needs_axis_detection(&self) -> bool {
        matches!(
            self.step,
            CalibrationStep::SteeringLeft
                | CalibrationStep::SteeringRight
                | CalibrationStep::ThrottlePressed
                | CalibrationStep::ThrottleReleased
                | CalibrationStep::BrakePressed
                | CalibrationStep::BrakeReleased
                | CalibrationStep::ClutchPressed
                | CalibrationStep::ClutchReleased
        )
    }

    /// Are we in a step that needs button detection?
    pub fn needs_button_detection(&self) -> bool {
        matches!(
            self.step,
            CalibrationStep::ShiftUp
                | CalibrationStep::ShiftDown
                | CalibrationStep::Gear(_)
                | CalibrationStep::Camera
                | CalibrationStep::Recovery
                | CalibrationStep::MirrorCamera
                | CalibrationStep::GearMode
                | CalibrationStep::MenuConfirm
                | CalibrationStep::MenuCancel
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk the wizard the way the Next button does and collect every stop.
    fn walk() -> Vec<CalibrationStep> {
        let mut steps = vec![CalibrationStep::Welcome];
        let mut step = CalibrationStep::Welcome;
        for _ in 0..100 {
            let next = step.next();
            if next == step {
                break;
            }
            steps.push(next);
            step = next;
        }
        steps
    }

    /// Spartan cabinets have a wheel, two pedals and two paddles. Anything else in
    /// the walk is a step the operator has no hardware to answer and has to click
    /// past; anything missing is a control the wheel will not answer at all.
    #[test]
    fn the_wizard_asks_for_the_four_spartan_controls_and_nothing_else() {
        assert_eq!(
            walk(),
            vec![
                CalibrationStep::Welcome,
                CalibrationStep::SteeringLeft,
                CalibrationStep::SteeringRight,
                CalibrationStep::ThrottlePressed,
                CalibrationStep::ThrottleReleased,
                CalibrationStep::BrakePressed,
                CalibrationStep::BrakeReleased,
                CalibrationStep::ShiftUp,
                CalibrationStep::ShiftDown,
                CalibrationStep::Complete,
            ]
        );
    }

    /// The wizard no longer visits the clutch, the gates or the booth buttons, so
    /// it cannot clear them either. A config left behind by an IC build would keep
    /// driving the dpad, A/B and R3 on a cabinet that has none of those controls.
    #[test]
    fn loading_an_ic_config_drops_everything_outside_the_four_controls() {
        let mut config = WheelConfig::default();
        let binding = ButtonBinding {
            device_id: "wheel".to_string(),
            device_name: "wheel".to_string(),
            button_code: 3,
        };
        config.clutch = Some(AxisBinding {
            device_id: "pedals".to_string(),
            device_name: "pedals".to_string(),
            axis_code: 2,
            min_value: 0.0,
            max_value: 1.0,
            inverted: false,
        });
        config.set_gear(1, binding.clone());
        config.set_gear(GEAR_REVERSE, binding.clone());
        config.camera = Some(binding.clone());
        config.recovery = Some(binding.clone());
        config.mirror_camera = Some(binding.clone());
        config.gear_mode = Some(binding.clone());
        config.menu_confirm = Some(binding.clone());
        config.menu_cancel = Some(binding.clone());

        config.keep_spartan_bindings_only();

        assert!(config.clutch.is_none(), "clutch survived");
        assert!(config.gears.is_empty(), "gates survived: {:?}", config.gears);
        assert!(config.camera.is_none(), "camera survived");
        assert!(config.recovery.is_none(), "recovery survived");
        assert!(config.mirror_camera.is_none(), "mirror camera survived");
        assert!(config.gear_mode.is_none(), "gear mode survived");
        assert!(config.menu_confirm.is_none(), "menu confirm survived");
        assert!(config.menu_cancel.is_none(), "menu cancel survived");
    }

    fn button(device: &str, code: u32) -> InputEvent {
        InputEvent::ButtonPressed {
            device_id: device.to_string(),
            device_name: device.to_string(),
            button_code: code,
        }
    }

    #[test]
    fn a_gate_will_not_bind_to_a_different_device_than_the_shifter() {
        let mut wizard = CalibrationWizard::new(None);
        wizard.step = CalibrationStep::Gear(1);
        wizard.process_event(&button("shifter", 5));
        wizard.advance();

        // A wheel press during the next gate must be ignored, not bound.
        assert_eq!(wizard.step, CalibrationStep::Gear(2));
        wizard.process_event(&button("wheel", 7));
        wizard.advance();

        let gears: Vec<i8> = wizard.config.gears.iter().map(|g| g.gear).collect();
        assert_eq!(gears, vec![1], "a wheel press leaked into a gate: {:?}", gears);
    }

    #[test]
    fn skipping_a_gate_drops_a_binding_it_had_before() {
        let mut wizard = CalibrationWizard::new(None);
        wizard.step = CalibrationStep::Gear(1);
        wizard.process_event(&button("shifter", 5));
        wizard.advance();
        assert_eq!(wizard.config.gears.len(), 1);

        wizard.step = CalibrationStep::Gear(1);
        wizard.skip();
        assert!(wizard.config.gears.is_empty(), "skip left a stale binding");
    }

    #[test]
    fn advancing_a_gate_with_nothing_pressed_drops_a_stale_binding() {
        let mut wizard = CalibrationWizard::new(None);
        wizard.step = CalibrationStep::Gear(1);
        wizard.process_event(&button("shifter", 5));
        wizard.advance();

        wizard.step = CalibrationStep::Gear(1);
        wizard.advance();
        assert!(
            wizard.config.gears.is_empty(),
            "a gate nobody engaged kept its old binding"
        );
    }

    #[test]
    fn step_indexes_are_unique_and_in_range() {
        let steps = walk();
        let mut seen = Vec::new();
        for step in &steps {
            let index = step.index();
            assert!(
                index < CalibrationStep::TOTAL_STEPS,
                "{} index {} exceeds TOTAL_STEPS",
                step.title(),
                index
            );
            assert!(!seen.contains(&index), "duplicate index {} at {}", index, step.title());
            seen.push(index);
        }
        assert_eq!(steps.len(), CalibrationStep::TOTAL_STEPS, "walk length vs TOTAL_STEPS");
    }

    /// A stick at rest must not look like a gear, or a plain gamepad -- or
    /// RoWheel with no shifter bound -- would silently select one.
    #[test]
    fn every_gear_is_clear_of_a_resting_stick_and_of_each_other() {
        let step = crate::config::GEAR_AXIS_STEP;
        let mut previous: Option<f32> = None;
        for gear in crate::config::GEAR_REVERSE..=GEAR_MAX {
            let x = crate::config::encode_gear(gear);
            assert!(
                x >= step * 1.5,
                "gear {} encodes to {}, too close to a resting stick",
                gear,
                x
            );
            assert!(x <= 1.0, "gear {} encodes to {}, past full deflection", gear, x);
            if let Some(prev) = previous {
                let gap = x - prev;
                assert!(
                    (gap - step).abs() < 1e-4,
                    "gear {} sits {} from the previous gate, expected {}",
                    gear,
                    gap,
                    step
                );
            }
            previous = Some(x);
        }
    }
}
