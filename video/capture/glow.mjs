export async function installGlow(shader) {
  const adapter = await navigator.gpu?.requestAdapter();
  if (!adapter)
    throw new Error("WebGPU is required to capture the Glowtune shader.");
  const device = await adapter.requestDevice();
  const canvas = document.createElement("canvas");
  canvas.width = innerWidth;
  canvas.height = innerHeight;
  Object.assign(canvas.style, {
    position: "fixed",
    inset: 0,
    pointerEvents: "none",
    zIndex: 100000,
  });
  document.body.append(canvas);

  const context = canvas.getContext("webgpu");
  const format = navigator.gpu.getPreferredCanvasFormat();
  context.configure({ device, format, alphaMode: "premultiplied" });
  const module = device.createShaderModule({ code: shader });
  const pipeline = await device.createRenderPipelineAsync({
    layout: "auto",
    vertex: { module, entryPoint: "vs" },
    fragment: { module, entryPoint: "fs", targets: [{ format }] },
  });
  const values = new Float32Array(64);
  const buffer = device.createBuffer({
    size: values.byteLength,
    usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
  });
  const bind = device.createBindGroup({
    layout: pipeline.getBindGroupLayout(0),
    entries: [{ binding: 0, resource: { buffer } }],
  });
  const { width, height } = canvas;
  values.set([0, 0, width, height, width, height, 110, 26]);
  values.set([0, 2, 2, 0, 0, 1, 1, 0.35, 1, 0.35, 1.2, 1], 12);
  const linear = (c) =>
    c < 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  let prevPalette;
  let fade = 0;
  function render() {
    const { status, reportedAtMs, palette, energy } = window.capturePlayback();
    const t = Math.max(0, Date.now() - reportedAtMs) / 1000;
    fade += ((status === "playing" ? 1 : 0) - fade) * 0.06;
    if (palette !== prevPalette) {
      for (let i = 0; i < palette.length; i++) {
        const rgb = palette[i]
          .slice(1)
          .match(/../g)
          .map((part) => linear(parseInt(part, 16) / 255));
        values.set(rgb, 24 + i * 4);
        values.set(rgb, 44 + i * 4);
      }
      prevPalette = palette;
    }
    values.set([t, fade, energy ?? Math.exp(-7 * (t % 0.5)), 1.05], 8);
    values[15] = t * 0.029;
    device.queue.writeBuffer(buffer, 0, values);
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [
        {
          view: context.getCurrentTexture().createView(),
          loadOp: "clear",
          storeOp: "store",
          clearValue: [0, 0, 0, 0],
        },
      ],
    });
    pass.setPipeline(pipeline);
    pass.setBindGroup(0, bind);
    pass.draw(3);
    pass.end();
    device.queue.submit([encoder.finish()]);
    requestAnimationFrame(render);
  }
  render();
}
