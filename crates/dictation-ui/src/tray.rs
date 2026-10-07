//! Bandeja (system tray) via `ksni` — StatusNotifierItem nativo do KDE, **sem GTK**
//! (por isso convive com o winit/eframe na thread principal).
//!
//! Ícone: microfone (ocioso) / media-record (gravando). Menu: iniciar/parar, ensinar,
//! editar correções/vocabulário, sair. Clique esquerdo alterna a gravação.

use ksni::menu::StandardItem;
use ksni::{MenuItem, ToolTip, Tray};
use std::sync::mpsc::Sender;

/// Ação disparada pela bandeja.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Toggle,
    Teach,
    EditCorrections,
    EditVocab,
    Quit,
}

/// Implementação da bandeja do dictation.
pub struct DictationTray {
    pub recording: bool,
    pub tx: Sender<TrayAction>,
}

fn item(label: &str, action: TrayAction, tx: &Sender<TrayAction>) -> MenuItem<DictationTray> {
    let tx = tx.clone();
    StandardItem {
        label: label.into(),
        activate: Box::new(move |_this: &mut DictationTray| {
            let _ = tx.send(action);
        }),
        ..Default::default()
    }
    .into()
}

impl Tray for DictationTray {
    fn id(&self) -> String {
        "dictation".into()
    }

    fn title(&self) -> String {
        "Dictation".into()
    }

    fn icon_name(&self) -> String {
        if self.recording {
            "media-record".into()
        } else {
            "audio-input-microphone".into()
        }
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Dictation".into(),
            description: if self.recording {
                "GRAVANDO".into()
            } else {
                "ocioso (F8)".into()
            },
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayAction::Toggle);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            item("Iniciar / parar", TrayAction::Toggle, &self.tx),
            item("Ensinar correção…", TrayAction::Teach, &self.tx),
            MenuItem::Separator,
            item(
                "Editar correções (teach)",
                TrayAction::EditCorrections,
                &self.tx,
            ),
            item("Editar vocabulário", TrayAction::EditVocab, &self.tx),
            MenuItem::Separator,
            item("Sair", TrayAction::Quit, &self.tx),
        ]
    }
}

/// Sobe a bandeja em background. Retorna o handle (para atualizar o ícone/estado).
pub fn spawn_tray(
    tx: Sender<TrayAction>,
) -> Result<ksni::blocking::Handle<DictationTray>, ksni::Error> {
    use ksni::blocking::TrayMethods;
    DictationTray {
        recording: false,
        tx,
    }
    .assume_sni_available(true)
    .spawn()
}

/// Handle da bandeja (re-export amigável para quem não depende de `ksni` diretamente).
pub type TrayHandle = ksni::blocking::Handle<DictationTray>;
