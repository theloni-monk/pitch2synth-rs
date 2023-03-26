use pitch_detection::Pitch;
use bus::{Bus,BusReader};
use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;

//TODO: query these
const SNAPSHOT_BUFFLEN:usize = 160;
const SAMPLE_RATE: usize = 48000;

const PADDING: usize = SNAPSHOT_BUFFLEN / 2;
// we capture audio in 20ms chunks so it would be wasteful to attempt to aquire a lock more frequently than 20ms

pub struct PitchEstimator{
    audio_rx:BusReader<[(f32, f32);SNAPSHOT_BUFFLEN]>,
    srate: usize,
    pitch_tx: Bus<(f32, f32, bool, f32)>, //sends (frequency float in hz, voiced bool, voiced probability float)
    waveform_snapshot: [(f32,f32); SNAPSHOT_BUFFLEN],
    predictor: McLeodDetector<f32>,
    pthresh: f32,
    cthresh: f32
}

impl PitchEstimator{
    pub fn new(sr:usize, snapshot_ref:BusReader<[(f32, f32); SNAPSHOT_BUFFLEN]>, tx:Bus<(f32, f32, bool, f32)>, pthresh:f32, cthresh:f32) -> PitchEstimator{
        let detector = McLeodDetector::new(SNAPSHOT_BUFFLEN, PADDING);
        PitchEstimator { 
            audio_rx: snapshot_ref, 
            srate: sr,
            pitch_tx: tx, 
            waveform_snapshot: [(0.0, 0.0); SNAPSHOT_BUFFLEN], 
            predictor: detector,
            pthresh: pthresh,
            cthresh: cthresh 
        }
    }
    pub fn run(&mut self){
        loop{
            self.waveform_snapshot = self.audio_rx.recv().unwrap();
            let timestamp = self.waveform_snapshot[0].0;

            let pitch = self.predictor
            .get_pitch(&self.waveform_snapshot.map(|el| {el.1}), self.srate, self.pthresh, self.cthresh)
            .unwrap_or(Pitch{frequency:0.0,clarity:0.0});

            self.pitch_tx.broadcast((timestamp, pitch.frequency, pitch.clarity>self.cthresh, pitch.clarity));
        }
    }
}