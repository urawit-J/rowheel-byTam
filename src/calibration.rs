use crate::config::{
    AxisBinding, ButtonBinding, WheelConfig, GEAR_MAX, GEAR_REVERSE, HAT_THRESHOLD,
};
use crate::input::InputEvent;
use std::collections::HashMap;

/// A place on the wheel photo (`assets/g29.jpg`), as fractions of the image so
/// nothing downstream has to know what size it is drawn at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

const fn spot(x: f32, y: f32, w: f32, h: f32) -> Spot {
    Spot { x, y, w, h }
}

// Measured off the 2000x2000 photo, then checked by drawing every one of them
// back onto it. The first photo was 500px wide and the lettering on the blue
// buttons could not be read at that size; this one is large enough to place
// L2/L3/R2/R3 individually, which is why it replaced it.
const WHEEL: Spot = spot(0.020, 0.222, 0.518, 0.510);
const PADDLE_LEFT: Spot = spot(0.105, 0.368, 0.049, 0.196);
const PADDLE_RIGHT: Spot = spot(0.397, 0.368, 0.050, 0.196);
// Left to right on the pedal unit, which is the order a G29 ships in.
const PEDAL_CLUTCH: Spot = spot(0.577, 0.398, 0.084, 0.134);
const PEDAL_BRAKE: Spot = spot(0.692, 0.398, 0.105, 0.134);
const PEDAL_THROTTLE: Spot = spot(0.880, 0.398, 0.084, 0.136);
const FACE_CIRCLE: Spot = spot(0.380, 0.414, 0.026, 0.026);
const FACE_CROSS: Spot = spot(0.359, 0.434, 0.026, 0.026);
// The upper button of each pair is L2/R2 and the lower one L3/R3.
const BLUE_L2: Spot = spot(0.166, 0.474, 0.028, 0.024);
const BLUE_R2: Spot = spot(0.347, 0.474, 0.028, 0.024);
const BLUE_L3: Spot = spot(0.132, 0.510, 0.028, 0.024);
const BLUE_R3: Spot = spot(0.382, 0.510, 0.028, 0.024);

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
    MenuConfirm,
    MenuCancel,
    GearMode,
    Complete,
}

impl CalibrationStep {
    pub const TOTAL_STEPS: usize = 26;

