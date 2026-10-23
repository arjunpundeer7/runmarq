//! Core contracts shared by every runmarq component.
//!
//! Everything that crosses a process boundary (hook -> collector -> desktop) is
//! defined here once and exported to TypeScript through `ts-rs`, so the Rust
//! and TypeScript sides cannot drift apart.

pub mod api;
pub mod brand;
pub mod capabilities;
pub mod event;
pub mod forecast;
pub mod paths;
pub mod run;
pub mod signals;

pub use capabilities::AgentCapabilities;
pub use event::{
    ActivityCategory, AgentSource, EventEnvelope, EventPayload, EventType, PromptFeatures,
    TaskCategory, SCHEMA_VERSION,
};
pub use forecast::{Confidence, EtaForecast, ForecastStatus};
pub use run::{RunState, TransitionError};
pub use signals::{Phase, RunSignals};
