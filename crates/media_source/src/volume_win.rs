//! Per-player audio-session volume. Called only by the COM MTA-initialized SMTC worker.
//! Reads the first readable matching session; writes every matching session.

use windows::Win32::Media::Audio::{
    IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, eMultimedia, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::{Interface, Owned, PWSTR};

pub(crate) fn get(app_id: &str) -> Option<f32> {
    matching_sessions(app_id)
        .ok()?
        .into_iter()
        .find_map(|session| {
            // SAFETY: Valid session interface, queried on the initialized worker thread.
            unsafe { session.GetMasterVolume() }
                .ok()
                .and_then(crate::types::clamp_volume)
        })
}

pub(crate) fn set(app_id: &str, volume: f32) -> windows::core::Result<()> {
    let mut result = Ok(());
    for session in matching_sessions(app_id)? {
        // SAFETY: Volume was clamped by MediaWatcher; a null context GUID is permitted.
        if let Err(error) = unsafe { session.SetMasterVolume(volume, std::ptr::null()) } {
            // A disappearing session must not prevent updating the remaining matches.
            result = Err(error);
        }
    }
    result
}

fn matching_sessions(app_id: &str) -> windows::core::Result<Vec<ISimpleAudioVolume>> {
    // SAFETY: The caller is the COM-initialized worker. Interfaces stay on this thread;
    // activation uses no parameters, and session indexes are bounded by GetCount.
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = devices.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let sessions = manager.GetSessionEnumerator()?;
        let mut matches = Vec::new();
        for index in 0..sessions.GetCount()? {
            let Ok(session) = sessions.GetSession(index) else {
                continue;
            };
            let Ok(control) = session.cast::<IAudioSessionControl2>() else {
                continue;
            };
            let Ok(pid) = control.GetProcessId() else {
                continue;
            };
            if let Some(image) = process_image(pid)
                && same_file_name(app_id, &image)
                && let Ok(volume) = session.cast::<ISimpleAudioVolume>()
            {
                matches.push(volume);
            }
        }
        Ok(matches)
    }
}

fn process_image(pid: u32) -> Option<String> {
    // SAFETY: OpenProcess grants only query access. Owned closes the handle on every path.
    let process =
        unsafe { Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?) };
    let mut image = vec![0u16; 32_768];
    let mut len = image.len() as u32;
    // SAFETY: The handle is live; image holds len writable UTF-16 units for this call.
    unsafe {
        QueryFullProcessImageNameW(
            *process,
            PROCESS_NAME_WIN32,
            PWSTR(image.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    String::from_utf16(&image[..len as usize]).ok()
}

fn same_file_name(app_id: &str, image: &str) -> bool {
    fn name(path: &str) -> &str {
        path.rsplit(['\\', '/']).next().unwrap_or_default()
    }
    let app = name(app_id);
    !app.is_empty() && app.to_lowercase() == name(image).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_case_insensitive_file_names() {
        assert!(same_file_name("Spotify.exe", r"C:\Apps\Spotify.EXE"));
        assert!(same_file_name("MÜSIC.exe", r"C:\Apps\müsic.EXE"));
        assert!(same_file_name(
            r"C:\Apps\Spotify.exe",
            "D:/Music/spotify.exe"
        ));
        assert!(!same_file_name("Spotify.exe", r"C:\Apps\SpotifyHelper.exe"));
        assert!(!same_file_name(
            "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic",
            "Music.UI.exe"
        ));
        assert!(!same_file_name("", ""));
        assert!(!same_file_name("Spotify.exe", r"C:\Spotify.exe\"));
    }
}
