use pitch_detection::Pitch;
use spmc::{Sender, Receiver};
use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;

//TODO: query these
const SNAPSHOT_BUFFLEN:usize = 160;
const SAMPLE_RATE: usize = 48000;

const PADDING: usize = SNAPSHOT_BUFFLEN / 2;
//TODO: tune
const POWER_THRESHOLD: f32 = 2.0;
const CLARITY_THRESHOLD: f32 = 0.4;

pub struct PitchEstimator{
    audio_rx: Receiver<[(f32, f32);SNAPSHOT_BUFFLEN]>,
    pitch_tx: Sender<(f32, f32, bool, f32)>, //sends (frequency float in hz, voiced bool, voiced probability float)
    waveform_snapshot: [f32; SNAPSHOT_BUFFLEN],
    predictor: McLeodDetector<f32> 
}

impl PitchEstimator{
    pub fn new(sr:u32, rx:Receiver<[(f32, f32);SNAPSHOT_BUFFLEN]>, tx:Sender<(f32, f32, bool, f32)>) -> PitchEstimator{
        let detector = McLeodDetector::new(SNAPSHOT_BUFFLEN, PADDING);
        PitchEstimator { 
            audio_rx: rx, 
            pitch_tx: tx, 
            waveform_snapshot: [0.0;SNAPSHOT_BUFFLEN], 
            predictor: detector 
        }
    }
    pub fn run(&mut self){
        loop{
            let buff = self.audio_rx.recv().unwrap();
            self.waveform_snapshot = buff.map(|el|{el.1});
            
            let pitch = self.predictor
            .get_pitch(&self.waveform_snapshot, SAMPLE_RATE, POWER_THRESHOLD, CLARITY_THRESHOLD)
            .unwrap_or(Pitch{frequency:0.0,clarity:0.0});
            
            //TODO: filter pitch output for smoother frequency contour
            self.pitch_tx.send((buff[0].0, pitch.frequency, pitch.clarity>0.2, pitch.clarity)).expect("unable to send pitch compute");
        }
    }
}