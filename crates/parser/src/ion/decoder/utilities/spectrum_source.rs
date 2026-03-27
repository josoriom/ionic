pub trait SpectrumSource {
    fn for_each_scan_in_range(
        &mut self,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
        callback: &mut dyn FnMut(f64, &[f64], &[f64]), // (rt_minutes, mz_slice, intensity_slice)
    );
}
