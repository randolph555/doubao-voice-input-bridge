mod proto;
mod modules;

use modules::{credentials::Credentials, asr_client::AsrClient, encoder::AudioEncoder, file_reader::AudioFileReader};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager, State};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

/// 录音会话的共享状态：持有麦克风句柄与停止开关。
#[derive(Default)]
struct RecordingState {
    mic: Mutex<Option<modules::audio::MicCapture>>,
    active: Mutex<bool>,
    stopping: Mutex<bool>,
    result_task: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
struct FileTranscriptionState {
    active: Mutex<bool>,
    cancel_requested: Arc<AtomicBool>,
    task: Mutex<Option<JoinHandle<()>>>,
}

const MAX_FILE_TRANSCRIPTION_ATTEMPTS: usize = 3;

fn is_retryable_transcription_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("service discovery failure") || lower.contains("50700000")
}

async fn emit_transcription_status(
    app: &tauri::AppHandle,
    result: modules::asr_client::TranscriptionResult,
) {
    let _ = app.emit("transcription", result);
}

#[tauri::command]
async fn transcribe_file(
    app: tauri::AppHandle,
    file_state: State<'_, Arc<FileTranscriptionState>>,
    file_path: String,
) -> Result<String, String> {
    eprintln!("========== transcribe_file CALLED ==========");
    eprintln!("File path: {}", file_path);
    println!("transcribe_file called with: {}", file_path);

    let path = PathBuf::from(&file_path);
    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    {
        let mut active = file_state.active.lock().await;
        if *active {
            return Err("已有文件转写任务正在进行中".to_string());
        }
        *active = true;
    }
    file_state.cancel_requested.store(false, Ordering::Relaxed);

    let file_state = Arc::clone(file_state.inner());
    let task_state = Arc::clone(&file_state);
    let handle = tokio::spawn(async move {
        eprintln!("========== SPAWN STARTED ==========");
        if let Err(e) = process_audio_file(app, path).await {
            eprintln!("Error processing file: {}", e);
        }
        task_state.cancel_requested.store(false, Ordering::Relaxed);
        let mut active = task_state.active.lock().await;
        *active = false;
        let mut task = task_state.task.lock().await;
        *task = None;
        eprintln!("========== SPAWN ENDED ==========");
    });
    let mut task = file_state.task.lock().await;
    *task = Some(handle);

    Ok("Processing started".to_string())
}

