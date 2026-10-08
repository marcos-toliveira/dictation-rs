//! Ícones da bandeja no Windows, gerados em tempo de execução (GDI), **sem assets**.
//!
//! Mesma semântica do Linux:
//! * Ocioso → microfone (glifo `U+E720` de *Segoe MDL2 Assets*);
//! * Gravando → círculo vermelho (*record*), como o `media-record`.
//!
//! A cor do microfone segue o tema do sistema (claro → preto, escuro → branco)
//! para permanecer legível em qualquer barra de tarefas.
//!
//! Também cuida da **visibilidade** dos ícones novos no Windows 11: por padrão o
//! shell os coloca no *overflow*; aqui marcamos `IsPromoted = 1` uma única vez
//! (respeitando escolha explícita do usuário) para o ícone aparecer ao lado do
//! relógio.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{BOOL, COLORREF, ERROR_SUCCESS, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, Ellipse, FillRect, GetDC, GetStockObject, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DT_CENTER, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HDC, HFONT, OUT_TT_PRECIS,
    TRANSPARENT, VARIABLE_PITCH,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_VALUE_TYPE, RRF_RT_REG_DWORD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON,
};

/// Glifo de microfone em *Segoe MDL2 Assets*.
const GLYPH_MIC: char = '\u{E720}';

/// Cor de destaque do microfone gravando (vermelho).
const RECORD_RGB: COLORREF = COLORREF(0x0028_28E6); // RGB(230, 40, 40)

/// Tamanho (em pixels) do ícone pequeno da bandeja, já considerando o DPI.
pub fn small_icon_size() -> i32 {
    unsafe {
        let s = GetSystemMetrics(SM_CXSMICON);
        if s > 0 {
            s
        } else {
            16
        }
    }
}

/// Ícone de microfone (ocioso).
pub fn idle_icon() -> Option<HICON> {
    let fg = if light_theme() {
        COLORREF(0x0000_0000)
    } else {
        COLORREF(0x00FF_FFFF)
    };
    build_icon(small_icon_size(), move |cdc, mdc, size| {
        draw_glyph(cdc, mdc, size, fg);
    })
}

/// Ícone de gravação (círculo vermelho).
pub fn recording_icon() -> Option<HICON> {
    build_icon(small_icon_size(), |cdc, mdc, size| {
        draw_record(cdc, mdc, size);
    })
}

/// `true` se o tema do sistema (barra de tarefas) for claro.
fn light_theme() -> bool {
    unsafe {
        let mut data: u32 = 0;
        let mut cb: u32 = std::mem::size_of::<u32>() as u32;
        let rc = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut core::ffi::c_void),
            Some(&mut cb),
        );
        rc == ERROR_SUCCESS && data == 1
    }
}

/// Cria um `HICON` desenhando em dois bitmaps GDI espelhados (cor + máscara).
///
/// O `draw` recebe `(dc_cor, dc_mascara, tamanho)` e deve pintar a **cor** no
/// primeiro e a **cobertura em preto sobre branco** no segundo (o GDI não faz
/// alfa; a máscara garante a transparência).
fn build_icon<F>(size: i32, draw: F) -> Option<HICON>
where
    F: FnOnce(HDC, HDC, i32),
{
    if size <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(HWND(0));
        if screen.is_invalid() {
            return None;
        }
        let color = CreateCompatibleBitmap(screen, size, size);
        if color.is_invalid() {
            ReleaseDC(HWND(0), screen);
            return None;
        }
        // Bitmap monocromático para a máscara (1 bpp).
        let mask = CreateBitmap(size, size, 1, 1, None);
        if mask.is_invalid() {
            let _ = DeleteObject(color);
            ReleaseDC(HWND(0), screen);
            return None;
        }

        let cdc = CreateCompatibleDC(screen);
        let mdc = CreateCompatibleDC(screen);

        let old_c = SelectObject(cdc, color);
        let old_m = SelectObject(mdc, mask);

        let rect = RECT {
            left: 0,
            top: 0,
            right: size,
            bottom: size,
        };
        // Cor: fundo preto. Máscara: fundo branco (transparente).
        let black = CreateSolidBrush(COLORREF(0));
        let white = CreateSolidBrush(COLORREF(0x00FF_FFFF));
        FillRect(cdc, &rect, black);
        FillRect(mdc, &rect, white);

        draw(cdc, mdc, size);

        let _ = SelectObject(cdc, old_c);
        let _ = SelectObject(mdc, old_m);
        let _ = DeleteObject(black);
        let _ = DeleteObject(white);
        let _ = DeleteDC(cdc);
        let _ = DeleteDC(mdc);
        ReleaseDC(HWND(0), screen);

        let info = ICONINFO {
            fIcon: BOOL::from(true),
            hbmMask: mask,
            hbmColor: color,
            ..Default::default()
        };
        let icon = CreateIconIndirect(&info).ok().filter(|i| !i.is_invalid());

        // O sistema copia os bitmaps; os originais podem ser liberados.
        let _ = DeleteObject(color);
        let _ = DeleteObject(mask);
        icon
    }
}

fn make_font(size: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -(size as f32 * 0.92) as i32,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_TT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            ANTIALIASED_QUALITY.0 as u32,
            VARIABLE_PITCH.0 as u32,
            w!("Segoe MDL2 Assets"),
        )
    }
}

