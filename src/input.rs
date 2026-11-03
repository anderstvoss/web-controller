//! Browser-to-service M1 input contract. No controller protocol bytes live here.
use serde::{Deserialize, Serialize};

/// Controls: A, B, X, Y, LB, RB, Back, Start, Guide, L3, R3,
/// D-up, D-down, D-left, D-right.
pub const BUTTON_COUNT: usize = 15;

/// A complete controller state; never interpret as a patch or delta.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputFrame {
    /// M1 contract version.
    pub v: u8,
    /// Strictly increasing within one WebSocket.
    pub seq: u64,
    pub buttons: [bool; BUTTON_COUNT],
    /// Left X/Y, right X/Y, normalized to -1..=1; positive Y is up.
    pub sticks: [f32; 4],
    /// Left and right triggers in 0..=1.
    pub triggers: [f32; 2],
}

impl Default for InputFrame {
    fn default() -> Self {
        Self {
            v: 1,
            seq: 0,
            buttons: [false; BUTTON_COUNT],
            sticks: [0.0; 4],
            triggers: [0.0; 2],
        }
    }
}

impl InputFrame {
    /// Reject schema versions or invalid controls; caller checks sequence freshness.
    ///
    /// # Errors
    /// Returns a static diagnostic for unsupported or out-of-range input.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.v != 1 {
            return Err("unsupported input version");
        }
        if self
            .sticks
            .iter()
            .any(|axis| !axis.is_finite() || !(-1.0..=1.0).contains(axis))
        {
            return Err("stick axis outside normalized range");
        }
        if self
            .triggers
            .iter()
            .any(|trigger| !trigger.is_finite() || !(0.0..=1.0).contains(trigger))
        {
            return Err("trigger outside normalized range");
        }
        Ok(())
    }
}

/// Parse a bounded, fully typed state with no unknown keys.
///
/// # Errors
/// Rejects JSON syntax, missing fields and invalid normalized inputs.
pub fn parse_frame(bytes: &[u8]) -> Result<InputFrame, &'static str> {
    if bytes.len() > 512 {
        return Err("frame too large");
    }
    let frame: InputFrame = serde_json::from_slice(bytes).map_err(|_| "invalid input JSON")?;
    frame.validate()?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::{InputFrame, parse_frame};
    use proptest::prelude::*;

    #[test]
    fn rejects_unexpected_fields() {
        let mut value = serde_json::to_value(InputFrame::default()).expect("valid state");
        value["extra"] = serde_json::json!(true);
        assert!(parse_frame(value.to_string().as_bytes()).is_err());
    }

    #[test]
    fn rejects_missing_or_incomplete_arrays() {
        let mut value = serde_json::to_value(InputFrame::default()).expect("valid state");
        value["sticks"] = serde_json::json!([0.0]);
        assert!(parse_frame(value.to_string().as_bytes()).is_err());
    }

    #[test]
    fn validates_range_and_version() {
        let mut frame = InputFrame::default();
        assert!(frame.validate().is_ok());
        frame.sticks[0] = 1.1;
        assert!(frame.validate().is_err());
        frame.sticks[0] = 0.0;
        frame.triggers[0] = -0.01;
        assert!(frame.validate().is_err());
        frame.triggers[0] = 0.0;
        frame.v = 9;
        assert!(frame.validate().is_err());
    }

    #[test]
    fn rejects_oversized_frames() {
        assert!(parse_frame(&[b'a'; 513]).is_err());
    }

    proptest! {
        #[test]
        fn valid_normalized_states_round_trip(
            axes in prop::array::uniform4(-1.0f32..=1.0f32),
            triggers in prop::array::uniform2(0.0f32..=1.0f32),
            seq in any::<u64>(),
        ) {
            let frame = InputFrame {
                sticks: axes,
                triggers,
                seq,
                ..InputFrame::default()
            };
            let encoded = serde_json::to_vec(&frame).expect("serializable");
            let decoded = parse_frame(&encoded).expect("well-formed state");
            prop_assert_eq!(frame, decoded);
        }

        #[test]
        fn malformed_buffers_never_pass_validation(data in proptest::collection::vec(any::<u8>(), 513..800)) {
            prop_assert!(parse_frame(&data).is_err());
        }
    }
}
