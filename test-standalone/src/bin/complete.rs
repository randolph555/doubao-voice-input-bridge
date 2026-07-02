// 完整实现：精确复制Python的所有逻辑
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use prost::Message;

mod asr {
    include!(concat!(env!("OUT_DIR"), "/asr.rs"));
}

use asr::{AsrRequest, AsrResponse};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("========== 豆包语音转写 - 完整测试 ==========\n");
    
    // 1. 读取音频
    let audio_path = "/Users/lijunke/Downloads/caption_sample2.wav";
    println!("1. 读取音频文件: {}", audio_path);
    let (pcm_data, sample_rate) = read_wav(audio_path)?;
    println!("   ✓ {} 字节, 采样率 {}\n", pcm_data.len(), sample_rate);
    
    // 2. 编码为Opus
    println!("2. 编码为Opus...");
    let frames = encode_opus(&pcm_data)?;
    println!("   ✓ {} 帧\n", frames.len());
    
    // 3. 读取凭据
    println!("3. 读取凭据...");
    let creds_path = "/Users/lijunke/vbcode/doubaoime-asr/credentials.json";
    let creds_data = std::fs::read_to_string(creds_path)?;
    let creds: serde_json::Value = serde_json::from_str(&creds_data)?;
    let token = creds["token"].as_str().ok_or("no token")?.to_string();
    println!("   ✓ Token: {}...\n", &token[..token.len().min(15)]);
    
    // 4. 连接WebSocket
    println!("4. 连接WebSocket...");
    let url = format!("wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?token={}", token);
    let (ws_stream, _) = connect_async(&url).await?;
    println!("   ✓ 连接成功\n");
    
    let (mut write, mut read) = ws_stream.split();
    let request_id = uuid::Uuid::new_v4().to_string();
    
    // 5. StartTask
    println!("5. StartTask...");
    send_start_task(&mut write, &request_id, &token).await?;
    
    let resp = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await?
        .ok_or("no message")??;
    
    match resp {
        WsMessage::Text(text) => {
            println!("   ✗ 收到JSON错误响应:");
            println!("   {}\n", text);
            println!("   这说明token已过期或WebSocket协议有问题。");
            println!("   Python版本可以工作，说明协议本身没问题。");
            println!("   可能的原因:");
            println!("   - Token被服务器标记为\"重启后无效\"");
            println!("   - 需要在同一个TCP连接内完成整个流程");
            return Ok(());
        }
        WsMessage::Binary(data) => {
            let resp = AsrResponse::decode(bytes::Bytes::from(data))?;
            if resp.message_type == "TaskStarted" {
                println!("   ✓ {}\n", resp.message_type);
            } else {
                println!("   ✗ 期望TaskStarted，收到: {}", resp.message_type);
                println!("   状态消息: {}\n", resp.status_message);
                return Ok(());
            }
        }
        _ => {
            println!("   ✗ 未知响应类型\n");
            return Ok(());
        }
    }
    
    // 6. StartSession
    println!("6. StartSession...");
    send_start_session(&mut write, &request_id, &token).await?;
    
    let resp = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await?
        .ok_or("no message")??;
    
    if let WsMessage::Binary(data) = resp {
        let resp = AsrResponse::decode(bytes::Bytes::from(data))?;
        if resp.message_type == "SessionStarted" {
            println!("   ✓ {}\n", resp.message_type);
        } else {
            println!("   ✗ 期望SessionStarted，收到: {}", resp.message_type);
            return Ok(());
        }
    } else {
        println!("   ✗ 非二进制响应\n");
        return Ok(());
    }
    
    // 7. 发送音频帧
    println!("7. 发送音频并接收结果...\n");
    
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;
    
    let mut all_text = String::new();
    
    for (i, frame) in frames.iter().enumerate() {
        let frame_state = if i == 0 { 1 } else if i == frames.len() - 1 { 9 } else { 3 };
        let timestamp_ms = start_time + (i as i64) * 20;
        
        send_audio_frame(&mut write, &request_id, frame, frame_state, timestamp_ms).await?;
        
        if i % 50 == 0 {
            print!("   发送进度: {}/{}\r", i, frames.len());
        }
        
        // 尝试接收结果
        while let Ok(Some(Ok(WsMessage::Binary(data)))) = tokio::time::timeout(
            std::time::Duration::from_millis(1),
            read.next()
        ).await {
            if let Ok(resp) = AsrResponse::decode(bytes::Bytes::from(data)) {
                if !resp.result_json.is_empty() {
                    if let Ok(result) = serde_json::from_str::<serde_json::Value>(&resp.result_json) {
                        if let Some(results) = result.get("results").and_then(|r| r.as_array()) {
                            for r in results {
                                if let Some(text) = r.get("text").and_then(|t| t.as_str()) {
                                    let is_final = !r.get("is_interim").and_then(|f| f.as_bool()).unwrap_or(true);
                                    if is_final {
                                        println!("\n   【最终】{}", text);
                                        all_text.push_str(text);
                                    } else {
                                        print!("\n   【中间】{}\r", text);
                                    }
                                }
                            }
                        }
                    }
                }
                
                if resp.message_type == "SessionFinished" {
                    println!("\n\n   ✓ 会话结束");
                    break;
                }
            }
        }
    }
    
    println!("\n\n8. FinishSession...");
    send_finish_session(&mut write, &request_id, &token).await?;
    
    // 等待最终结果
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    while let Ok(Some(Ok(WsMessage::Binary(data)))) = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        read.next()
    ).await {
        if let Ok(resp) = AsrResponse::decode(bytes::Bytes::from(data)) {
            if !resp.result_json.is_empty() {
                if let Ok(result) = serde_json::from_str::<serde_json::Value>(&resp.result_json) {
                    if let Some(results) = result.get("results").and_then(|r| r.as_array()) {
                        for r in results {
                            if let Some(text) = r.get("text").and_then(|t| t.as_str()) {
                                let is_final = !r.get("is_interim").and_then(|f| f.as_bool()).unwrap_or(true);
                                if is_final && !all_text.contains(text) {
                                    println!("   【最终】{}", text);
                                    all_text.push_str(text);
                                }
                            }
                        }
                    }
                }
            }
            
            if resp.message_type == "SessionFinished" {
                break;
            }
        }
    }
    
    println!("\n========== 转写结果 ==========");
    println!("{}", all_text);
    println!("========== 测试完成 ==========");
    
    Ok(())
}

