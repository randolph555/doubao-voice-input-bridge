use opus::{Channels, Application, Encoder as OpusEncoder};

pub struct AudioEncoder {
    encoder: OpusEncoder,
}

impl AudioEncoder {
    pub fn new(sample_rate: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let encoder = OpusEncoder::new(sample_rate, Channels::Mono, Application::Voip)?;

        Ok(Self { encoder })
    }

    pub fn encode(&mut self, pcm: &[f32]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // 转换 f32 到 i16
        let pcm_i16: Vec<i16> = pcm
            .iter()
            .map(|&sample| (sample * 32767.0).clamp(-32768.0, 32767.0) as i16)
            .collect();

        let mut output = vec![0u8; 4000];
        let len = self.encoder.encode(&pcm_i16, &mut output)?;
        output.truncate(len);

        Ok(output)
    }
}
