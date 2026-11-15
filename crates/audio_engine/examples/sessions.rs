//! Lists audio sessions on the default output, or sets the volume of one
//! process tree: `sessions [<root pid> <volume 0..1>]`.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use audio_engine::sessions;
    use windows::core::GUID;
    wasapi::initialize_mta().ok()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [pid, volume] = args.as_slice() {
        let tree = sessions::process_tree(pid.parse()?);
        for (pid, v) in sessions::session_volumes(&tree)? {
            // SAFETY: COM call on a live interface.
            unsafe { v.SetMasterVolume(volume.parse()?, &GUID::zeroed())? };
            println!("set pid {pid}");
        }
    }
    for line in sessions::describe_all()? {
        println!("{line}");
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
