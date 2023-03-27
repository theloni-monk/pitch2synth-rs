use bus::{Bus,BusReader};

use crate::MIN_FREQ;
use crate::NUM_FREQS;
use crate::SNAPSHOT_BUFFLEN;
mod goertzel;


pub struct PitchEstimatorThread{
    audio_rx:BusReader<[(f32, f32); SNAPSHOT_BUFFLEN]>,
    pitch_tx: Bus<(f32, f32, bool, f32)>, //sends (frequency float in hz, voiced bool, voiced probability float)
    spec_tx:Bus<[u64;NUM_FREQS]>,
    waveform_snapshot: [(f32,f32); SNAPSHOT_BUFFLEN],
    predictor: goertzel::GoertzelEstimator,
    cthresh: f32
}

impl PitchEstimatorThread{
    pub fn new(sr:usize, snapshot_ref:BusReader<[(f32, f32); SNAPSHOT_BUFFLEN]>, f0_tx:Bus<(f32, f32, bool, f32)>, spec_tx:Bus<[u64;NUM_FREQS]>, cthresh:f32) -> PitchEstimatorThread{
        let detector =goertzel::GoertzelEstimator::new(MIN_FREQ, sr as f32);
        PitchEstimatorThread { 
            audio_rx: snapshot_ref, 
            pitch_tx: f0_tx, 
            spec_tx: spec_tx,
            waveform_snapshot: [(0.0, 0.0); SNAPSHOT_BUFFLEN], 
            predictor: detector,
            cthresh: cthresh 
        }
    }
    pub fn run(&mut self){
        loop{
            self.waveform_snapshot = self.audio_rx.recv().unwrap();
            let timestamp = self.waveform_snapshot[0].0;
            let amps = self.waveform_snapshot.map(|el| el.1);
            self.predictor.process(&amps);
            let pitch = self.predictor.get_pitch();
            self.spec_tx.broadcast(self.predictor.gvec.map(|el| el as u64));
            self.pitch_tx.broadcast((timestamp, pitch.0, pitch.1>self.cthresh, pitch.1));
        }
    }
}