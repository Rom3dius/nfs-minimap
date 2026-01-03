//! Turn type definitions

/// Types of turns/maneuvers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TurnType {
    /// Route start
    Start = 0,
    /// Continue straight
    Straight = 1,
    /// Slight left turn
    SlightLeft = 2,
    /// Left turn
    Left = 3,
    /// Sharp left turn
    SharpLeft = 4,
    /// Slight right turn
    SlightRight = 5,
    /// Right turn
    Right = 6,
    /// Sharp right turn
    SharpRight = 7,
    /// U-turn
    UTurn = 8,
    /// Arrived at destination
    Destination = 9,
}

impl TurnType {
    /// Convert to integer for FFI/Slint
    pub fn to_int(self) -> i32 {
        self as i32
    }

    /// Create from integer
    pub fn from_int(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Start),
            1 => Some(Self::Straight),
            2 => Some(Self::SlightLeft),
            3 => Some(Self::Left),
            4 => Some(Self::SharpLeft),
            5 => Some(Self::SlightRight),
            6 => Some(Self::Right),
            7 => Some(Self::SharpRight),
            8 => Some(Self::UTurn),
            9 => Some(Self::Destination),
            _ => None,
        }
    }

    /// Get a human-readable name for this turn type
    pub fn name(&self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Straight => "Straight",
            Self::SlightLeft => "Slight Left",
            Self::Left => "Left",
            Self::SharpLeft => "Sharp Left",
            Self::SlightRight => "Slight Right",
            Self::Right => "Right",
            Self::SharpRight => "Sharp Right",
            Self::UTurn => "U-Turn",
            Self::Destination => "Destination",
        }
    }

    /// Get a short instruction for this turn
    pub fn instruction(&self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Straight => "Continue straight",
            Self::SlightLeft => "Keep left",
            Self::Left => "Turn left",
            Self::SharpLeft => "Turn sharp left",
            Self::SlightRight => "Keep right",
            Self::Right => "Turn right",
            Self::SharpRight => "Turn sharp right",
            Self::UTurn => "Make a U-turn",
            Self::Destination => "You have arrived",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turn_type_roundtrip() {
        for i in 0..=9 {
            let turn = TurnType::from_int(i).unwrap();
            assert_eq!(turn.to_int(), i);
        }
    }

    #[test]
    fn test_invalid_turn_type() {
        assert!(TurnType::from_int(-1).is_none());
        assert!(TurnType::from_int(100).is_none());
    }
}
