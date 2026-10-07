//! Transcrição **append-only**.
//!
//! O texto só cresce — nunca é reescrito nem reordenado. É isso que impede o sintoma
//! de "texto antigo reaparecendo no meio do novo": a transcrição final é sempre a
//! concatenação dos segmentos, na ordem em que foram produzidos.

/// Acumulador monotônico de partes de transcrição.
#[derive(Debug, Default, Clone)]
pub struct Transcript {
    parts: Vec<String>,
}

impl Transcript {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acrescenta uma parte (ignora vazias/espaços).
    pub fn push(&mut self, part: impl Into<String>) {
        let s = part.into();
        let t = s.trim();
        if !t.is_empty() {
            self.parts.push(t.to_string());
        }
    }

    /// Texto completo (partes unidas por espaço).
    pub fn text(&self) -> String {
        self.parts.join(" ")
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn parts(&self) -> &[String] {
        &self.parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_only_is_monotonic() {
        let mut t = Transcript::new();
        t.push("olá");
        let a = t.text();
        t.push("mundo");
        let b = t.text();
        assert!(b.starts_with(&a), "o texto nunca pode encolher/reescrever");
        assert_eq!(b, "olá mundo");
    }

    #[test]
    fn ignores_blank_parts() {
        let mut t = Transcript::new();
        t.push("  ");
        t.push("ok");
        t.push("");
        assert_eq!(t.text(), "ok");
        assert_eq!(t.len(), 1);
    }
}
