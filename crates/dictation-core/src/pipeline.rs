//! Motor de sessão: orquestra o ciclo `Start → grava → Stop → finaliza → injeta`.
//!
//! É aqui que os bugs de fala longa são eliminados **por construção**:
//! * a máquina de estados **proíbe iniciar** uma gravação enquanto outra não terminou
//!   (não há duas capturas concorrentes nem arquivos compartilhados);
//! * cada resultado de transcrição é **aceito só se pertencer à sessão corrente**
//!   (`accept`), então texto de uma sessão antiga é **descartado**, nunca injetado;
//! * a transcrição é **append-only** (via [`Transcript`]).

use crate::machine::{Event, Machine, Phase, TransitionError};
use crate::segmenter::{Segment, Segmenter, SegmenterConfig};
use crate::session::SessionId;
use crate::transcript::Transcript;

/// Uma gravação em andamento (isolada por `SessionId`).
pub struct Session {
    id: SessionId,
    segmenter: Segmenter,
    transcript: Transcript,
}

impl Session {
    pub fn id(&self) -> SessionId {
        self.id
    }

    pub fn push_audio(&mut self, samples: &[i16]) -> Vec<Segment> {
        self.segmenter.push(samples)
    }

    pub fn flush(&mut self) -> Option<Segment> {
        self.segmenter.flush()
    }

    pub fn append_text(&mut self, text: &str) {
        self.transcript.push(text);
    }

    pub fn text(&self) -> String {
        self.transcript.text()
    }

    pub fn is_empty(&self) -> bool {
        self.transcript.is_empty()
    }

    pub fn pushed_samples(&self) -> u64 {
        self.segmenter.pushed()
    }

    /// Texto comprometido (segmentos já transcritos).
    pub fn committed(&self) -> String {
        self.transcript.text()
    }

    /// Cauda de áudio em formação (ainda não transcrita).
    pub fn pending_pcm(&self) -> &[i16] {
        self.segmenter.buffered()
    }
}

/// Orquestrador do ciclo de ditado.
pub struct Pipeline {
    machine: Machine,
    session: Option<Session>,
    cfg: SegmenterConfig,
}

impl Pipeline {
    pub fn new(cfg: SegmenterConfig) -> Self {
        Self {
            machine: Machine::new(),
            session: None,
            cfg,
        }
    }

    pub fn phase(&self) -> Phase {
        self.machine.phase()
    }

    pub fn session_id(&self) -> Option<SessionId> {
        self.machine.session()
    }

    pub fn is_recording(&self) -> bool {
        self.machine.is_recording()
    }

    /// Texto já comprometido (segmentos transcritos, append-only).
    pub fn committed_text(&self) -> String {
        self.session
            .as_ref()
            .map(|s| s.committed())
            .unwrap_or_default()
    }

    /// Cauda de áudio ainda não transcrita (para a prévia ao vivo).
    pub fn pending_pcm(&self) -> Vec<i16> {
        self.session
            .as_ref()
            .map(|s| s.pending_pcm().to_vec())
            .unwrap_or_default()
    }

    /// Inicia uma sessão. Falha se já houver gravação em andamento.
    pub fn start(&mut self) -> Result<SessionId, TransitionError> {
        self.machine.on(Event::Start)?;
        let id = self.machine.session().expect("sessão após Start");
        self.session = Some(Session {
            id,
            segmenter: Segmenter::new(self.cfg),
            transcript: Transcript::new(),
        });
        Ok(id)
    }

