//! Power and shimmer domain events — see SPEC.md Module 1.

/// Active power feed at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSource {
    /// Running on battery.
    Battery,
    /// AC adapter or line power connected.
    Ac,
    /// State could not be determined (adapter startup only).
    Unknown,
}

/// Factual reports from the Power Listener (not commands; no shimmer policy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerEvent {
    /// Emitted once when the listener starts, before any transition events.
    InitialState {
        /// Observed power source at subscription time.
        source: PowerSource,
    },
    /// Emitted when the active power source changes.
    Transition {
        /// Previous power source.
        from: PowerSource,
        /// New power source.
        to: PowerSource,
    },
}

impl PowerEvent {
    /// Returns true for `Transition { from: Battery, to: Ac }` only.
    #[must_use]
    pub fn is_battery_to_ac(&self) -> bool {
        matches!(
            self,
            Self::Transition {
                from: PowerSource::Battery,
                to: PowerSource::Ac,
            }
        )
    }
}

/// Which display to cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorTarget {
    /// v1 default — primary display only.
    Primary,
}

/// User-tunable visual parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct ShimmerConfig {
    /// Total animation length in milliseconds. Default: `2000`.
    pub duration_ms: u32,
    /// Peak overlay opacity in `[0.0, 1.0]`. Default: `0.35`.
    pub opacity: f32,
    /// Shimmer scroll speed multiplier. Default: `1.0`.
    pub speed: f32,
    /// Which display to cover. v1: always `Primary`.
    pub monitor: MonitorTarget,
}

impl Default for ShimmerConfig {
    fn default() -> Self {
        Self {
            duration_ms: 2_000,
            opacity: 0.35,
            speed: 1.0,
            monitor: MonitorTarget::Primary,
        }
    }
}

/// Distinguishes automatic vs user-initiated plays (logging/metrics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimmerTrigger {
    /// Automatic play after a Battery→AC power transition.
    PowerTransition,
    /// User-initiated play from the system tray.
    Manual,
    /// User-initiated play from the CLI `--trigger` flag.
    Cli,
}

/// A single play invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct ShimmerRequest {
    /// Visual parameters for this play.
    pub config: ShimmerConfig,
    /// What caused this play (logging/metrics only).
    pub trigger: ShimmerTrigger,
}