async fn send_start_task(
    write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        WsMessage,
    >,
    request_id: &str,
    token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let req = AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "StartTask".to_string(),
        request_id: request_id.to_string(),
        ..Default::default()
    };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
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
        "enable_timestamp": false,
        "enable_nlu_recognition": false,
        "enable_word_level_result": false,
        "language": "zh-CN"
    });
    
    let req = AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "StartSession".to_string(),
        request_id: request_id.to_string(),
        payload: config.to_string(),
        ..Default::default()
    };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
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
    
    let req = AsrRequest {
        request_id: request_id.to_string(),
        service_name: "ASR".to_string(),
        method_name: "TaskRequest".to_string(),
        frame_state,
        payload: payload.to_string(),
        audio_data: audio_data.to_vec(),
        ..Default::default()
    };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
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
    let req = AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "FinishSession".to_string(),
        request_id: request_id.to_string(),
        ..Default::default()
    };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    Ok(())
}

fn read_wav(path: &str) -> Result<(Vec<u8>, u32), Box<dyn std::error::Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<i16> = reader.samples::<i16>().collect::<Result<Vec<_>, _>>()?;
    
    let mono = if spec.channels == 2 {
        samples.chunks(2).map(|c| ((c[0] as i32 + c[1] as i32) / 2) as i16).collect()
    } else {
        samples
    };
    
    let target_rate = 16000;
    let resampled: Vec<i16> = if spec.sample_rate != target_rate {
        let ratio = spec.sample_rate as f32 / target_rate as f32;
        (0..((mono.len() as f32 / ratio) as usize))
            .map(|i| mono[(i as f32 * ratio) as usize])
            .collect()
    } else {
        mono
    };
    
    let bytes: Vec<u8> = resampled.iter().flat_map(|&s| s.to_le_bytes()).collect();
    Ok((bytes, target_rate))
}

fn encode_opus(pcm: &[u8]) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    use opus::{Encoder, Application, Channels};
    
    let mut encoder = Encoder::new(16000, Channels::Mono, Application::Voip)?;
    let pcm_f32: Vec<f32> = pcm.chunks(2).map(|c| {
        let s = i16::from_le_bytes([c[0], c[1]]);
        s as f32 / 32768.0
    }).collect();
    
    let mut frames = Vec::new();
    let mut output = vec![0u8; 4000];
    
    for chunk in pcm_f32.chunks(320) {
        if chunk.len() < 320 {
            break;
        }
        let len = encoder.encode_float(chunk, &mut output)?;
        frames.push(output[..len].to_vec());
    }
    
    Ok(frames)
}
