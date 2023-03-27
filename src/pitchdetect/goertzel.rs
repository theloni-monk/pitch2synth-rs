use std::array;

use crate::SNAPSHOT_BUFFLEN;
use crate::NUM_FREQS;
use crate::NOISE_THRESH;

use super::NUM_FRAMES_CONCAT;
//TODO: tune thresh


fn argmax(slice: &[f32]) -> i8 {
    let mut max = 0.0f32;
    let mut max_idx:i8 = -1;
    for i in 1..slice.len(){
        if slice[i]>max {
            max_idx = i as i8;
            max = slice[i];
        }
    }
    return max_idx;
}
// FIXME: not behaving as usual
// TODO: execute goertzel for 4 freqs at once via SIMD
pub fn goertzel(buff:&[f32], target_freq:f32, srate:f32) -> f32{
    let k = (0.5+((buff.len() as f32 * target_freq)/srate)).floor();
    let w = (2.0*std::f32::consts::PI/(buff.len() as f32)) * k;

    let coeff = 2.0 * w.cos();

    let mut q0;
    let mut q1 = 0.0;
    let mut q2 = 0.0;
    for i in 1..buff.len(){
        q0 = coeff * q1 - q2 + buff[i];
        q2 = q1;
        q1 = q0;
    }

    let magsquared = q1*q1 + (q2*q2) - (q1*q2*coeff);
    return magsquared.sqrt();
}

pub struct GoertzelEstimator{
    thresh: f32,
    target_freqs: [f32; NUM_FREQS],
    pub gvec: [f32; NUM_FREQS],
    srate:f32
}

impl GoertzelEstimator{
    pub fn new(min_freq:f32, srate:f32) -> GoertzelEstimator{
        let tw_root_of_two:f32 = 2.0f32.powf(1.0/12.0);

        let freq_array:[f32;NUM_FREQS] = array::from_fn(|i|{
            min_freq * tw_root_of_two.powf(i as f32)
        });
        //println!("{:?}", freq_array);
        GoertzelEstimator{
            target_freqs: freq_array,
            thresh: NOISE_THRESH,
            gvec: [0.0; NUM_FREQS],
            srate:srate
        }
    }

    pub fn process(&mut self, buff:&[f32;SNAPSHOT_BUFFLEN*NUM_FRAMES_CONCAT]){
        self.gvec = array::from_fn(|i| goertzel(buff, self.target_freqs[i], self.srate));
    }

    pub fn get_pitch(&mut self)->(f32, f32){
        let idx = argmax(&self.gvec);
        if idx == -1 {
            return (0.0, 0.0);
        }
        if self.gvec[idx as usize] < self.thresh {
            return (0.0, 0.0);
        }
        return (self.target_freqs[idx as usize], self.gvec[idx as usize]);
    }
}