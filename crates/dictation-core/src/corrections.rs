//! Correções determinísticas aplicadas após a transcrição (sem dependências externas).
//!
//! Formato do arquivo: `errado<TAB>certo` ou `errado=>certo`; linhas `#` são comentários.
//! A substituição ignora maiúsc./minúsc. e respeita limites de palavra (quando o termo é alfanumérico).

use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct Corrections {
    rules: Vec<(String, String)>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

fn lc(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

impl Corrections {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Parseia o texto do arquivo de correções.
    pub fn parse(text: &str) -> Self {
        let mut rules = Vec::new();
        for line in text.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let (from, to) = if let Some((a, b)) = line.split_once('\t') {
                (a, b)
            } else if let Some((a, b)) = line.split_once("=>") {
                (a, b)
            } else {
                continue;
            };
            let from = from.trim();
            let to = to.trim();
            if from.is_empty() {
                continue;
            }
            rules.push((from.to_string(), to.to_string()));
        }
        Self { rules }
    }

    pub fn from_file(path: &Path) -> std::io::Result<Self> {
        Ok(Self::parse(&std::fs::read_to_string(path)?))
    }

    /// Aplica todas as correções, na ordem.
    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (from, to) in &self.rules {
            out = replace_ci(&out, from, to);
        }
        out
    }
}

/// Substitui `needle` por `to` em `haystack`, ignorando caixa e respeitando limites de palavra.
fn replace_ci(haystack: &str, needle: &str, to: &str) -> String {
    let h: Vec<char> = haystack.chars().collect();
    let n: Vec<char> = needle.chars().map(lc).collect();
    if n.is_empty() {
        return haystack.to_string();
    }
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while i < h.len() {
        if i + n.len() <= h.len() && (0..n.len()).all(|k| lc(h[i + k]) == n[k]) {
            let before_ok = i == 0 || !is_word(h[i - 1]);
            let after_ok = i + n.len() == h.len() || !is_word(h[i + n.len()]);
            if before_ok && after_ok {
                out.push_str(to);
                i += n.len();
                continue;
            }
        }
        out.push(h[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tab_and_arrow() {
        let c = Corrections::parse("teh\tthe\npresel=>presell\n# comentário\n");
        assert_eq!(c.len(), 2);
        assert_eq!(c.apply("teh"), "the");
        assert_eq!(c.apply("presel"), "presell");
    }

    #[test]
    fn case_insensitive_with_word_boundary() {
        let c = Corrections::parse("open code\tOpenCode\n");
        assert_eq!(c.apply("eu uso open code aqui"), "eu uso OpenCode aqui");
        assert_eq!(c.apply("Open Code"), "OpenCode");
        // não deve casar dentro de palavra maior
        assert_eq!(c.apply("open codes"), "open codes");
    }

    #[test]
    fn applies_in_order() {
        let c = Corrections::parse("a\tb\nb\tc\n");
        assert_eq!(c.apply("a"), "c");
    }
}
