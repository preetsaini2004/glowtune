//! Step 1: load the stem model, print its inputs/outputs, time one 7.8 s
//! segment. Usage: inspect <model.onnx> <dml|cpu>
use std::time::Instant;

use ort::{ep, session::Session, value::Tensor};

fn main() -> ort::Result<()> {
    let model = std::env::args().nth(1).expect("path to htdemucs .onnx");
    let gpu = std::env::args().nth(2).as_deref() == Some("dml");
    let env = ort::init().with_name("stems-proof").build()?;
    let t = Instant::now();
    let mut builder = Session::builder(&env)?;
    if gpu {
        // DirectML can't use ONNX Runtime's memory patterns or parallel runs.
        builder = builder
            .with_memory_pattern(false)?
            .with_parallel_execution(false)?
            .with_execution_providers([ep::DirectML::default().build().error_on_failure()])?;
    }
    let mut session = builder.commit_from_file(&model)?;
    println!(
        "== {} (load {:.1} s)",
        if gpu { "DirectML" } else { "CPU" },
        t.elapsed().as_secs_f32()
    );
    let name = session.inputs()[0].name().to_string();
    let n = 343_980usize;
    for run in 0..3 {
        let data: Vec<f32> = (0..2 * n)
            .map(|i| ((i as f32) * 0.01).sin() * 0.1)
            .collect();
        let input = Tensor::from_array(([1usize, 2, n], data))?;
        let t = Instant::now();
        let outputs = session.run(ort::inputs![name.as_str() => input])?;
        let dt = t.elapsed().as_secs_f32();
        let (shape, _) = outputs[0].try_extract_tensor::<f32>()?;
        println!("run {run}: {dt:.2} s for 7.8 s of audio, output {shape:?}");
    }
    Ok(())
}
