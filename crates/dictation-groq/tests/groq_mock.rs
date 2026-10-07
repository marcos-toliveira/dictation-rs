//! Teste de integração do `dictation-groq` contra um servidor HTTP mock local
//! (sem dependências extras): valida o multipart enviado e o tratamento de erros.

use dictation_core::{AsrEngine, AsrError, AsrOptions};
use dictation_groq::GroqEngine;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

/// Sobe um servidor que responde uma única requisição e captura o corpo recebido.
fn spawn_mock(status: u16, body: &'static str) -> (String, Arc<Mutex<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let cap = captured.clone();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            headers.push_str(&line);
        }
        let len = headers
            .lines()
            .find_map(|l| {
                let low = l.to_ascii_lowercase();
                low.strip_prefix("content-length:")
                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
            })
            .unwrap_or(0);
        let mut body_bytes = vec![0u8; len];
        let _ = reader.read_exact(&mut body_bytes);
        *cap.lock().unwrap() = body_bytes;
        let reason = if status == 200 { "OK" } else { "Error" };
        let resp = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
    });
    (
        format!("http://127.0.0.1:{port}/openai/v1/audio/transcriptions"),
        captured,
    )
}

#[test]
fn transcribes_against_mock() {
    let (url, captured) = spawn_mock(200, r#"{"text":"olá mundo"}"#);
    let engine = GroqEngine::new("gsk_test").with_endpoint(url);
    let opts = AsrOptions {
        model: "whisper-large-v3-turbo".into(),
        language: Some("pt".into()),
        prompt: Some("Tulinho".into()),
    };
    let text = engine.transcribe(b"RIFFfakeaudio", &opts).unwrap();
    assert_eq!(text, "olá mundo");

    let req = captured.lock().unwrap().clone();
    let s = String::from_utf8_lossy(&req);
    assert!(s.contains("name=\"model\""));
    assert!(s.contains("whisper-large-v3-turbo"));
    assert!(s.contains("name=\"language\""));
    assert!(s.contains("name=\"prompt\""));
    assert!(s.contains("Tulinho"));
    assert!(
        req.windows(13).any(|w| w == b"RIFFfakeaudio"),
        "o áudio deve ir no corpo multipart"
    );
}

#[test]
fn maps_http_error() {
    let (url, _) = spawn_mock(400, r#"{"error":"bad"}"#);
    let engine = GroqEngine::new("gsk_test").with_endpoint(url);
    let err = engine
        .transcribe(
            b"x",
            &AsrOptions {
                model: "m".into(),
                ..Default::default()
            },
        )
        .unwrap_err();
    match err {
        AsrError::Http { status, .. } => assert_eq!(status, 400),
        other => panic!("esperava AsrError::Http, veio {other:?}"),
    }
}
