use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// 麦克风采集句柄。
///
/// cpal 的 `Stream` 不是 `Send`，所以采集在一个独立的 OS 线程里进行，
/// 原始 f32 样本通过 channel 送出。调用 `stop()` 通知线程结束、关闭 channel。
pub struct MicCapture {
    pub sample_rate: u32,
    pub channels: u16,
    stop: Arc<AtomicBool>,
}

impl MicCapture {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// 启动麦克风采集。原始交错 f32 样本推入 `raw_tx`。
pub fn start_capture(raw_tx: Sender<Vec<f32>>) -> Result<MicCapture, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "没有可用的麦克风设备".to_string())?;

    let supported_config = device
        .default_input_config()
        .map_err(|e| format!("获取麦克风配置失败: {}", e))?;

    let sample_format = supported_config.sample_format();
    let sample_rate = supported_config.sample_rate().0;
    let channels = supported_config.channels();
    let stream_config: cpal::StreamConfig = supported_config.into();

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    // 独立线程持有 Stream（Stream 非 Send，不能跨 await/线程移动）
    std::thread::spawn(move || {
        let stream_result = match sample_format {
            SampleFormat::F32 => build_input_stream::<f32>(&device, &stream_config, raw_tx),
            SampleFormat::I16 => build_input_stream::<i16>(&device, &stream_config, raw_tx),
            SampleFormat::U16 => build_input_stream::<u16>(&device, &stream_config, raw_tx),
            other => Err(format!("暂不支持的麦克风采样格式: {}", other)),
        };

        let stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                eprintln!("创建音频流失败: {}", e);
                return;
            }
        };

        if let Err(e) = stream.play() {
            eprintln!("启动音频流失败: {}", e);
            return;
        }

        while !stop_thread.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // 退出：drop stream 停止采集，闭包随之析构 → raw_tx 关闭
        drop(stream);
    });

    Ok(MicCapture {
        sample_rate,
        channels,
        stop,
    })
}

fn build_input_stream<T>(
    device: &cpal::Device,
    stream_config: &cpal::StreamConfig,
    raw_tx: Sender<Vec<f32>>,
) -> Result<cpal::Stream, String>
where
    T: Sample + SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            stream_config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                // 回调必须快速返回：仅做采样格式转换和复制，编码在别处做
                let converted: Vec<f32> = data.iter().map(|&sample| sample.to_sample::<f32>()).collect();
                let _ = raw_tx.send(converted);
            },
            |err| eprintln!("麦克风采集错误: {}", err),
            None,
        )
        .map_err(|e| e.to_string())
}