async fn process_audio_file(app: tauri::AppHandle, path: PathBuf) -> Result<(), String> {
    eprintln!("========== process_audio_file STARTED ==========");
    println!("Reading audio file...");
    let (pcm_data, sample_rate) = tokio::task::spawn_blocking(move || {
        eprintln!("In spawn_blocking for audio file reading");
        AudioFileReader::read_audio_file(&path)
    })
    .await
    .map_err(|e| format!("Task error: {}", e))??;

    eprintln!("Audio file read: {} bytes, sample rate: {}", pcm_data.len(), sample_rate);

    // 编码
    let encode_handle = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<u8>>, String> {
        let mut encoder = AudioEncoder::new(sample_rate)
            .map_err(|e| format!("Failed to create encoder: {}", e))?;

        let pcm_f32: Vec<f32> = pcm_data
            .chunks(2)
            .map(|chunk| {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                sample as f32 / 32768.0
            })
            .collect();

        let frame_size = 320; // 20ms @ 16kHz
        let mut encoded_frames = Vec::new();
        for chunk in pcm_f32.chunks(frame_size) {
            if chunk.len() < frame_size {
                break;
            }
            let encoded = encoder.encode(chunk)
                .map_err(|e| format!("Encoding failed: {}", e))?;
            encoded_frames.push(encoded);
        }
        Ok(encoded_frames)
    });

    let encoded_frames = encode_handle.await
        .map_err(|e| format!("Task join error: {}", e))??;

    println!("Encoded {} frames, starting transcription stream...", encoded_frames.len());

    let file_state = Arc::clone(app.state::<Arc<FileTranscriptionState>>().inner());

    for attempt in 1..=MAX_FILE_TRANSCRIPTION_ATTEMPTS {
        if file_state.cancel_requested.load(Ordering::Relaxed) {
            emit_transcription_status(&app, modules::asr_client::TranscriptionResult::cancelled()).await;
            println!("File transcription cancelled before attempt {}", attempt);
            return Ok(());
        }

        println!("Loading credentials for attempt {}...", attempt);
        let credentials = tokio::task::spawn_blocking(|| {
            eprintln!("In spawn_blocking for credentials");
            Credentials::create_fresh()
                .map_err(|e| format!("Failed to load credentials: {}", e))
        })
        .await
        .map_err(|e| format!("Task error: {}", e))??;
        eprintln!("Credentials loaded successfully for attempt {}", attempt);

        let asr_client = AsrClient::new(credentials);
        let start_result = asr_client
            .transcribe_file(
                encoded_frames.clone(),
                Arc::clone(&file_state.cancel_requested),
            )
            .await
            .map_err(|e| format!("Failed to start transcription attempt {}: {}", attempt, e));
        let mut result_rx = match start_result {
            Ok(rx) => rx,
            Err(message) => {
                if attempt < MAX_FILE_TRANSCRIPTION_ATTEMPTS && is_retryable_transcription_error(&message) {
                    eprintln!("{}; retrying...", message);
                    sleep(Duration::from_millis(700 * attempt as u64)).await;
                    continue;
                }
                emit_transcription_status(&app, modules::asr_client::TranscriptionResult::failed(message.clone())).await;
                return Err(message);
            }
        };

        let mut retryable_failure = None;
        while let Some(result) = result_rx.recv().await {
            if result.status.as_deref() == Some("failed") {
                let error = result.error.clone().unwrap_or_else(|| "unknown transcription error".to_string());
                if attempt < MAX_FILE_TRANSCRIPTION_ATTEMPTS && is_retryable_transcription_error(&error) {
                    retryable_failure = Some(error);
                    break;
                }
            }

            emit_transcription_status(&app, result.clone()).await;

            if result.status.is_some() {
                return Ok(());
            }
        }

        if let Some(error) = retryable_failure {
            eprintln!(
                "Retryable file transcription failure on attempt {}: {}",
                attempt, error
            );
            sleep(Duration::from_millis(700 * attempt as u64)).await;
            continue;
        }

        break;
    }

    println!("Transcription complete");
    Ok(())
}

#[tauri::command]
async fn cancel_transcription(
    app: tauri::AppHandle,
    state: State<'_, Arc<FileTranscriptionState>>,
) -> Result<(), String> {
    let mut active = state.active.lock().await;
    if !*active {
        return Ok(());
    }
    *active = false;
    drop(active);

    state.cancel_requested.store(true, Ordering::Relaxed);
    if let Some(handle) = state.task.lock().await.take() {
        handle.abort();
    }
    emit_transcription_status(&app, modules::asr_client::TranscriptionResult::cancelled()).await;
    println!("文件转写已请求取消");
    Ok(())
}