    pub fn index(&self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::SteeringLeft => 1,
            Self::SteeringRight => 2,
            Self::ThrottlePressed => 3,
            Self::ThrottleReleased => 4,
            Self::BrakePressed => 5,
            Self::BrakeReleased => 6,
            Self::ClutchPressed => 7,
            Self::ClutchReleased => 8,
            Self::ShiftUp => 9,
            Self::ShiftDown => 10,
            // Reverse is asked last, so the forward gates establish which device
            // the shifter is before the hardest gate to engage comes up.
            Self::Gear(GEAR_REVERSE) => 18,
            Self::Gear(g) => 10 + (*g as usize),
            Self::Camera => 19,
            Self::Recovery => 20,
            Self::MirrorCamera => 21,
            Self::MenuConfirm => 22,
            Self::MenuCancel => 23,
            Self::GearMode => 24,
            Self::Complete => 25,
        }
    }

    pub fn title(&self) -> String {
        match self {
            Self::Welcome => "เริ่มต้น".to_string(),
            Self::SteeringLeft | Self::SteeringRight => "พวงมาลัย".to_string(),
            Self::ThrottlePressed | Self::ThrottleReleased => "คันเร่ง".to_string(),
            Self::BrakePressed | Self::BrakeReleased => "เบรก".to_string(),
            Self::ClutchPressed | Self::ClutchReleased => "คลัตช์".to_string(),
            Self::ShiftUp => "เปลี่ยนเกียร์ขึ้น".to_string(),
            Self::ShiftDown => "เปลี่ยนเกียร์ลง".to_string(),
            Self::Gear(GEAR_REVERSE) => "คันเกียร์ - เกียร์ถอย".to_string(),
            Self::Gear(g) => format!("คันเกียร์ - เกียร์ {}", g),
            Self::Camera => "ปุ่มเปลี่ยนมุมกล้อง".to_string(),
            Self::Recovery => "ปุ่มกู้รถ".to_string(),
            Self::MirrorCamera => "ปุ่มกระจกมองข้าง".to_string(),
            Self::MenuConfirm => "ปุ่มยืนยันในเมนู".to_string(),
            Self::MenuCancel => "ปุ่มย้อนกลับในเมนู".to_string(),
            Self::GearMode => "ปุ่มสลับโหมดเกียร์".to_string(),
            Self::Complete => "เสร็จแล้ว".to_string(),
        }
    }

    pub fn instructions(&self) -> String {
        match self {
            Self::Welcome => "ตรวจดูว่าพวงมาลัย แป้นเหยียบ และคันเกียร์ ต่ออยู่ครบแล้ว".to_string(),
            Self::SteeringLeft => "หมุนพวงมาลัยไปทางซ้ายจนสุด แล้วกดถัดไป".to_string(),
            Self::SteeringRight => "หมุนพวงมาลัยไปทางขวาจนสุด แล้วกดถัดไป".to_string(),
            Self::ThrottlePressed => "เหยียบคันเร่งจนสุด แล้วกดถัดไป".to_string(),
            Self::ThrottleReleased => "ปล่อยคันเร่งให้สุด แล้วกดถัดไป".to_string(),
            Self::BrakePressed => "เหยียบเบรกจนสุด แล้วกดถัดไป".to_string(),
            Self::BrakeReleased => "ปล่อยเบรกให้สุด แล้วกดถัดไป".to_string(),
            Self::ClutchPressed => "เหยียบคลัตช์จนสุด แล้วกดถัดไป\n(ถ้าชุดนี้ไม่มีคลัตช์ ให้กดข้าม)".to_string(),
            Self::ClutchReleased => "ปล่อยคลัตช์ให้สุด แล้วกดถัดไป".to_string(),
            Self::ShiftUp => {
                "กด paddle shift ขวา แล้วกดถัดไป\n(ปุ่มนี้ใช้เลื่อนไปตัวเลือกถัดไปในเมนูตู้ด้วย)".to_string()
            }
            Self::ShiftDown => {
                "กด paddle shift ซ้าย แล้วกดถัดไป\n(ปุ่มนี้ใช้เลื่อนกลับตัวเลือกก่อนหน้าด้วย)".to_string()
            }
            Self::Gear(GEAR_REVERSE) => {
                "เข้าเกียร์ถอยบนคันเกียร์ แล้วกดถัดไป\n(ช่องไหนคันเกียร์ไม่มี ให้กดข้าม)".to_string()
            }
            Self::Gear(g) => format!(
                "เข้าเกียร์ {} บนคันเกียร์ แล้วกดถัดไป\n(ช่องไหนคันเกียร์ไม่มี ให้กดข้าม)",
                g
            ),
            Self::Camera => "กดปุ่มเปลี่ยนมุมกล้อง (R3 บนพวงมาลัย) แล้วกดถัดไป".to_string(),
            Self::Recovery => "กดปุ่มกู้รถ (L3 บนพวงมาลัย) แล้วกดถัดไป".to_string(),
            Self::MirrorCamera => "กดปุ่มกระจกมองข้าง (L2 บนพวงมาลัย) แล้วกดถัดไป".to_string(),
            Self::MenuConfirm => "กดปุ่มยืนยัน (ปุ่ม × บนพวงมาลัย) แล้วกดถัดไป".to_string(),
            Self::MenuCancel => "กดปุ่มย้อนกลับ (ปุ่ม ○ บนพวงมาลัย) แล้วกดถัดไป".to_string(),
            Self::GearMode => "กดปุ่มสลับโหมดเกียร์ (R2 บนพวงมาลัย) แล้วกดถัดไป".to_string(),
            Self::Complete => "ตั้งค่าเรียบร้อยแล้ว".to_string(),
        }
    }

    /// Where to ring the wheel photo for this step. The operator setting the
    /// booth up is not the person who designed the mapping, so the words alone
    /// are not enough -- "L2" means nothing until you can see which one it is.
    ///
    /// Empty when the photo has nothing to show, and the wizard then draws no
    /// photo rather than one with no mark on it.
    pub fn spots(&self) -> &'static [Spot] {
        match self {
            Self::SteeringLeft | Self::SteeringRight => &[WHEEL],
            Self::ThrottlePressed | Self::ThrottleReleased => &[PEDAL_THROTTLE],
            Self::BrakePressed | Self::BrakeReleased => &[PEDAL_BRAKE],
            Self::ClutchPressed | Self::ClutchReleased => &[PEDAL_CLUTCH],
            Self::ShiftUp => &[PADDLE_RIGHT],
            Self::ShiftDown => &[PADDLE_LEFT],
            Self::Camera => &[BLUE_R3],
            Self::GearMode => &[BLUE_R2],
            Self::Recovery => &[BLUE_L3],
            Self::MirrorCamera => &[BLUE_L2],
            Self::MenuConfirm => &[FACE_CROSS],
            Self::MenuCancel => &[FACE_CIRCLE],
            // The H-shifter is a separate box, and the first and last screens
            // are not asking for a control at all.
            Self::Welcome | Self::Gear(_) | Self::Complete => &[],
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
            Self::BrakeReleased => Self::ClutchPressed,
            Self::ClutchPressed => Self::ClutchReleased,
            Self::ClutchReleased => Self::ShiftUp,
            Self::ShiftUp => Self::ShiftDown,
            Self::ShiftDown => Self::Gear(1),
            Self::Gear(GEAR_REVERSE) => Self::Camera,
            Self::Gear(g) if *g >= GEAR_MAX => Self::Gear(GEAR_REVERSE),
            Self::Gear(g) => Self::Gear(g + 1),
            Self::Camera => Self::Recovery,
            Self::Recovery => Self::MirrorCamera,
            Self::MirrorCamera => Self::MenuConfirm,
            Self::MenuConfirm => Self::MenuCancel,
            Self::MenuCancel => Self::GearMode,
            Self::GearMode => Self::Complete,
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

    /// The step before this one, for the Back button -- bind the wrong control,
    /// press Next, and this is the way back to it.
    ///
    /// Found by walking `next()` from the start rather than written out as a
    /// second table. The forward order is not the obvious one -- reverse is
    /// asked after seventh, so Camera comes back to Gear(REVERSE) and
    /// Gear(REVERSE) back to Gear(GEAR_MAX) -- and a hand-written inverse would
    /// be one edit away from disagreeing with it for ever. 27 steps, so the
    /// walk costs nothing.
    pub fn previous(&self) -> Self {
        let mut step = Self::Welcome;
        loop {
            let next = step.next();
            if next == *self {
                return step;
            }
            // `Complete.next()` is itself, so the walk has run out without
            // finding a predecessor: Welcome has none, and neither would a step
            // no walk reaches. Stay put rather than jump somewhere arbitrary --
            // returning the last step walked would send Welcome to Complete.
            if next == step {
                return *self;
            }
            step = next;
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

    captured_button: Option<ButtonBinding>,
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
            InputEvent::AxisMoved {
                device_id,
                device_name,
                axis_code,
                value,
            } => {
                let key = (device_id.clone(), *axis_code);

                if let Some(tracker) = self.axis_trackers.get_mut(&key) {
                    tracker.current_value = *value;
                } else {
                    self.axis_trackers.insert(
                        key,
                        AxisTracker {
                            device_id: device_id.clone(),
                            device_name: device_name.clone(),
                            axis_code: *axis_code,
                            initial_value: *value,
                            current_value: *value,
                        },
                    );
                }
            }
            InputEvent::ButtonPressed {
                device_id,
                device_name,
                button_code,
            } => {
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
                self.captured_button = Some(ButtonBinding {
                    device_id: device_id.clone(),
                    device_name: device_name.clone(),
                    button_code: *button_code,
                    axis_direction: None,
                });
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
        self.config
            .gears
            .first()
            .map(|g| g.button.device_id.clone())
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

    /// The hat direction currently held over, if any.
    ///
    /// A wheel's D-pad is a POV hat: it reaches gilrs as an axis that snaps
    /// between -1, 0 and +1, never as four buttons. Waiting only for a
    /// ButtonPressed leaves those directions impossible to bind, which is what
    /// made the G29 D-pad look like dead hardware. Require both a real movement
    /// and a held deflection so a pedal resting off-centre cannot qualify.
    fn held_hat(&self) -> Option<ButtonBinding> {
        let tracker = self.get_most_moved_axis()?;
        if tracker.movement() < HAT_THRESHOLD || tracker.current_value.abs() < HAT_THRESHOLD {
            return None;
        }
        Some(ButtonBinding {
            device_id: tracker.device_id.clone(),
            device_name: tracker.device_name.clone(),
            button_code: tracker.axis_code,
            axis_direction: Some(if tracker.current_value < 0.0 { -1 } else { 1 }),
        })
    }

    /// What a button step captured: a real button if one was pressed, otherwise
    /// a hat direction.
    fn take_button_or_hat(&mut self) -> Option<ButtonBinding> {
        if let Some(binding) = self.captured_button.take() {
            return Some(binding);
        }
        self.held_hat()
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
                if let Some((device_id, device_name, axis_code, value)) = self.captured_axis.take()
                {
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
                if let Some((device_id, device_name, axis_code, value)) = self.captured_axis.take()
                {
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
                if let Some((device_id, device_name, axis_code, value)) = self.captured_axis.take()
                {
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
                if let Some((device_id, device_name, axis_code, value)) = self.captured_axis.take()
                {
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
                if let Some(binding) = self.take_button_or_hat() {
                    self.config.shift_up = Some(binding);
                }
            }
            CalibrationStep::ShiftDown => {
                if let Some(binding) = self.take_button_or_hat() {
                    self.config.shift_down = Some(binding);
                }
            }
            CalibrationStep::Gear(gear) => {
                // The wizard walks every gate on every run, so a gate is exactly
                // what was captured this time. Leaving a stale binding in place
                // when nothing was captured is how a wrong gear survives a
                // recalibration that was meant to correct it.
                match self.take_button_or_hat() {
                    Some(binding) => {
                        self.config.set_gear(gear, binding);
                    }
                    None => self.config.clear_gear(gear),
                }
            }
            CalibrationStep::Camera => {
                if let Some(binding) = self.take_button_or_hat() {
                    self.config.camera = Some(binding);
                }
            }
            CalibrationStep::Recovery => {
                if let Some(binding) = self.take_button_or_hat() {
                    self.config.recovery = Some(binding);
                }
            }
            CalibrationStep::MirrorCamera => {
                if let Some(binding) = self.take_button_or_hat() {
                    self.config.mirror_camera = Some(binding);
                }
            }
            CalibrationStep::MenuConfirm => {
                if let Some(binding) = self.take_button_or_hat() {
                    self.config.menu_confirm = Some(binding);
                }
            }
            CalibrationStep::MenuCancel => {
                if let Some(binding) = self.take_button_or_hat() {
                    self.config.menu_cancel = Some(binding);
                }
            }
            CalibrationStep::GearMode => {
                if let Some(binding) = self.take_button_or_hat() {
                    self.config.gear_mode = Some(binding);
                }
            }
            // The config is written by `RoWheelApp::finish_calibration`, which
            // owns the Start button. `advance()` never reaches `Complete`.
            _ => {}
        }

        self.reset_detection();
        self.step = self.step.next();
    }

    pub fn skip(&mut self) {
        // Skipping a gate means "my shifter does not have this one", so drop any
        // binding it had rather than keeping one the driver just disowned.
        if let CalibrationStep::Gear(gear) = self.step {
            self.config.clear_gear(gear);
        }

        self.reset_detection();
        self.step = self.step.skip_target();
    }

    /// Return to the step before this one so a control bound by mistake can be
    /// bound again. The binding it already wrote is left alone: pressing the
    /// right control and advancing overwrites it, which is the whole point, and
    /// clearing it here would also punish someone who only wanted another look
    /// at the instructions.
    pub fn back(&mut self) {
        self.reset_detection();
        self.step = self.step.previous();
    }

    /// Whatever the last step saw must not be read as this step's answer.
    fn reset_detection(&mut self) {
        self.axis_trackers.clear();
        self.captured_axis = None;
        self.captured_button = None;
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
        let binding = match self.captured_button.as_ref() {
            Some(binding) => binding.clone(),
            // Nothing pressed, but a hat may be held over. Show it, or a D-pad
            // looks like dead hardware right up until the binding is saved.
            None => self.held_hat()?,
        };
        Some(format!("{} - {}", binding.device_name, binding.describe()))
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
                | CalibrationStep::MenuConfirm
                | CalibrationStep::MenuCancel
                | CalibrationStep::GearMode
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

    /// Asserted on the steps rather than their titles, so translating the UI
    /// cannot quietly turn this into a test of nothing.
    #[test]
    fn wizard_offers_every_gate_including_reverse() {
        let steps = walk();
        assert!(
            steps.contains(&CalibrationStep::Gear(GEAR_REVERSE)),
            "reverse gate missing from the wizard: {:?}",
            steps
        );
        for gear in 1..=GEAR_MAX {
            assert!(
                steps.contains(&CalibrationStep::Gear(gear)),
                "gate {} missing: {:?}",
                gear,
                steps
            );
        }
    }

    /// Reverse is the gate most likely to report nothing, so it must be asked
    /// after the forward gates have established which device the shifter is.
    #[test]
    fn reverse_is_the_last_gate() {
        let steps = walk();
        let reverse = steps
            .iter()
            .position(|s| *s == CalibrationStep::Gear(GEAR_REVERSE))
            .expect("reverse step");
        let top = steps
            .iter()
            .position(|s| *s == CalibrationStep::Gear(GEAR_MAX))
            .expect("top gear step");
        assert!(reverse > top, "reverse must come after gear {}", GEAR_MAX);
        assert_eq!(steps[reverse + 1], CalibrationStep::Camera);
    }

    fn button(device: &str, code: u32) -> InputEvent {
        InputEvent::ButtonPressed {
            device_id: device.to_string(),
            device_name: device.to_string(),
            button_code: code,
        }
    }

    fn axis(device: &str, code: u32, value: f32) -> InputEvent {
        InputEvent::AxisMoved {
            device_id: device.to_string(),
            device_name: device.to_string(),
            axis_code: code,
            value,
        }
    }

    /// Some controls report as a POV hat, which never produces a ButtonPressed.
    /// A button step has to bind those from the axis, keeping the direction.
    #[test]
    fn a_button_step_binds_a_hat_direction_when_no_button_is_pressed() {
        let mut wizard = CalibrationWizard::new(None);
        wizard.step = CalibrationStep::Camera;
        wizard.process_event(&axis("wheel", 9, 0.0));
        wizard.process_event(&axis("wheel", 9, -1.0));
        wizard.advance();

        let binding = wizard.config.camera.expect("hat bound");
        assert_eq!(binding.button_code, 9);
        assert_eq!(binding.axis_direction, Some(-1));

        // Held the other way, the same axis is a different control.
        let mut other = CalibrationWizard::new(None);
        other.step = CalibrationStep::MirrorCamera;
        other.process_event(&axis("wheel", 9, 0.0));
        other.process_event(&axis("wheel", 9, 1.0));
        other.advance();
        assert_eq!(
            other
                .config
                .mirror_camera
                .expect("hat bound")
                .axis_direction,
            Some(1)
        );
    }

    /// An axis that drifts but is not held over is not a hat press, or a pedal
    /// resting off-centre would bind itself to whatever step is open.
    #[test]
    fn a_lightly_moved_axis_is_not_taken_for_a_hat() {
        let mut wizard = CalibrationWizard::new(None);
        wizard.step = CalibrationStep::Camera;
        wizard.process_event(&axis("pedals", 2, 0.0));
        wizard.process_event(&axis("pedals", 2, 0.3));
        wizard.advance();
        assert!(wizard.config.camera.is_none());
    }

    /// A real button still wins: the hat is only the fallback.
    #[test]
    fn a_pressed_button_beats_a_held_hat() {
        let mut wizard = CalibrationWizard::new(None);
        wizard.step = CalibrationStep::Camera;
        wizard.process_event(&axis("wheel", 9, 0.0));
        wizard.process_event(&axis("wheel", 9, -1.0));
        wizard.process_event(&button("wheel", 4));
        wizard.advance();

        let binding = wizard.config.camera.expect("button bound");
        assert_eq!(binding.button_code, 4);
        assert_eq!(binding.axis_direction, None);
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
        assert_eq!(
            gears,
            vec![1],
            "a wheel press leaked into a gate: {:?}",
            gears
        );
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
            assert!(
                !seen.contains(&index),
                "duplicate index {} at {}",
                index,
                step.title()
            );
            seen.push(index);
        }
        assert_eq!(
            steps.len(),
            CalibrationStep::TOTAL_STEPS,
            "walk length vs TOTAL_STEPS"
        );
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
            assert!(
                x <= 1.0,
                "gear {} encodes to {}, past full deflection",
                gear,
                x
            );
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

    /// Back has to land exactly where Next came from, at every step -- the
    /// whole wizard, not just the easy linear middle.
    #[test]
    fn back_undoes_next_at_every_step() {
        for step in walk() {
            let next = step.next();
            if next == step {
                continue; // Complete: the walk ends here
            }
            assert_eq!(
                next.previous(),
                step,
                "{:?} -> {:?} does not come back",
                step.title(),
                next.title()
            );
        }
    }

    /// The gates are not asked in their own order, so this is where a
    /// hand-written inverse would have gone wrong.
    #[test]
    fn back_follows_the_gates_in_the_order_they_were_asked() {
        assert_eq!(
            CalibrationStep::Gear(1).previous(),
            CalibrationStep::ShiftDown
        );
        assert_eq!(
            CalibrationStep::Gear(GEAR_REVERSE).previous(),
            CalibrationStep::Gear(GEAR_MAX),
            "reverse is asked after the top gear, so it goes back to it"
        );
        assert_eq!(
            CalibrationStep::Camera.previous(),
            CalibrationStep::Gear(GEAR_REVERSE)
        );
    }

    /// What the Exit button rests on: the wizard is seeded from the config
    /// already loaded, so leaving part-way writes back the bindings the walk
    /// never reached rather than blanking them.
    #[test]
    fn leaving_part_way_keeps_the_bindings_the_walk_never_reached() {
        let bound = |code: u32| ButtonBinding {
            device_id: "wheel".to_string(),
            device_name: "wheel".to_string(),
            button_code: code,
            axis_direction: None,
        };
        let mut existing = WheelConfig::default();
        existing.camera = Some(bound(99));
        existing.gear_mode = Some(bound(6));

        let mut wizard = CalibrationWizard::new(Some(existing));
        wizard.step = CalibrationStep::Camera;
        wizard.process_event(&button("wheel", 10));
        wizard.advance();

        assert_eq!(
            wizard.config.camera.map(|b| b.button_code),
            Some(10),
            "the step that was walked is rebound"
        );
        assert_eq!(
            wizard.config.gear_mode.map(|b| b.button_code),
            Some(6),
            "a step never reached keeps what it already had"
        );
    }

    /// A ring drawn off the edge of the photo points at nothing, and a typo in
    /// the table is the only way that happens.
    #[test]
    fn every_spot_lands_inside_the_photo() {
        for step in walk() {
            for spot in step.spots() {
                assert!(
                    spot.x >= 0.0
                        && spot.y >= 0.0
                        && spot.x + spot.w <= 1.0
                        && spot.y + spot.h <= 1.0,
                    "{} marks {:?}, which falls outside the photo",
                    step.title(),
                    spot
                );
            }
        }
    }

    /// Add a step that asks for something on the wheel and the photo has to
    /// learn where it is. The gates are the exception -- the H-shifter is a
    /// separate box that this photo does not show.
    #[test]
    fn every_step_that_asks_for_a_wheel_control_points_at_it() {
        for step in walk() {
            let photo_cannot_show_it = matches!(
                step,
                CalibrationStep::Welcome | CalibrationStep::Complete | CalibrationStep::Gear(_)
            );
            assert_eq!(
                step.spots().is_empty(),
                photo_cannot_show_it,
                "{} disagrees with the photo about whether it can be shown",
                step.title()
            );
        }
    }

    #[test]
    fn the_first_step_has_nowhere_to_go_back_to() {
        assert_eq!(
            CalibrationStep::Welcome.previous(),
            CalibrationStep::Welcome
        );
    }

    /// Back exists to undo a wrong binding, so the press that caused it must
    /// not still be sitting there when the earlier step reopens.
    #[test]
    fn going_back_forgets_what_the_last_step_detected() {
        let mut wizard = CalibrationWizard::new(None);
        wizard.step = CalibrationStep::MenuCancel;
        wizard.process_event(&button("wheel", 2));
        assert!(
            wizard.get_detected_button_info().is_some(),
            "the press has to register before there is anything to forget"
        );

        wizard.back();

        assert_eq!(wizard.step, CalibrationStep::MenuConfirm);
        assert!(
            wizard.get_detected_button_info().is_none(),
            "the last step's press must not be read as this step's answer"
        );
    }

    /// The reason the button exists: the wrong control is already written to
    /// the config, and going back and pressing the right one has to replace it.
    #[test]
    fn pressing_the_right_control_after_back_replaces_the_wrong_one() {
        let mut wizard = CalibrationWizard::new(None);
        wizard.step = CalibrationStep::MenuConfirm;
        wizard.process_event(&button("wheel", 9));
        wizard.advance();
        assert_eq!(
            wizard.config.menu_confirm.as_ref().map(|b| b.button_code),
            Some(9)
        );

        wizard.back();
        assert_eq!(wizard.step, CalibrationStep::MenuConfirm);
        wizard.process_event(&button("wheel", 0));
        wizard.advance();

        assert_eq!(wizard.config.menu_confirm.map(|b| b.button_code), Some(0));
    }
}