fn draw_glyph(cdc: HDC, mdc: HDC, size: i32, fg: COLORREF) {
    unsafe {
        let font = make_font(size);
        let mut wide: Vec<u16> = vec![GLYPH_MIC as u16];

        // Cor: glifo na cor de destaque.
        let old_c = SelectObject(cdc, font);
        SetBkMode(cdc, TRANSPARENT);
        SetTextColor(cdc, fg);
        let mut r1 = RECT {
            left: 0,
            top: 0,
            right: size,
            bottom: size,
        };
        DrawTextW(
            cdc,
            &mut wide,
            &mut r1,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        let _ = SelectObject(cdc, old_c);

        // Máscara: glifo preto sobre branco.
        let old_m = SelectObject(mdc, font);
        SetBkMode(mdc, TRANSPARENT);
        SetTextColor(mdc, COLORREF(0));
        let mut r2 = RECT {
            left: 0,
            top: 0,
            right: size,
            bottom: size,
        };
        DrawTextW(
            mdc,
            &mut wide,
            &mut r2,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        let _ = SelectObject(mdc, old_m);

        let _ = DeleteObject(font);
    }
}

fn draw_record(cdc: HDC, mdc: HDC, size: i32) {
    unsafe {
        let red = CreateSolidBrush(RECORD_RGB);
        let black = CreateSolidBrush(COLORREF(0));
        let null_pen = GetStockObject(windows::Win32::Graphics::Gdi::NULL_PEN);

        let old_c = SelectObject(cdc, red);
        let old_cp = SelectObject(cdc, null_pen);
        let old_m = SelectObject(mdc, black);
        let old_mp = SelectObject(mdc, null_pen);

        let pad = (size / 8).max(1);
        Ellipse(cdc, pad, pad, size - pad, size - pad);
        Ellipse(mdc, pad, pad, size - pad, size - pad);

        let _ = SelectObject(cdc, old_c);
        let _ = SelectObject(cdc, old_cp);
        let _ = SelectObject(mdc, old_m);
        let _ = SelectObject(mdc, old_mp);
        let _ = DeleteObject(red);
        let _ = DeleteObject(black);
    }
}

/// Libera um `HICON` criado por [`idle_icon`] / [`recording_icon`].
pub fn destroy(h: HICON) {
    if !h.is_invalid() {
        unsafe {
            let _ = DestroyIcon(h);
        }
    }
}

/// Garante que o ícone deste executável esteja **promovido** (visível ao lado do
/// relógio) no Windows 11.
///
/// Só grava `IsPromoted = 1` quando o usuário **nunca** escolheu (valor ausente),
/// preservando uma decisão explícita de ocultar. Retorna `true` se o registro foi
/// alterado — caso em que o shell precisa reler (re-adicionar o ícone).
pub fn ensure_promoted() -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().to_lowercase(),
        Err(_) => return false,
    };
    unsafe {
        let mut root = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Control Panel\\NotifyIconSettings"),
            0,
            KEY_READ,
            &mut root,
        ) != ERROR_SUCCESS
        {
            return false;
        }

        let mut changed = false;
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 512];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(
                root,
                index,
                windows::core::PWSTR(name.as_mut_ptr()),
                &mut len,
                None,
                windows::core::PWSTR::null(),
                None,
                None,
            ) != ERROR_SUCCESS
            {
                break;
            }
            index += 1;

            let subkey = String::from_utf16_lossy(&name[..len as usize]);
            let wide = windows::core::HSTRING::from(subkey);
            let mut sub = HKEY::default();
            if RegOpenKeyExW(
                root,
                PCWSTR(wide.as_ptr()),
                0,
                KEY_READ | KEY_SET_VALUE,
                &mut sub,
            ) != ERROR_SUCCESS
            {
                continue;
            }

            let matches = query_string(sub, w!("ExecutablePath"))
                .map(|p| p.to_lowercase() == exe)
                .unwrap_or(false);
            if matches && query_dword(sub, w!("IsPromoted")).is_none() {
                let one: u32 = 1;
                if RegSetValueExW(
                    sub,
                    w!("IsPromoted"),
                    0,
                    REG_DWORD,
                    Some(&one.to_ne_bytes()),
                ) == ERROR_SUCCESS
                {
                    changed = true;
                }
            }
            let _ = RegCloseKey(sub);
            if changed {
                break;
            }
        }

        let _ = RegCloseKey(root);
        changed
    }
}

unsafe fn query_string(hkey: HKEY, name: PCWSTR) -> Option<String> {
    let mut ty = REG_VALUE_TYPE::default();
    let mut cb: u32 = 0;
    if RegQueryValueExW(hkey, name, None, Some(&mut ty), None, Some(&mut cb)) != ERROR_SUCCESS
        || cb == 0
    {
        return None;
    }
    let mut buf = vec![0u8; cb as usize];
    if RegQueryValueExW(
        hkey,
        name,
        None,
        Some(&mut ty),
        Some(buf.as_mut_ptr()),
        Some(&mut cb),
    ) != ERROR_SUCCESS
    {
        return None;
    }
    let wide: Vec<u16> = buf[..cb as usize]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_ne_bytes(*c))
        .collect();
    Some(
        String::from_utf16_lossy(&wide)
            .trim_end_matches('\0')
            .to_string(),
    )
}

unsafe fn query_dword(hkey: HKEY, name: PCWSTR) -> Option<u32> {
    let mut ty = REG_VALUE_TYPE::default();
    let mut data: u32 = 0;
    let mut cb: u32 = std::mem::size_of::<u32>() as u32;
    if RegQueryValueExW(
        hkey,
        name,
        None,
        Some(&mut ty),
        Some(&mut data as *mut u32 as *mut u8),
        Some(&mut cb),
    ) != ERROR_SUCCESS
    {
        return None;
    }
    Some(data)
}
