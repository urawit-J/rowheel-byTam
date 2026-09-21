use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_FILENAME: &str = "rowheel_config.json";

/// Gear index space, matching the mka GXZ truck's `Tune.Ratios`:
/// -1 = reverse, 0 = neutral, 1..=7 = forward gears.
pub const GEAR_REVERSE: i8 = -1;
pub const GEAR_NEUTRAL: i8 = 0;
pub const GEAR_MAX: i8 = 7;

/// Gear is transported to Roblox on `right_stick_x`, because nine gear states do
/// not fit in the remaining gamepad buttons.
///
/// It used to ride both right-stick axes, with y held at 0.6 as a "this axis
/// carries gear data" flag. That flag never arrived: with the game logging the
/// axis it received, x came back byte-exact on every gate while y read 0.000
/// every single time. Whatever drops it sits below RoWheel -- the value is set
/// in `app.rs`, mapped in `virtual_controller`, and x from the same struct
/// survives -- so the flag now lives in x instead of on a channel that does not
/// work.
///
/// Every gear maps to a distinct positive value, `GEAR_AXIS_STEP` apart, the
/// smallest being reverse at 0.2. A stick at rest reads 0.0 and so decodes to no
/// gear at all, which is what a plain gamepad or a RoWheel with no shifter bound
/// must look like.
pub const GEAR_AXIS_STEP: f32 = 0.1;
/// Slot number of gear 0 (neutral); reverse is one slot below it.
pub const GEAR_AXIS_BASE: i8 = 3;

/// Encode a gear index as `right_stick_x`.
pub fn encode_gear(gear: i8) -> f32 {
    (((gear + GEAR_AXIS_BASE) as f32) * GEAR_AXIS_STEP).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxisBinding {
    /// Stable per-device key: the SDL-style UUID reported by gilrs, or the
    /// device name when the driver reports a nil UUID.
    pub device_id: String,
    pub device_name: String,
    pub axis_code: u32,
    pub min_value: f32,
    pub max_value: f32,
    pub inverted: bool,
}

impl AxisBinding {
    /// Normalize to -1.0..1.0 range based on calibration
    pub fn normalize(&self, raw_value: f32) -> f32 {
        let range = (self.max_value - self.min_value) as f64;
        if range.abs() < 0.001 {
            return 0.0;
        }
        let normalized = ((raw_value - self.min_value) as f64) / range * 2.0 - 1.0;
        let normalized = normalized.clamp(-1.0, 1.0);
        let result = if self.inverted {
            -normalized
        } else {
            normalized
        };
        result as f32
    }

    /// Normalize between 0 and 1 just in case scaling is weird
    pub fn normalize_trigger(&self, raw_value: f32) -> f32 {
        let range = (self.max_value - self.min_value) as f64;
        if range.abs() < 0.001 {
            return 0.0;
        }
        let normalized = ((raw_value - self.min_value) as f64) / range;
        let normalized = normalized.clamp(0.0, 1.0);
        let result = if self.inverted {
            1.0 - normalized
        } else {
            normalized
        };
        result as f32
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonBinding {
    /// Stable per-device key, same scheme as `AxisBinding::device_id`.
    pub device_id: String,
    pub device_name: String,
    pub button_code: u32,
}

/// One slot of an H-pattern shifter. The Moza HGP reports every gate position
/// as its own HID button, so each gear gets a plain `ButtonBinding`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GearBinding {
    /// -1 = reverse, 1..=7 forward. Neutral is "no gear button pressed".
    pub gear: i8,
    #[serde(flatten)]
    pub button: ButtonBinding,
}

fn default_clutch_engage() -> f32 {
    0.55
}

fn default_clutch_release() -> f32 {
    0.40
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WheelConfig {
    pub steering: Option<AxisBinding>,
    pub throttle: Option<AxisBinding>,
    pub brake: Option<AxisBinding>,
    /// A-Chassis has no analog clutch; this axis is thresholded into a button.
    pub clutch: Option<AxisBinding>,
    /// Pedal travel at which the clutch counts as pressed.
    #[serde(default = "default_clutch_engage")]
    pub clutch_engage: f32,
    /// Travel at which it releases again. Lower than `clutch_engage` so a foot
    /// resting near the bite point does not chatter the button.
    #[serde(default = "default_clutch_release")]
    pub clutch_release: f32,
    pub shift_up: Option<ButtonBinding>,
    pub shift_down: Option<ButtonBinding>,
    /// H-pattern shifter gate positions. Empty means "no shifter": the gear
    /// axis stays at rest and the game ignores it.
    #[serde(default)]
    pub gears: Vec<GearBinding>,
    /// Camera toggle (game key `V`).
    #[serde(default)]
    pub camera: Option<ButtonBinding>,
    /// Recovery / return to checkpoint (game key `R`).
    #[serde(default)]
    pub recovery: Option<ButtonBinding>,
    /// Parking aid overlay (game key `T`).
    #[serde(default)]
    pub parking_aid: Option<ButtonBinding>,
    /// Transmission mode toggle (game key `M`).
    pub gear_mode: Option<ButtonBinding>,
    pub force_feedback_device: Option<String>,
}

impl Default for WheelConfig {
    fn default() -> Self {
        Self {
            steering: None,
            throttle: None,
            brake: None,
            clutch: None,
            clutch_engage: default_clutch_engage(),
            clutch_release: default_clutch_release(),
            shift_up: None,
            shift_down: None,
            gears: Vec::new(),
            camera: None,
            recovery: None,
            parking_aid: None,
            gear_mode: None,
            force_feedback_device: None,
        }
    }
}

impl WheelConfig {
    pub fn load() -> Option<Self> {
        let path = Self::config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str(&contents) {
                    Ok(config) => {
                        log::info!("Loaded config from {:?}", path);
                        return Some(config);
                    }
                    Err(e) => {
                        log::error!("Failed to parse config: {}", e);
                    }
                },
                Err(e) => {
                    log::error!("Failed to read config file: {}", e);
                }
            }
        }
        None
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path();
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;
        log::info!("Saved config to {:?}", path);
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(CONFIG_FILENAME)
    }

    pub fn is_complete(&self) -> bool {
        self.steering.is_some()
            && self.throttle.is_some()
            && self.brake.is_some()
            && self.shift_up.is_some()
            && self.shift_down.is_some()
    }

    /// Replace the binding for one gate position, so recalibrating a gear does
    /// not leave a stale duplicate behind.
    pub fn set_gear(&mut self, gear: i8, button: ButtonBinding) {
        self.gears.retain(|g| g.gear != gear);
        self.gears.push(GearBinding { gear, button });
        self.gears.sort_by_key(|g| g.gear);
    }

    /// Forget one gate, for a shifter that does not have it.
    pub fn clear_gear(&mut self, gear: i8) {
        self.gears.retain(|g| g.gear != gear);
    }
}
