// 终极测试：每次运行都注册新设备、获取新token、立即使用
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use prost::Message;

mod asr {
    include!(concat!(env!("OUT_DIR"), "/asr.rs"));
}

use asr::{AsrRequest, AsrResponse};

async fn register_and_get_token() -> Result<(String, String), Box<dyn std::error::Error>> {
    use reqwest::Client;

    let client = Client::new();

    // 1. 注册设备
    let cdid = uuid::Uuid::new_v4().to_string();
    let openudid = format!("{:016x}", rand::random::<u64>());
    let clientudid = uuid::Uuid::new_v4().to_string();

    let body = serde_json::json!({
        "magic_tag": "ss_app_log",
        "header": {
            "device_id": 0,
            "install_id": 0,
            "aid": 401734,
            "app_name": "oime",
            "version_code": 100102018,
            "version_name": "1.1.2",
            "manifest_version_code": 100102018,
            "update_version_code": 100102018,
            "channel": "official",
            "package": "com.bytedance.android.doubaoime",
            "device_platform": "android",
            "os": "android",
            "os_api": "34",
            "os_version": "16",
            "device_type": "Pixel 7 Pro",
            "device_brand": "google",
            "device_model": "Pixel 7 Pro",
            "resolution": "1080*2400",
            "dpi": "420",
            "language": "zh",
            "timezone": 8,
            "access": "wifi",
            "rom": "UP1A.231005.007",
            "rom_version": "UP1A.231005.007",
            "openudid": &openudid,
            "clientudid": &clientudid,
            "cdid": &cdid,
            "region": "CN",
            "tz_name": "Asia/Shanghai",
            "tz_offset": 28800,
            "sim_region": "cn",
            "carrier_region": "cn",
            "cpu_abi": "arm64-v8a",
            "build_serial": "unknown",
            "not_request_sender": 0,
            "sig_hash": "",
            "google_aid": "",
            "mc": "",
            "serial_number": "",
        },
        "_gen_time": chrono::Utc::now().timestamp_millis(),
    });

    let resp = client
        .post("https://log.snssdk.com/service/2/device_register/")
        .header("User-Agent", "com.bytedance.android.doubaoime/100102018")
        .query(&[
            ("device_platform", "android"),
            ("os", "android"),
            ("ssmix", "a"),
            ("_rticket", &chrono::Utc::now().timestamp_millis().to_string()),
            ("cdid", &cdid),
            ("channel", "official"),
            ("aid", "401734"),
            ("app_name", "oime"),
            ("version_code", "100102018"),
            ("version_name", "1.1.2"),
            ("manifest_version_code", "100102018"),
            ("update_version_code", "100102018"),
            ("resolution", "1080*2400"),
            ("dpi", "420"),
            ("device_type", "Pixel 7 Pro"),
            ("device_brand", "google"),
            ("language", "zh"),
            ("os_api", "34"),
            ("os_version", "16"),
            ("ac", "wifi"),
        ])
        .json(&body)
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let device_id = data["device_id"].as_i64().ok_or("no device_id")?.to_string();

    // 2. 获取token
    let body_str = "body=null";
    let digest = md5::compute(body_str.as_bytes());
    let x_ss_stub = format!("{:X}", digest);

    let resp = client
        .post("https://is.snssdk.com/service/settings/v3/")
        .header("User-Agent", "com.bytedance.android.doubaoime/100102018 (Linux; U; Android 16; en_US; Pixel 7 Pro; Build/BP2A.250605.031.A2; Cronet/TTNetVersion:94cf429a 2025-11-17 QuicVersion:1f89f732 2025-05-08)")
        .header("x-ss-stub", x_ss_stub)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .query(&[
            ("device_platform", "android"),
            ("os", "android"),
            ("ssmix", "a"),
            ("_rticket", &chrono::Utc::now().timestamp_millis().to_string()),
            ("cdid", &cdid),
            ("channel", "official"),
            ("aid", "401734"),
            ("app_name", "oime"),
            ("version_code", "100102018"),
            ("version_name", "1.1.2"),
            ("device_id", &device_id),
        ])
        .body(body_str)
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let token = data["data"]["settings"]["asr_config"]["app_key"]
        .as_str()
        .ok_or("no token")?
        .to_string();

    Ok((device_id, token))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("========== 豆包语音转写 - 全新设备测试 ==========\n");

    // 1. 注册全新设备并获取token
    println!("1. 注册新设备...");
    let (device_id, token) = register_and_get_token().await?;
    println!("   ✓ Device ID: {}", device_id);
    println!("   ✓ Token: {}...\n", &token[..token.len().min(15)]);

    // 2. 读取音频
    println!("2. 读取音频...");
    let (pcm_data, _) = read_wav("/Users/lijunke/Downloads/caption_sample2.wav")?;
    println!("   ✓ {} 字节\n", pcm_data.len());

    // 3. 编码
    println!("3. 编码Opus...");
    let frames = encode_opus(&pcm_data)?;
    println!("   ✓ {} 帧\n", frames.len());

    // 4. **立即连接WebSocket**（不给token任何"过期"的机会）
    //    关键：URL 用 aid+device_id（不是 token），并带上 proto-version=v2 头
    println!("4. 连接WebSocket（使用刚获取的token）...");
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let url = format!(
        "wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?aid=401734&device_id={}",
        device_id
    );
    let mut request = url.into_client_request()?;
    {
        let headers = request.headers_mut();
        headers.insert("User-Agent", "com.bytedance.android.doubaoime/100102018 (Linux; U; Android 16; en_US; Pixel 7 Pro; Build/BP2A.250605.031.A2; Cronet/TTNetVersion:94cf429a 2025-11-17 QuicVersion:1f89f732 2025-05-08)".parse()?);
        headers.insert("proto-version", "v2".parse()?);
        headers.insert("x-custom-keepalive", "true".parse()?);
    }
    let (ws_stream, _) = connect_async(request).await?;
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
            println!("   ✗ 仍然收到JSON错误:");
            println!("   {}\n", text);
            println!("   即使是全新token，依然出现EventInvalidAfterRestart错误。");
            println!("   这说明问题不在token，而在于：");
            println!("   1. Rust的WebSocket库与Python的websockets库行为不同");
            println!("   2. 或者服务器检测到了某些Rust特有的握手行为");
            println!("\n   下一步尝试：使用不同的WebSocket库");
            return Ok(());
        }
        WsMessage::Binary(data) => {
            let resp = AsrResponse::decode(bytes::Bytes::from(data))?;
            println!("   ✓✓✓ 成功！{}\n", resp.message_type);

            if resp.message_type != "TaskStarted" {
                println!("   ✗ 但收到错误: {}", resp.status_message);
                return Ok(());
            }
        }
        _ => {
            println!("   ✗ 未知响应\n");
            return Ok(());
        }
    }

    // 6. StartSession
    println!("6. StartSession...");
    send_start_session(&mut write, &request_id, &token, &device_id).await?;

    let resp = tokio::time::timeout(std::time::Duration::from_secs(5), read.next())
        .await?
        .ok_or("no message")??;

    if let WsMessage::Binary(data) = resp {
        let resp = AsrResponse::decode(bytes::Bytes::from(data))?;
        println!("   ✓ {}\n", resp.message_type);
    } else {
        println!("   ✗ 非二进制响应\n");
        return Ok(());
    }

    // 7. 发送全部音频帧
    println!("7. 发送 {} 帧音频...", frames.len());

    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;

    let total = frames.len();
    for (i, frame) in frames.iter().enumerate() {
        // 第一帧 FIRST(1)，最后一帧 LAST(9)，其余 MIDDLE(3)
        let frame_state = if i == 0 { 1 } else if i == total - 1 { 9 } else { 3 };
        let timestamp_ms = start_time + (i as i64) * 20;
        send_audio_frame(&mut write, &request_id, frame, frame_state, timestamp_ms).await?;
    }
    println!("   ✓ 音频发送完毕");

    // 8. FinishSession
    println!("8. FinishSession...");
    send_finish_session(&mut write, &request_id, &token).await?;

    // 9. 持续接收识别结果，直到 SessionFinished
    println!("9. 接收识别结果...\n");
    let mut final_text = String::new();
    loop {
        let msg = match tokio::time::timeout(std::time::Duration::from_secs(15), read.next()).await {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => { println!("   读取错误: {}", e); break; }
            Ok(None) => { println!("   连接关闭"); break; }
            Err(_) => { println!("   接收超时"); break; }
        };

        if let WsMessage::Binary(data) = msg {
            if let Ok(resp) = AsrResponse::decode(bytes::Bytes::from(data)) {
                if resp.message_type == "SessionFinished" {
                    println!("   会话结束");
                    break;
                }
                if !resp.result_json.is_empty() {
                    if let Ok(result) = serde_json::from_str::<serde_json::Value>(&resp.result_json) {
                        if let Some(results) = result.get("results").and_then(|r| r.as_array()) {
                            for r in results {
                                if let Some(text) = r.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        println!("   【识别中】{}", text);
                                        final_text = text.to_string();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\n========== 最终识别结果 ==========");
    println!("{}", final_text);
    println!("==================================\n");
    Ok(())
}

async fn send_finish_session(write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, WsMessage>, request_id: &str, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    let req = AsrRequest { token: token.to_string(), service_name: "ASR".to_string(), method_name: "FinishSession".to_string(), request_id: request_id.to_string(), ..Default::default() };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    Ok(())
}

// 辅助函数（与之前相同）
async fn send_start_task(write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, WsMessage>, request_id: &str, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    let req = AsrRequest { token: token.to_string(), service_name: "ASR".to_string(), method_name: "StartTask".to_string(), request_id: request_id.to_string(), ..Default::default() };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    Ok(())
}

async fn send_start_session(write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, WsMessage>, request_id: &str, token: &str, device_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = serde_json::json!({
        "audio_info": {"channel": 1, "format": "speech_opus", "sample_rate": 16000},
        "enable_punctuation": true,
        "enable_speech_rejection": false,
        "extra": {
            "app_name": "com.android.chrome",
            "cell_compress_rate": 8,
            "did": device_id,
            "enable_asr_threepass": true,
            "enable_asr_twopass": true,
            "input_mode": "tool"
        }
    });
    let req = AsrRequest { token: token.to_string(), service_name: "ASR".to_string(), method_name: "StartSession".to_string(), request_id: request_id.to_string(), payload: config.to_string(), ..Default::default() };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    Ok(())
}

async fn send_audio_frame(write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, WsMessage>, request_id: &str, audio_data: &[u8], frame_state: i32, timestamp_ms: i64) -> Result<(), Box<dyn std::error::Error>> {
    let payload = serde_json::json!({"timestamp_ms": timestamp_ms, "extra": {}});
    let req = AsrRequest { request_id: request_id.to_string(), service_name: "ASR".to_string(), method_name: "TaskRequest".to_string(), frame_state, payload: payload.to_string(), audio_data: audio_data.to_vec(), ..Default::default() };
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    Ok(())
}

fn read_wav(path: &str) -> Result<(Vec<u8>, u32), Box<dyn std::error::Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<i16> = reader.samples::<i16>().collect::<Result<Vec<_>, _>>()?;
    let mono = if spec.channels == 2 { samples.chunks(2).map(|c| ((c[0] as i32 + c[1] as i32) / 2) as i16).collect() } else { samples };
    let target_rate = 16000;
    let resampled: Vec<i16> = if spec.sample_rate != target_rate { let ratio = spec.sample_rate as f32 / target_rate as f32; (0..((mono.len() as f32 / ratio) as usize)).map(|i| mono[(i as f32 * ratio) as usize]).collect() } else { mono };
    let bytes: Vec<u8> = resampled.iter().flat_map(|&s| s.to_le_bytes()).collect();
    Ok((bytes, target_rate))
}

fn encode_opus(pcm: &[u8]) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    use opus::{Encoder, Application, Channels};
    let mut encoder = Encoder::new(16000, Channels::Mono, Application::Voip)?;
    let pcm_f32: Vec<f32> = pcm.chunks(2).map(|c| { let s = i16::from_le_bytes([c[0], c[1]]); s as f32 / 32768.0 }).collect();
    let mut frames = Vec::new();
    let mut output = vec![0u8; 4000];
    for chunk in pcm_f32.chunks(320) { if chunk.len() < 320 { break; } let len = encoder.encode_float(chunk, &mut output)?; frames.push(output[..len].to_vec()); }
    Ok(frames)
}
