use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use std::fs::File;
use std::path::Path;

pub struct AudioFileReader;

impl AudioFileReader {
    pub fn read_audio_file(path: &Path) -> Result<(Vec<u8>, u32), String> {
        // 打开文件
        let file = Box::new(File::open(path).map_err(|e| e.to_string())?);
        let mss = MediaSourceStream::new(file, Default::default());

        // 创建 hint
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        // Probe 文件格式
        let probed = symphonia::default::get_probe().format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        ).map_err(|e| e.to_string())?;

        let mut format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("No audio track found")?;

        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(16000);

        // 创建解码器
        let mut decoder = symphonia::default::get_codecs().make(
            &track.codec_params,
            &DecoderOptions::default(),
        ).map_err(|e| e.to_string())?;

        // 读取所有音频数据
        let mut pcm_data = Vec::new();

        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(_) => break,
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet).map_err(|e| e.to_string())? {
                AudioBufferRef::F32(buf) => {
                    // 转换 f32 到 i16
                    for &sample in buf.chan(0) {
                        let s16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                        pcm_data.extend_from_slice(&s16.to_le_bytes());
                    }
                }
                AudioBufferRef::S16(buf) => {
                    for &sample in buf.chan(0) {
                        pcm_data.extend_from_slice(&sample.to_le_bytes());
                    }
                }
                _ => {
                    return Err("Unsupported audio format".into());
                }
            }
        }

        // 重采样到 16kHz（如果需要）
        let resampled = if sample_rate != 16000 {
            Self::resample(&pcm_data, sample_rate, 16000)
        } else {
            pcm_data
        };

        Ok((resampled, 16000))
    }

    fn resample(data: &[u8], from_rate: u32, to_rate: u32) -> Vec<u8> {
        // 简单的线性插值重采样
        let samples: Vec<i16> = data
            .chunks(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        let ratio = from_rate as f32 / to_rate as f32;
        let new_len = (samples.len() as f32 / ratio) as usize;
        let mut resampled = Vec::new();

        for i in 0..new_len {
            let src_idx = (i as f32 * ratio) as usize;
            if src_idx < samples.len() {
                resampled.extend_from_slice(&samples[src_idx].to_le_bytes());
            }
        }

        resampled
    }
}
