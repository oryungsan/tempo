//! 음성 안내 — Windows 기본 음성(SAPI, 한국어가 있으면 한국어)을 숨은 PowerShell 로 읽음
use std::os::windows::process::CommandExt;

/// 말하기 (뒤에서). volume 0..1 — 0 이면 안 함
pub fn speak(text: &str, volume: f32) {
    if volume <= 0.0 || text.trim().is_empty() {
        return;
    }
    let t = text.replace('\'', "''");
    let vol = (volume.clamp(0.0, 1.0) * 100.0).round() as i32;
    let script = format!(
        "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $v = $s.GetInstalledVoices() | Where-Object {{ $_.VoiceInfo.Culture.Name -eq 'ko-KR' }} | Select-Object -First 1; \
         if ($v) {{ $s.SelectVoice($v.VoiceInfo.Name) }}; $s.Volume = {vol}; $s.Rate = 0; $s.Speak('{t}')"
    );
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let enc = crate::sync::b64(&utf16);
    let what = text.to_string();
    std::thread::spawn(move || {
        let r = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &enc]).creation_flags(0x0800_0000).status();
        if !matches!(r, Ok(s) if s.success()) {
            crate::store::log(format!("tts failed ({what}): {r:?}"));
        }
    });
}
