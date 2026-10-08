//! Bandeja (system tray) via `ksni` — StatusNotifierItem nativo do KDE, **sem GTK**
//! (por isso convive com o winit/eframe na thread principal).
//!
//! Ícone: microfone (ocioso) / media-record (gravando). Menu: iniciar/parar, ensinar,
//! editar correções/vocabulário, sair. Clique esquerdo alterna a gravação.

#[cfg(target_os = "linux")]
use ksni::menu::StandardItem;
#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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

/// Sobe a bandeja em background no Linux (ksni).
#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
pub type TrayHandle = ksni::blocking::Handle<DictationTray>;

#[cfg(windows)]
pub struct TrayHandle {
    tx_state: std::sync::mpsc::Sender<bool>,
}

#[cfg(windows)]
impl TrayHandle {
    pub fn update<F>(&self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut DictationTray),
    {
        let mut dummy = DictationTray {
            recording: false,
            tx: std::sync::mpsc::channel().0,
        };
        f(&mut dummy);
        self.tx_state
            .send(dummy.recording)
            .map_err(|e| e.to_string())
    }
}

/// Sobe a bandeja em background no Windows (Shell_NotifyIconW).
#[cfg(windows)]
pub fn spawn_tray(tx: Sender<TrayAction>) -> Result<TrayHandle, String> {
    use crate::win_icon;
    use std::sync::mpsc;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DispatchMessageW, GetCursorPos, LoadIconW, PeekMessageW, PostQuitMessage, RegisterClassW,
        RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, TranslateMessage,
        IDI_APPLICATION, MF_SEPARATOR, MF_STRING, PM_REMOVE, TPM_NONOTIFY, TPM_RETURNCMD,
        TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_DESTROY, WM_LBUTTONUP,
        WM_RBUTTONUP, WNDCLASSW,
    };

    let (tx_state, rx_state) = mpsc::channel::<bool>();

    std::thread::spawn(move || unsafe {
        const WM_TRAY_CALLBACK: u32 = WM_APP + 10;
        // O shell envia esta mensagem quando o Explorer (re)inicia: os apps
        // precisam re-adicionar o ícone, senão ele "some" da bandeja.
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));

        unsafe extern "system" fn window_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            if msg == WM_DESTROY {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        let class_name = w!("DictationTrayMsgWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("DictationTray"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND(0),
            None,
            None,
            None,
        );
        if hwnd.0 == 0 {
            tracing::warn!("não foi possível criar a janela da bandeja");
            return;
        }

        // Ícones próprios (microfone no ocioso / círculo vermelho gravando), com
        // fallback para o ícone padrão do Windows caso a geração GDI falhe.
        let idle_icon = win_icon::idle_icon()
            .or_else(|| LoadIconW(None, IDI_APPLICATION).ok())
            .unwrap_or_default();
        let rec_icon = win_icon::recording_icon().unwrap_or(idle_icon);

        fn to_sz_tip(s: &str) -> [u16; 128] {
            let mut buf = [0u16; 128];
            let wide: Vec<u16> = s.encode_utf16().collect();
            let len = wide.len().min(127);
            buf[..len].copy_from_slice(&wide[..len]);
            buf
        }

        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1001,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY_CALLBACK,
            hIcon: idle_icon,
            szTip: to_sz_tip("Dictation - ocioso (F8)"),
            ..Default::default()
        };

        if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
            tracing::warn!("Shell_NotifyIconW(NIM_ADD) falhou");
        }

        // Windows 11 coloca ícones novos no *overflow*. Promove uma única vez
        // (só quando o usuário nunca escolheu) e re-adiciona para o shell reler.
        if win_icon::ensure_promoted() {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }

        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
        loop {
            while let Ok(recording) = rx_state.try_recv() {
                nid.hIcon = if recording { rec_icon } else { idle_icon };
                nid.szTip = to_sz_tip(if recording {
                    "Dictation - GRAVANDO (F8)"
                } else {
                    "Dictation - ocioso (F8)"
                });
                let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
            }

            while PeekMessageW(&mut msg, HWND(0), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_DESTROY {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
                    if rec_icon.0 != idle_icon.0 {
                        win_icon::destroy(rec_icon);
                    }
                    win_icon::destroy(idle_icon);
                    return;
                }

                // Explorer reiniciou: re-adiciona o ícone.
                if msg.message == taskbar_created {
                    nid.hIcon = idle_icon;
                    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
                }

                if msg.message == WM_TRAY_CALLBACK {
                    let event = msg.lParam.0 as u32;
                    if event == WM_LBUTTONUP {
                        let _ = tx.send(TrayAction::Toggle);
                    } else if event == WM_RBUTTONUP {
                        if let Ok(hmenu) = CreatePopupMenu() {
                            let _ = AppendMenuW(hmenu, MF_STRING, 1, w!("Iniciar / parar"));
                            let _ = AppendMenuW(
                                hmenu,
                                MF_STRING,
                                2,
                                w!("Ensinar corre\u{00e7}\u{00e3}o\u{2026}"),
                            );
                            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                            let _ = AppendMenuW(
                                hmenu,
                                MF_STRING,
                                3,
                                w!("Editar corre\u{00e7}\u{00f5}es (teach)"),
                            );
                            let _ =
                                AppendMenuW(hmenu, MF_STRING, 4, w!("Editar vocabul\u{00e1}rio"));
                            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                            let _ = AppendMenuW(hmenu, MF_STRING, 5, w!("Sair"));

                            let mut pt = POINT::default();
                            let _ = GetCursorPos(&mut pt);
                            let _ = SetForegroundWindow(hwnd);
                            let cmd = TrackPopupMenu(
                                hmenu,
                                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                                pt.x,
                                pt.y,
                                0,
                                hwnd,
                                None,
                            );
                            let _ = DestroyMenu(hmenu);

                            match cmd.0 {
                                1 => {
                                    let _ = tx.send(TrayAction::Toggle);
                                }
                                2 => {
                                    let _ = tx.send(TrayAction::Teach);
                                }
                                3 => {
                                    let _ = tx.send(TrayAction::EditCorrections);
                                }
                                4 => {
                                    let _ = tx.send(TrayAction::EditVocab);
                                }
                                5 => {
                                    let _ = tx.send(TrayAction::Quit);
                                }
                                _ => {}
                            }
                        }
                    }
                }

                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    Ok(TrayHandle { tx_state })
}
