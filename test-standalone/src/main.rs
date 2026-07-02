// 独立测试：验证整个转写流程
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use std::path::Path;

mod asr {
    include!(concat!(env!("OUT_DIR"), "/asr.rs"));
}

use asr::{AsrRequest, AsrResponse};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "RTIHIRzbwS";
    let audio_file = "/Users/lijunke/Downloads/caption_sample2.wav";

    println!("========== 豆包 ASR 测试 ==========\n");

    // 1. 读取音频文件
    println!("1. 读取音频文件: {}", audio_file);
    let (pcm_data, sample_rate) = read_audio_file(audio_file)?;
    println!("   ✓ 读取成功: {} 字节, 采样率 {}", pcm_data.len(), sample_rate);

    // 2. 编码音频
    println!("\n2. 编码音频为 Opus...");
    let frames = encode_audio(&pcm_data, sample_rate)?;
    println!("   ✓ 编码完成: {} 帧", frames.len());

    // 3. 连接 WebSocket
    let ws_url = format!("wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?token={}", token);
    println!("\n3. 连接 WebSocket...");
    let (ws_stream, _) = connect_async(&ws_url).await?;
    println!("   ✓ 连接成功");

    let (mut write, mut read) = ws_stream.split();
    let request_id = uuid::Uuid::new_v4().to_string();

    // 3.1 检查服务器是否主动发送消息
    println!("\n3.1 检查服务器初始消息...");
    match tokio::time::timeout(std::time::Duration::from_millis(500), read.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => {
            println!("   ⚠ 服务器发送了初始JSON消息: {}", text);
            // 如果是TaskFailed/EventInvalidAfterRestart，这是正常的，继续
        }
        Ok(Some(Ok(WsMessage::Binary(data)))) => {
            println!("   ⚠ 服务器发送了初始二进制消息，长度: {}", data.len());
        }
        Ok(Some(Ok(_))) => {
            println!("   ⚠ 服务器发送了其他类型消息");
        }
        Ok(Some(Err(e))) => {
            println!("   ✗ 读取错误: {}", e);
            return Ok(());
        }
        Ok(None) => {
            println!("   ✗ 连接已关闭");
            return Ok(());
        }
        Err(_) => {
            println!("   ✓ 没有初始消息（正常）");
        }
    }

    // 4. StartTask
    println!("\n4. StartTask...");
    send_start_task(&mut write, &request_id, token).await?;

    match tokio::time::timeout(std::time::Duration::from_secs(5), read.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => {
            println!("   ✗ 收到 JSON 错误: {}", text);
            return Ok(());
        }
        Ok(Some(Ok(WsMessage::Binary(data)))) => {
            let response = AsrResponse::decode(bytes::Bytes::from(data))?;
            println!("   ✓ {} (status: {})", response.message_type, response.status_code);
            if response.message_type != "TaskStarted" {
                println!("   ✗ 期望 TaskStarted，收到: {}", response.message_type);
                if !response.status_message.is_empty() {
                    println!("   错误信息: {}", response.status_message);
                }
                return Ok(());
            }
        }
        Ok(Some(Ok(msg))) => {
            println!("   ✗ 收到未知消息类型: {:?}", msg);
            return Ok(());
        }
        Ok(Some(Err(e))) => {
            println!("   ✗ WebSocket 错误: {}", e);
            return Ok(());
        }
        Ok(None) => {
            println!("   ✗ WebSocket 连接关闭");
            return Ok(());
        }
        Err(_) => {
            println!("   ✗ 超时，没有收到响应");
            return Ok(());
        }
    }

    // 5. StartSession
    println!("\n5. StartSession...");
    send_start_session(&mut write, &request_id, token).await?;

    match tokio::time::timeout(std::time::Duration::from_secs(5), read.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => {
            println!("   ✗ 收到 JSON 错误: {}", text);
            return Ok(());
        }
        Ok(Some(Ok(WsMessage::Binary(data)))) => {
            let response = AsrResponse::decode(bytes::Bytes::from(data))?;
            println!("   ✓ {} (status: {})", response.message_type, response.status_code);
            if response.message_type != "SessionStarted" {
                println!("   ✗ 期望 SessionStarted，收到: {}", response.message_type);
                if !response.status_message.is_empty() {
                    println!("   错误信息: {}", response.status_message);
                }
                return Ok(());
            }
        }
        Ok(Some(Ok(msg))) => {
            println!("   ✗ 收到未知消息类型: {:?}", msg);
            return Ok(());
        }
        Ok(Some(Err(e))) => {
            println!("   ✗ WebSocket 错误: {}", e);
            return Ok(());
        }
        Ok(None) => {
            println!("   ✗ WebSocket 连接关闭");
            return Ok(());
        }
        Err(_) => {
            println!("   ✗ 超时，没有收到响应");
            return Ok(());
        }
    }

    // 6. 发送音频帧
    println!("\n6. 发送音频帧...");
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;

    for (i, frame) in frames.iter().enumerate() {
        let frame_state = if i == 0 { 1 } else { 3 };
        let timestamp_ms = start_time + (i as i64) * 20;

        send_audio_frame(&mut write, &request_id, frame, frame_state, timestamp_ms).await?;

        if i % 50 == 0 {
            print!("   进度: {}/{}\r", i, frames.len());
        }

        // 同时接收结果
        if let Ok(Some(Ok(WsMessage::Binary(data)))) = tokio::time::timeout(
            std::time::Duration::from_millis(1),
            read.next()
        ).await {
            if let Ok(response) = AsrResponse::decode(bytes::Bytes::from(data)) {
                if !response.result_json.is_empty() {
                    if let Ok(result) = serde_json::from_str::<serde_json::Value>(&response.result_json) {
                        if let Some(results) = result.get("results").and_then(|r| r.as_array()) {
                            for r in results {
                                if let Some(text) = r.get("text").and_then(|t| t.as_str()) {
                                    println!("\n   【识别】{}", text);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 发送最后一帧
    send_audio_frame(&mut write, &request_id, &[], 9, start_time + (frames.len() as i64) * 20).await?;
    println!("\n   ✓ 所有帧已发送");

    // 7. FinishSession
    println!("\n7. FinishSession...");
    send_finish_session(&mut write, &request_id, token).await?;

    // 8. 接收剩余结果
    println!("\n8. 等待最终结果...");
    while let Some(Ok(WsMessage::Binary(data))) = read.next().await {
        if let Ok(response) = AsrResponse::decode(bytes::Bytes::from(data)) {
            if !response.result_json.is_empty() {
                if let Ok(result) = serde_json::from_str::<serde_json::Value>(&response.result_json) {
                    if let Some(results) = result.get("results").and_then(|r| r.as_array()) {
                        for r in results {
                            if let Some(text) = r.get("text").and_then(|t| t.as_str()) {
                                println!("   【最终】{}", text);
                            }
                        }
                    }
                }
            }

            if response.message_type == "SessionFinished" {
                println!("\n   ✓ Session 结束");
                break;
            }
        }
    }

    println!("\n========== 测试完成 ==========");
    Ok(())
}

fn read_audio_file(path: &str) -> Result<(Vec<u8>, u32), Box<dyn std::error::Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();

    println!("   原始采样率: {}, 声道: {}, 位深: {}",
        spec.sample_rate, spec.channels, spec.bits_per_sample);

    // 读取所有样本
    let samples: Vec<i16> = reader.samples::<i16>().collect::<Result<Vec<_>, _>>()?;

    // 如果是立体声，转为单声道
    let mono_samples: Vec<i16> = if spec.channels == 2 {
        samples.chunks(2).map(|chunk| ((chunk[0] as i32 + chunk[1] as i32) / 2) as i16).collect()
    } else {
        samples
    };

    // 重采样到 16kHz（简单的抽样）
    let target_rate = 16000;
    let resampled: Vec<i16> = if spec.sample_rate != target_rate {
        let ratio = spec.sample_rate as f32 / target_rate as f32;
        (0..((mono_samples.len() as f32 / ratio) as usize))
            .map(|i| mono_samples[(i as f32 * ratio) as usize])
            .collect()
    } else {
        mono_samples
    };

    // 转为字节
    let pcm_bytes: Vec<u8> = resampled.iter().flat_map(|&s| s.to_le_bytes()).collect();

    Ok((pcm_bytes, target_rate))
}

fn encode_audio(pcm_data: &[u8], sample_rate: u32) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    use opus::{Encoder, Application, Channels};

    let mut encoder = Encoder::new(sample_rate, Channels::Mono, Application::Voip)?;

    let pcm_f32: Vec<f32> = pcm_data
        .chunks(2)
        .map(|chunk| {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            sample as f32 / 32768.0
        })
        .collect();

    let frame_size = 320;
    let mut frames = Vec::new();
    let mut output = vec![0u8; 4000];

    for chunk in pcm_f32.chunks(frame_size) {
        if chunk.len() < frame_size {
            break;
        }
        let len = encoder.encode_float(chunk, &mut output)?;
        frames.push(output[..len].to_vec());
    }

    Ok(frames)
}

async fn send_start_task(
    write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        WsMessage,
    >,
    request_id: &str,
    token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "StartTask".to_string(),
        request_id: request_id.to_string(),
        ..Default::default()
    };

    let mut buf = Vec::new();
    request.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    Ok(())
}

async fn send_start_session(
    write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        WsMessage,
    >,
    request_id: &str,
    token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = serde_json::json!({
        "audio_info": {
            "format": "opus",
            "sample_rate": 16000,
            "channel": 1,
            "bits": 16
        },
        "enable_punctuation": true,
        "language": "zh-CN"
    });

    let request = AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "StartSession".to_string(),
        request_id: request_id.to_string(),
        payload: config.to_string(),
        ..Default::default()
    };

    let mut buf = Vec::new();
    request.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    Ok(())
}

async fn send_audio_frame(
    write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        WsMessage,
    >,
    request_id: &str,
    audio_data: &[u8],
    frame_state: i32,
    timestamp_ms: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let payload = serde_json::json!({
        "timestamp_ms": timestamp_ms,
        "extra": {}
    });

    let request = AsrRequest {
        request_id: request_id.to_string(),
        service_name: "ASR".to_string(),
        method_name: "TaskRequest".to_string(),
        frame_state,
        payload: payload.to_string(),
        audio_data: audio_data.to_vec(),
        ..Default::default()
    };

    let mut buf = Vec::new();
    request.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    Ok(())
}

async fn send_finish_session(
    write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        WsMessage,
    >,
    request_id: &str,
    token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "FinishSession".to_string(),
        request_id: request_id.to_string(),
        ..Default::default()
    };

    let mut buf = Vec::new();
    request.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    Ok(())
}