/// 开始麦克风实时转写。点击按钮触发，一直听到 stop_recording 为止。
#[tauri::command]
async fn start_recording(
    app: tauri::AppHandle,
    state: State<'_, Arc<RecordingState>>,
) -> Result<(), String> {
    {
        let stopping = state.stopping.lock().await;
        if *stopping {
            return Err("录音会话正在停止，请稍候再试".to_string());
        }
    }

    {
        let mut active = state.active.lock().await;
        if *active {
            return Err("已在录音中".to_string());
        }
        *active = true;
    }

    // 加载凭据
    let credentials = match tokio::task::spawn_blocking(|| {
        Credentials::create_fresh()
            .map_err(|e| format!("加载凭据失败: {}", e))
    })
    .await
    .map_err(|e| format!("任务错误: {}", e))
    {
        Ok(Ok(credentials)) => credentials,
        Ok(Err(message)) | Err(message) => {
            let mut active = state.active.lock().await;
            *active = false;
            return Err(message);
        }
    };

    // 启动麦克风采集（原始 f32 样本通过 std channel 送出）
    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<Vec<f32>>();
    let mic = match modules::audio::start_capture(raw_tx) {
        Ok(mic) => mic,
        Err(message) => {
            let mut active = state.active.lock().await;
            *active = false;
            return Err(message);
        }
    };
    let src_rate = mic.sample_rate;
    let src_channels = mic.channels;
    println!("麦克风已启动: {} Hz, {} 声道", src_rate, src_channels);

    // 保存句柄以便停止
    {
        let mut guard = state.mic.lock().await;
        *guard = Some(mic);
    }

    let (audio_tx, audio_rx) = mpsc::channel::<Vec<u8>>(200);

    // 先启动编码线程，预缓冲少量音频帧，再建立 ASR 会话。
    std::thread::spawn(move || {
        let mut encoder = match AudioEncoder::new(16000) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("创建编码器失败: {}", e);
                return;
            }
        };

        let frame_size = 320usize; // 20ms @ 16kHz
        let ratio = src_rate as f32 / 16000.0;
        let mut mono_buf: Vec<f32> = Vec::new();   // 降混后的原始采样率单声道
        let mut resampled: Vec<f32> = Vec::new();  // 16kHz 单声道
        let mut resample_pos = 0f32;

        while let Ok(chunk) = raw_rx.recv() {
            // 1. 降混为单声道
            if src_channels > 1 {
                let ch = src_channels as usize;
                for frame in chunk.chunks(ch) {
                    let sum: f32 = frame.iter().sum();
                    mono_buf.push(sum / ch as f32);
                }
            } else {
                mono_buf.extend_from_slice(&chunk);
            }

            // 2. 线性重采样到 16kHz
            while (resample_pos as usize) < mono_buf.len() {
                let idx = resample_pos as usize;
                resampled.push(mono_buf[idx]);
                resample_pos += ratio;
            }
            // 丢弃已消费的样本，保留小数偏移
            let consumed = resample_pos as usize;
            if consumed > 0 && consumed <= mono_buf.len() {
                mono_buf.drain(0..consumed);
                resample_pos -= consumed as f32;
            }

            // 3. 按 20ms 分帧编码
            while resampled.len() >= frame_size {
                let frame: Vec<f32> = resampled.drain(0..frame_size).collect();
                match encoder.encode(&frame) {
                    Ok(opus) => {
                        if audio_tx.capacity() == 200 {
                            println!("实时录音首帧已进入发送队列");
                        }
                        if audio_tx.blocking_send(opus).is_err() {
                            return; // 接收端已关闭
                        }
                    }
                    Err(e) => eprintln!("编码失败: {}", e),
                }
            }
        }
        // raw_rx 关闭（麦克风停止）→ audio_tx drop → ASR 流收尾
        println!("采集线程结束");
    });

    tokio::time::sleep(Duration::from_millis(120)).await;

    // 建立 ASR 流
    let asr_client = AsrClient::new(credentials);
    let start_result = asr_client
        .transcribe_stream(audio_rx)
        .await
        .map_err(|e| format!("启动转写失败: {}", e));
    let mut result_rx = match start_result {
        Ok(rx) => rx,
        Err(message) => {
            {
                let mut guard = state.mic.lock().await;
                if let Some(mic) = guard.take() {
                    mic.stop();
                }
            }
            let mut active = state.active.lock().await;
            *active = false;
            let mut stopping = state.stopping.lock().await;
            *stopping = false;
            return Err(message);
        }
    };

    // 转发识别结果到前端
    let recording_state = Arc::clone(state.inner());
    let forward_task = tokio::spawn(async move {
        while let Some(result) = result_rx.recv().await {
            let _ = app.emit("transcription", result);
        }
        {
            let mut guard = recording_state.mic.lock().await;
            if let Some(mic) = guard.take() {
                mic.stop();
            }
        }
        {
            let mut active = recording_state.active.lock().await;
            *active = false;
        }
        {
            let mut stopping = recording_state.stopping.lock().await;
            *stopping = false;
        }
        {
            let mut task = recording_state.result_task.lock().await;
            *task = None;
        }
        println!("录音结果流结束");
    });
    let mut task = state.result_task.lock().await;
    *task = Some(forward_task);

    Ok(())
}

/// 停止麦克风录音。
#[tauri::command]
async fn stop_recording(
    state: State<'_, Arc<RecordingState>>,
) -> Result<(), String> {
    {
        let mut stopping = state.stopping.lock().await;
        if *stopping {
            return Ok(());
        }
        *stopping = true;
    }
    {
        let mut guard = state.mic.lock().await;
        if let Some(mic) = guard.take() {
            mic.stop();
        }
    }
    let mut active = state.active.lock().await;
    *active = false;
    println!("录音已停止");
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(Arc::new(RecordingState::default()));
            app.manage(Arc::new(FileTranscriptionState::default()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            transcribe_file,
            cancel_transcription,
            start_recording,
            stop_recording
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
