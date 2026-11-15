use core::ffi::{c_int,c_float,c_double};
#[repr(C)] pub struct signalsmith_stretch_t { _opaque: [u8;0] }
unsafe extern "C" {
pub fn signalsmith_stretch_create(ch:c_int,block:usize,interval:usize)->*mut signalsmith_stretch_t;
pub fn signalsmith_stretch_create_preset_default(ch:c_int,sr:c_float)->*mut signalsmith_stretch_t;
pub fn signalsmith_stretch_create_preset_cheaper(ch:c_int,sr:c_float)->*mut signalsmith_stretch_t;
pub fn signalsmith_stretch_destroy(h:*mut signalsmith_stretch_t);
pub fn signalsmith_stretch_reset(h:*mut signalsmith_stretch_t);
pub fn signalsmith_stretch_input_latency(h:*mut signalsmith_stretch_t)->usize;
pub fn signalsmith_stretch_output_latency(h:*mut signalsmith_stretch_t)->usize;
pub fn signalsmith_stretch_seek(h:*mut signalsmith_stretch_t,input:*mut c_float,len:usize,rate:c_double);
pub fn signalsmith_stretch_process(h:*mut signalsmith_stretch_t,input:*mut c_float,len:usize,out:*mut c_float,out_len:usize);
pub fn signalsmith_stretch_exact(h:*mut signalsmith_stretch_t,input:*mut c_float,len:usize,out:*mut c_float,out_len:usize)->bool;
pub fn signalsmith_stretch_flush(h:*mut signalsmith_stretch_t,out:*mut c_float,len:usize);
pub fn signalsmith_stretch_set_transpose_factor(h:*mut signalsmith_stretch_t,f:c_float,t:c_float);
pub fn signalsmith_stretch_set_transpose_factor_semitones(h:*mut signalsmith_stretch_t,f:c_float,t:c_float);
pub fn signalsmith_stretch_set_formant_factor(h:*mut signalsmith_stretch_t,f:c_float,comp:c_int);
pub fn signalsmith_stretch_set_formant_factor_semitones(h:*mut signalsmith_stretch_t,f:c_float,comp:c_int);
pub fn signalsmith_stretch_set_formant_base(h:*mut signalsmith_stretch_t,f:c_float);
}
