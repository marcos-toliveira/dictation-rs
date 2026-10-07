//! Identificador de sessão — isola completamente cada gravação.

/// Uma gravação = um `SessionId`. Nenhum estado/arquivo é compartilhado entre sessões,
/// o que impede que uma transcrição antiga contamine a nova.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub u64);

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s{}", self.0)
    }
}

/// Gerador monotônico de `SessionId` (nunca repete).
#[derive(Debug)]
pub struct SessionIdGen {
    next: u64,
}

impl SessionIdGen {
    pub fn new() -> Self {
        Self { next: 1 }
    }

    pub fn next_id(&mut self) -> SessionId {
        let id = SessionId(self.next);
        self.next += 1;
        id
    }
}

impl Default for SessionIdGen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_monotonic() {
        let mut g = SessionIdGen::new();
        let a = g.next_id();
        let b = g.next_id();
        assert_ne!(a, b);
        assert!(b.0 > a.0);
    }
}
