//! `dictation-core` — núcleo **puro** (sem I/O de plataforma) do dictation-rs.
//!
//! Aqui vivem as garantias que impedem os bugs observados na versão Python:
//! * uma **máquina de estados** determinística (nunca duas gravações ao mesmo tempo);
//! * **sessões isoladas** (`SessionId`) — nenhum estado/arquivo é compartilhado entre gravações;
//! * **transcrição append-only** por segmento — o texto só cresce, nunca "volta" texto antigo;
//! * segmentação **sem perda** de áudio (custo linear, mesmo em falas longas).
//!
//! Nada aqui depende de X11/Wayland/Windows — por isso é 100% testável e portável.

pub mod asr;
pub mod audio;
pub mod config;
pub mod corrections;
pub mod machine;
pub mod pipeline;
pub mod segmenter;
pub mod session;
pub mod transcript;

pub use asr::{AsrEngine, AsrError, AsrOptions, MockAsr};
pub use config::{Config, InjectMode, Provider};
pub use corrections::Corrections;
pub use machine::{Event, Machine, Phase, TransitionError};
pub use pipeline::{Pipeline, Session};
pub use segmenter::{Segment, Segmenter, SegmenterConfig};
pub use session::{SessionId, SessionIdGen};
pub use transcript::Transcript;
