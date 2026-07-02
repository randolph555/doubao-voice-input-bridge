// 最终测试：完全模拟Python的行为
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use prost::Message;

mod asr {
    include!(concat!(env!("OUT_DIR"), "/asr.rs"));
}

use asr::{AsrRequest, AsrResponse, FrameState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 读取音频
    let audio_path = "/Users/lijunke/Downloads/caption_sample2.wav";
    let (pcm_data, sample_rate) = read_wav(audio_path)?;
    println!("读取音频: {} 字节, 采样率 {}", pcm_data.len(), sample_rate);

    // 编码
    let frames = encode_opus(&pcm_data)?;
    println!("编码完成: {} 帧\n", frames.len());

    // 连接
    let token = "RTIHIRzbwS";
    let url = format!("wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?token={}", token);
    println!("连接: {}", url);

    let (ws_stream, _) = connect_async(&url).await?;
    println!("✓ 连接成功\n");

    let (mut write, mut read) = ws_stream.split();
    let request_id = uuid::Uuid::new_v4().to_string();

    // StartTask
    println!("发送 StartTask...");
    let req = AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "StartTask".to_string(),
        request_id: request_id.clone(),
        ..Default::default()
    };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;

    let resp = read.next().await.ok_or("no response")??;
    match resp {
        WsMessage::Binary(data) => {
            let resp = AsrResponse::decode(bytes::Bytes::from(data))?;
            println!("✓ {}", resp.message_type);
            if resp.message_type != "TaskStarted" {
                println!("✗ 错误: {}", resp.status_message);
                return Ok(());
            }
        }
        WsMessage::Text(text) => {
            println!("✗ 收到JSON错误: {}", text);
            println!("\n这表明token可能已过期或无效。请运行:");
            println!("  cd /Users/lijunke/vbcode/doubaoime-asr && python3 register_device.py");
            println!("然后重试。");
            return Ok(());
        }
        _ => {
            println!("✗ 未知响应类型");
            return Ok(());
        }
    }

    // StartSession
    println!("\n发送 StartSession...");
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

    let req = AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "StartSession".to_string(),
        request_id: request_id.clone(),
        payload: config.to_string(),
        ..Default::default()
    };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;

    let resp = read.next().await.ok_or("no response")??;
    if let WsMessage::Binary(data) = resp {
        let resp = AsrResponse::decode(bytes::Bytes::from(data))?;
        println!("✓ {}", resp.message_type);
        if resp.message_type != "SessionStarted" {
            println!("✗ 错误: {}", resp.status_message);
            return Ok(());
        }
    } else {
        println!("✗ 非二进制响应");
        return Ok(());
    }

    println!("\n✓✓✓ 初始化成功！开始发送音频...\n");

    // 发送音频（简化版，只发前10帧）
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;

    for (i, frame) in frames.iter().take(10).enumerate() {
        let frame_state = if i == 0 { 1 } else { 3 };
        let timestamp_ms = start_time + (i as i64) * 20;

        let payload = serde_json::json!({
            "timestamp_ms": timestamp_ms,
            "extra": {}
        });

        let req = AsrRequest {
            request_id: request_id.clone(),
            service_name: "ASR".to_string(),
            method_name: "TaskRequest".to_string(),
            frame_state,
            payload: payload.to_string(),
            audio_data: frame.clone(),
            ..Default::default()
        };

        let mut buf = Vec::new();
        req.encode(&mut buf)?;
        write.send(WsMessage::Binary(buf)).await?;

        print!("发送帧 {}...\r", i);

        // 尝试接收结果
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
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
                                    println!("\n【识别】{}", text);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\n\n✓ 测试完成");
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
