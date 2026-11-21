//! 음성 안내 — Windows 기본 음성(SAPI, 한국어가 있으면 한국어)을 숨은 PowerShell 로 읽음

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
        use std::ffi::OsStr;
        // 핸들 상속 없이 (crate::proc) — 받아 둔 소켓이 말하는 동안 PowerShell 에 넘어가지 않게
        let r = crate::proc::run_hidden(&crate::proc::powershell(), &[OsStr::new("-NoProfile"), OsStr::new("-NonInteractive"), OsStr::new("-EncodedCommand"), OsStr::new(&enc)], 120_000);
        if !matches!(r, Ok(Some(0))) {
            crate::store::log(format!("tts failed ({what}): {r:?}"));
        }
    });
}
