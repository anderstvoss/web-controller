//! Single-owner adapter to the upstream Xbox 360-style uinput controller.
//! Network handlers may only publish full-state snapshots; they never own HID handles.
use std::{
    thread,
    time::{Duration, Instant},
};

use tokio::sync::{oneshot, watch};
use virtualgamepad::{
    CreationOptions, DigitalControlUpdate, DpadDirection, RealizationId, Xbox360Axis,
    Xbox360Control, Xbox360Controller, Xbox360Trigger, create_xbox360,
};

use crate::input::InputFrame;

const BUTTONS: [Xbox360Control; 11] = [
    Xbox360Control::A,
    Xbox360Control::B,
    Xbox360Control::X,
    Xbox360Control::Y,
    Xbox360Control::LeftShoulder,
    Xbox360Control::RightShoulder,
    Xbox360Control::Back,
    Xbox360Control::Start,
    Xbox360Control::Guide,
    Xbox360Control::LeftStickPress,
    Xbox360Control::RightStickPress,
];
const DPAD: [DpadDirection; 4] = [
    DpadDirection::Up,
    DpadDirection::Down,
    DpadDirection::Left,
    DpadDirection::Right,
];

/// Input silence beyond this duration must not leave any pressed button held.
const INPUT_TIMEOUT: Duration = Duration::from_millis(300);
/// Bounded idle servicing until readiness integration lands.
const MAX_SERVICE_WAIT: Duration = Duration::from_millis(2);

#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "clamped and rounded to the native axis range"
)]
pub fn stick_axis(value: f32) -> Xbox360Axis {
    Xbox360Axis::new((value.clamp(-1.0, 1.0) * 32767.0).round() as i16)
}

#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped and rounded to the unsigned trigger range"
)]
pub fn trigger_axis(value: f32) -> Xbox360Trigger {
    Xbox360Trigger::new((value.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn apply(device: &mut Xbox360Controller, frame: &InputFrame) -> Result<(), String> {
    for (index, control) in BUTTONS.iter().copied().enumerate() {
        device
            .set_native(control, frame.buttons[index])
            .map_err(|error| error.to_string())?;
    }
    for (index, direction) in DPAD.iter().copied().enumerate() {
        device
            .set_digital(DigitalControlUpdate::Dpad {
                direction,
                pressed: frame.buttons[BUTTONS.len() + index],
            })
            .map_err(|error| error.to_string())?;
    }
    device
        .set_left_stick(stick_axis(frame.sticks[0]), stick_axis(frame.sticks[1]))
        .map_err(|error| error.to_string())?;
    device
        .set_right_stick(stick_axis(frame.sticks[2]), stick_axis(frame.sticks[3]))
        .map_err(|error| error.to_string())?;
    device
        .set_triggers(
            trigger_axis(frame.triggers[0]),
            trigger_axis(frame.triggers[1]),
        )
        .map_err(|error| error.to_string())?;
    device.commit().map_err(|error| error.to_string())
}

#[expect(clippy::float_cmp, reason = "only identical snapshots are redundant")]
fn same_controls(lhs: &InputFrame, rhs: &InputFrame) -> bool {
    lhs.buttons == rhs.buttons && lhs.sticks == rhs.sticks && lhs.triggers == rhs.triggers
}

/// Execute on a dedicated blocking thread; no network IO or Tokio reactor work.
///
/// Sends one startup result before any input is applied. Always closes the
/// device on exit, including errors and closure of the snapshot channel.
///
/// # Errors
/// Returns an error if creation, input application, servicing or neutralization fails.
pub fn run_xbox_owner(
    mut input: watch::Receiver<Option<InputFrame>>,
    startup: oneshot::Sender<Result<(), String>>,
) -> Result<(), String> {
    let mut device = match create_xbox360(CreationOptions::new(RealizationId::LINUX_UINPUT)) {
        Ok(device) => device,
        Err(error) => {
            let message = error.to_string();
            let _ = startup.send(Err(message.clone()));
            return Err(message);
        }
    };
    let _ = startup.send(Ok(()));
    let mut last_controls: Option<InputFrame> = None;
    let mut last_seen = Instant::now();
    let mut stale = false;
    let mut outcome = Ok(());

    while let Ok(changed) = input.has_changed() {
        if changed {
            let frame = input.borrow_and_update().clone();
            if let Some(frame) = frame {
                last_seen = Instant::now();
                stale = false;
                if last_controls
                    .as_ref()
                    .is_none_or(|old| !same_controls(old, &frame))
                {
                    if let Err(error) = apply(&mut device, &frame) {
                        outcome = Err(error);
                        break;
                    }
                }
                last_controls = Some(frame);
            }
        }

        if !stale && last_seen.elapsed() > INPUT_TIMEOUT {
            if let Err(error) = device
                .neutralize()
                .map_err(|error| error.to_string())
                .and_then(|()| device.commit().map_err(|error| error.to_string()))
            {
                outcome = Err(error);
                break;
            }
            stale = true;
            last_controls = None;
        }

        if let Err(error) = device.service(&mut |_| {}) {
            outcome = Err(error.to_string());
            break;
        }

        let delay = device
            .next_service_in()
            .unwrap_or(MAX_SERVICE_WAIT)
            .min(MAX_SERVICE_WAIT);
        // Even with an immediate next deadline, never spin unboundedly.
        thread::sleep(delay.max(Duration::from_micros(250)));
    }

    if let Err(error) = device
        .neutralize()
        .map_err(|error| error.to_string())
        .and_then(|()| device.commit().map_err(|error| error.to_string()))
    {
        if outcome.is_ok() {
            outcome = Err(error);
        }
    }
    device.close();
    outcome
}

#[cfg(test)]
mod tests {
    use super::{same_controls, stick_axis, trigger_axis};
    use crate::input::InputFrame;

    #[test]
    fn maps_axis_extremes_and_neutral() {
        assert_eq!(stick_axis(-1.0).raw(), -32767);
        assert_eq!(stick_axis(0.0).raw(), 0);
        assert_eq!(stick_axis(1.0).raw(), 32767);
        assert_eq!(trigger_axis(0.0).raw(), 0);
        assert_eq!(trigger_axis(1.0).raw(), 255);
    }

    #[test]
    fn changed_sequence_alone_does_not_change_controls() {
        let left = InputFrame::default();
        let right = InputFrame {
            seq: 1,
            ..left.clone()
        };
        assert!(same_controls(&left, &right));
    }
}
