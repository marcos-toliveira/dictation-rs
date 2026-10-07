//! Máquina de estados determinística do ciclo de ditado.
//!
//! Garante, por construção, que **não há duas gravações ao mesmo tempo** e que o
//! "finalizar" só ocorre a partir de uma gravação — a origem dos bugs de concorrência
//! da versão anterior.

/// Fase atual do daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Recording,
    Finalizing,
}

/// Eventos que movem a máquina.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Start,
    Stop,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("evento {event:?} inválido na fase {phase:?}")]
    Invalid { phase: Phase, event: Event },
}

/// Máquina de estados + sessão corrente.
pub struct Machine {
    phase: Phase,
    session: Option<SessionId>,
    ids: SessionIdGen,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

use crate::session::{SessionId, SessionIdGen};

impl Machine {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            session: None,
            ids: SessionIdGen::new(),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn session(&self) -> Option<SessionId> {
        self.session
    }

    pub fn is_recording(&self) -> bool {
        self.phase == Phase::Recording
    }

    /// Aplica um evento. Retorna a nova fase ou um erro de transição inválida.
    pub fn on(&mut self, event: Event) -> Result<Phase, TransitionError> {
        match (self.phase, event) {
            (Phase::Idle, Event::Start) => {
                self.session = Some(self.ids.next_id());
                self.phase = Phase::Recording;
            }
            (Phase::Recording, Event::Stop) => {
                self.phase = Phase::Finalizing;
            }
            (Phase::Finalizing, Event::Finished) => {
                self.phase = Phase::Idle;
                self.session = None;
            }
            _ => {
                return Err(TransitionError::Invalid {
                    phase: self.phase,
                    event,
                })
            }
        }
        Ok(self.phase)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let mut m = Machine::new();
        assert_eq!(m.phase(), Phase::Idle);
        m.on(Event::Start).unwrap();
        assert_eq!(m.phase(), Phase::Recording);
        let s = m.session().unwrap();
        m.on(Event::Stop).unwrap();
        assert_eq!(m.phase(), Phase::Finalizing);
        assert_eq!(m.session(), Some(s));
        m.on(Event::Finished).unwrap();
        assert_eq!(m.phase(), Phase::Idle);
        assert_eq!(m.session(), None);
    }

    #[test]
    fn no_double_start() {
        let mut m = Machine::new();
        m.on(Event::Start).unwrap();
        assert!(
            m.on(Event::Start).is_err(),
            "não pode iniciar duas gravações"
        );
    }

    #[test]
    fn stop_requires_recording() {
        let mut m = Machine::new();
        assert!(m.on(Event::Stop).is_err());
    }

    #[test]
    fn finished_requires_finalizing() {
        let mut m = Machine::new();
        assert!(m.on(Event::Finished).is_err());
    }

    #[test]
    fn sessions_are_unique_across_cycles() {
        let mut m = Machine::new();
        m.on(Event::Start).unwrap();
        let a = m.session().unwrap();
        m.on(Event::Stop).unwrap();
        m.on(Event::Finished).unwrap();
        m.on(Event::Start).unwrap();
        let b = m.session().unwrap();
        assert_ne!(a, b);
    }
}
