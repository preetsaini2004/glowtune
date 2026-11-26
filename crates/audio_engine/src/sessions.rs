//! The Windows audio sessions that belong to one process tree (a service
//! window's WebView2 browser process and its helpers).
//!
//! ```text
//!  browser pid ─► process tree (toolhelp) ─► audio sessions on the output device
//!                                              └─ ISimpleAudioVolume: mute / volume
//! ```

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::{
    IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::core::{GUID, Interface};

/// `root` and every process started (directly or not) by it.
pub fn process_tree(root: u32) -> Vec<u32> {
    let mut pairs = Vec::new();
    // SAFETY: plain toolhelp snapshot walk; the handle is closed below.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return vec![root];
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                pairs.push((entry.th32ProcessID, entry.th32ParentProcessID));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    let mut tree = vec![root];
    let mut i = 0;
    while i < tree.len() {
        let parent = tree[i];
        for &(pid, ppid) in &pairs {
            if ppid == parent && pid != 0 && !tree.contains(&pid) {
                tree.push(pid);
            }
        }
        i += 1;
    }
    tree
}

/// The main processes of a program (`exe`, e.g. "spotify.exe", any case):
/// those whose parent isn't the same program. Their trees hold every
/// helper (a browser's audio process).
pub fn roots_named(exe: &str) -> Vec<u32> {
    let mut all = Vec::new();
    // SAFETY: plain toolhelp snapshot walk; the handle is closed below.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Vec::new();
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                all.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    let named: Vec<(u32, u32)> = all
        .iter()
        .filter(|(_, _, n)| n.eq_ignore_ascii_case(exe))
        .map(|(p, pp, _)| (*p, *pp))
        .collect();
    named
        .iter()
        .filter(|(_, parent)| !named.iter().any(|(p, _)| p == parent))
        .map(|(p, _)| *p)
        .collect()
}

/// Volume controls of every audio session owned by `pids` on the default output.
/// COM must be initialized on the calling thread.
pub fn session_volumes(pids: &[u32]) -> windows::core::Result<Vec<(u32, ISimpleAudioVolume)>> {
    let mut out = Vec::new();
    // SAFETY: standard COM calls; interfaces are reference counted by `windows`.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let sessions = manager.GetSessionEnumerator()?;
        for i in 0..sessions.GetCount()? {
            let control: IAudioSessionControl2 = sessions.GetSession(i)?.cast()?;
            let pid = control.GetProcessId().unwrap_or(0);
            if pids.contains(&pid) {
                out.push((pid, control.cast::<ISimpleAudioVolume>()?));
            }
        }
    }
    Ok(out)
}

/// Mutes or unmutes those sessions. Returns how many were changed.
pub fn set_muted(pids: &[u32], muted: bool) -> windows::core::Result<usize> {
    let sessions = session_volumes(pids)?;
    for (_, volume) in &sessions {
        // SAFETY: COM call on a live interface.
        unsafe { volume.SetMute(muted, &GUID::zeroed())? };
    }
    Ok(sessions.len())
}

/// The service window keeps playing, but at this level: 80 dB down, far too
/// quiet to hear. The captured copy has the same level and, being 32-bit
/// float, loses nothing when it is turned back up.
pub const WHISPER: f32 = 1e-4;

/// Turns the tree's sessions down to [`WHISPER`]. Returns `(sessions found,
/// changes made)`. `remembered` collects the volume each session had before,
/// for [`restore`].
pub fn whisper(
    pids: &[u32],
    remembered: &mut Vec<(u32, f32)>,
) -> windows::core::Result<(usize, usize)> {
    let sessions = session_volumes(pids)?;
    let mut changed = 0;
    for (pid, volume) in &sessions {
        // SAFETY: COM calls on a live interface.
        unsafe {
            let now = volume.GetMasterVolume()?;
            if (now - WHISPER).abs() > WHISPER * 0.01 {
                if !remembered.iter().any(|(p, _)| p == pid) {
                    // A whisper left behind by a crash is not a real setting.
                    remembered.push((*pid, if now < WHISPER * 10.0 { 1.0 } else { now }));
                }
                volume.SetMasterVolume(WHISPER, &GUID::zeroed())?;
                changed += 1;
            }
            if volume.GetMute()?.as_bool() {
                volume.SetMute(false, &GUID::zeroed())?;
                changed += 1;
            }
        }
    }
    Ok((sessions.len(), changed))
}

/// Gives the tree's sessions their normal volume back.
pub fn restore(pids: &[u32], remembered: &[(u32, f32)]) -> windows::core::Result<()> {
    for (pid, volume) in session_volumes(pids)? {
        let level = remembered
            .iter()
            .find(|(p, _)| *p == pid)
            .map_or(1.0, |(_, v)| *v);
        // SAFETY: COM calls on a live interface.
        unsafe {
            if volume.GetMasterVolume()? < WHISPER * 10.0 {
                volume.SetMasterVolume(level, &GUID::zeroed())?;
            }
        }
    }
    Ok(())
}

/// One line per audio session on the default output (for diagnostics).
pub fn describe_all() -> windows::core::Result<Vec<String>> {
    let mut out = Vec::new();
    // SAFETY: standard COM calls; returned strings are freed by `to_string` + CoTaskMemFree.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let sessions = manager.GetSessionEnumerator()?;
        for i in 0..sessions.GetCount()? {
            let control: IAudioSessionControl2 = sessions.GetSession(i)?.cast()?;
            let volume: ISimpleAudioVolume = control.cast()?;
            let id = control.GetSessionIdentifier()?;
            let text = id.to_string().unwrap_or_default();
            windows::Win32::System::Com::CoTaskMemFree(Some(id.0.cast()));
            out.push(format!(
                "pid={} state={} volume={:.4} muted={} id={}",
                control.GetProcessId().unwrap_or(0),
                control.GetState()?.0,
                volume.GetMasterVolume()?,
                volume.GetMute()?.as_bool(),
                text
            ));
        }
    }
    Ok(out)
}