    /// Recebe áudio; devolve os segmentos prontos com o id da sessão.
    pub fn push_audio(&mut self, samples: &[i16]) -> Vec<(SessionId, Segment)> {
        match self.session.as_mut() {
            Some(s) if self.machine.is_recording() => {
                let id = s.id;
                s.push_audio(samples)
                    .into_iter()
                    .map(|seg| (id, seg))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Encerra a gravação: devolve o segmento final (se houver).
    pub fn stop(&mut self) -> Result<Option<(SessionId, Segment)>, TransitionError> {
        self.machine.on(Event::Stop)?;
        Ok(self.session.as_mut().and_then(|s| {
            let id = s.id;
            s.flush().map(|seg| (id, seg))
        }))
    }

    /// Aplica um texto transcrito. Retorna `false` se a sessão foi superada (descartar!).
    pub fn accept(&mut self, id: SessionId, text: &str) -> bool {
        match self.session.as_mut() {
            Some(s) if s.id == id => {
                s.append_text(text);
                true
            }
            _ => false,
        }
    }

    /// Finaliza a sessão e devolve o texto final (ou `None` se vazio). Volta a `Idle`.
    pub fn finish(&mut self) -> Result<Option<String>, TransitionError> {
        self.machine.on(Event::Finished)?;
        Ok(self
            .session
            .take()
            .map(|s| s.text())
            .filter(|t| !t.is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SegmenterConfig {
        SegmenterConfig {
            segment_samples: 10,
            overlap_samples: 2,
            min_samples: 1,
        }
    }

    #[test]
    fn full_cycle_concatenates_segments() {
        let mut p = Pipeline::new(cfg());
        let id = p.start().unwrap();
        let segs = p.push_audio(&[0i16; 25]);
        assert_eq!(segs.len(), 2);
        assert!(segs.iter().all(|(sid, _)| *sid == id));
        assert!(p.accept(id, "primeira parte"));
        assert!(p.accept(id, "segunda parte"));
        let tail = p.stop().unwrap();
        assert!(tail.is_some());
        let text = p.finish().unwrap().unwrap();
        assert_eq!(text, "primeira parte segunda parte");
        assert_eq!(p.phase(), Phase::Idle);
    }

    #[test]
    fn cannot_start_while_recording() {
        let mut p = Pipeline::new(cfg());
        p.start().unwrap();
        assert!(p.start().is_err(), "não pode haver duas gravações");
    }

    #[test]
    fn accepts_only_current_session() {
        let mut p = Pipeline::new(cfg());
        let a = p.start().unwrap();
        // sessão "estrangeira" (nunca a corrente) deve ser descartada
        let foreign = SessionId(9999);
        assert!(!p.accept(foreign, "texto antigo"));
        assert!(p.accept(a, "ok"));
    }

    #[test]
    fn superseded_text_is_discarded_after_new_session() {
        let mut p = Pipeline::new(cfg());
        let a = p.start().unwrap();
        p.accept(a, "sessao A");
        p.stop().unwrap();
        let text_a = p.finish().unwrap().unwrap();
        assert_eq!(text_a, "sessao A");

        // nova sessão
        let b = p.start().unwrap();
        assert_ne!(a, b);
        // uma resposta atrasada da sessão A não pode contaminar a B
        assert!(!p.accept(a, "texto velho de A"));
        assert!(p.accept(b, "sessao B"));
        p.stop().unwrap();
        assert_eq!(p.finish().unwrap().unwrap(), "sessao B");
    }

    #[test]
    fn stop_without_start_fails() {
        let mut p = Pipeline::new(cfg());
        assert!(p.stop().is_err());
    }

    #[test]
    fn empty_session_finishes_none() {
        let mut p = Pipeline::new(cfg());
        p.start().unwrap();
        p.stop().unwrap();
        assert!(p.finish().unwrap().is_none());
    }

    #[test]
    fn preview_exposes_committed_and_pending() {
        let mut p = Pipeline::new(cfg());
        let id = p.start().unwrap();
        p.accept(id, "comprometido");
        let segs = p.push_audio(&[0i16; 7]); // < 10 => fica na cauda
        assert!(segs.is_empty());
        assert_eq!(p.committed_text(), "comprometido");
        assert_eq!(p.pending_pcm().len(), 7);

        p.stop().unwrap();
        p.finish().unwrap();
        assert_eq!(p.committed_text(), "");
        assert!(p.pending_pcm().is_empty());
    }
}
