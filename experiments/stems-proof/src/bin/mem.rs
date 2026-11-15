//! Memory per stage. Usage: mem <model.onnx> <cpu|dml> <opt: none|basic|all> <arena: on|off>
use std::time::Instant;

use ort::{
    ep,
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};

fn mb() -> (f64, f64) {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    };
    let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut _ as *mut _, c.cb) };
    (
        c.PrivateUsage as f64 / 1048576.0,
        c.PeakWorkingSetSize as f64 / 1048576.0,
    )
}

fn main() -> ort::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (model, gpu) = (&a[1], a[2] == "dml");
    let opt = match a[3].as_str() {
        "none" => GraphOptimizationLevel::Disable,
        "basic" => GraphOptimizationLevel::Level1,
        _ => GraphOptimizationLevel::All,
    };
    let arena = a[4] == "on";
    let env = ort::init().with_name("stems-proof").build()?;
    let mut b = Session::builder(&env)?
        .with_optimization_level(opt)?
        .with_memory_pattern(!gpu && arena)?
        .with_intra_threads(4)?;
    b = if gpu {
        b.with_parallel_execution(false)?
            .with_execution_providers([ep::DirectML::default().build().error_on_failure()])?
    } else {
        b.with_execution_providers([ep::CPU::default().with_arena_allocator(arena).build()])?
    };
    let t = Instant::now();
    let mut s = b.commit_from_file(model)?;
    let (p, pk) = mb();
    println!(
        "loaded {:.1}s  private {p:.0} MB  peak {pk:.0} MB",
        t.elapsed().as_secs_f32()
    );
    let name = s.inputs()[0].name().to_string();
    let n = 343_980usize;
    for run in 0..2 {
        let data: Vec<f32> = (0..2 * n)
            .map(|i| ((i as f32) * 0.01).sin() * 0.1)
            .collect();
        let t = Instant::now();
        let out =
            s.run(ort::inputs![name.as_str() => Tensor::from_array(([1usize, 2, n], data))?])?;
        let dt = t.elapsed().as_secs_f32();
        drop(out);
        let (p, pk) = mb();
        println!("run {run}: {dt:.2}s  private {p:.0} MB  peak {pk:.0} MB");
    }
    Ok(())
}
