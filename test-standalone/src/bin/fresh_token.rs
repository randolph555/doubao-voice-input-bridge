// 测试：每次连接使用新的token
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use std::collections::HashMap;

mod asr {
    include!(concat!(env!("OUT_DIR"), "/asr.rs"));
}

use asr::{AsrRequest, AsrResponse};

async fn get_fresh_token() -> Result<String, Box<dyn std::error::Error>> {
    use reqwest::Client;
    use serde_json::json;

    // 生成新的设备ID
    let cdid = uuid::Uuid::new_v4().to_string();
    let openudid = format!("{:016x}", rand::random::<u64>());
    let clientudid = uuid::Uuid::new_v4().to_string();

    // 注册设备
    let client = Client::new();
    let register_url = "https://log.snssdk.com/service/2/device_register/";

    let body = json!({
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
            "openudid": openudid,
            "clientudid": clientudid,
            "cdid": cdid.clone(),
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
        .post(register_url)
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

    println!("✓ 注册新设备: {}", device_id);

    // 获取token
    let rticket = chrono::Utc::now().timestamp_millis().to_string();
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
            ("_rticket", &rticket),
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

    println!("✓ 获取新token: {}...", &token[..token.len().min(15)]);

    Ok(token)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("========== 测试: 使用全新token ==========\n");

    // 获取全新token
    println!("1. 注册新设备并获取token...");
    let token = get_fresh_token().await?;

    println!("\n2. 连接 WebSocket...");
    let ws_url = format!("wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?token={}", token);
    let (ws_stream, _) = connect_async(&ws_url).await?;
    println!("✓ 连接成功");

    let (mut write, mut read) = ws_stream.split();
    let request_id = uuid::Uuid::new_v4().to_string();

    // 发送 StartTask
    println!("\n3. 发送 StartTask...");
    let request = AsrRequest {
        token: token.clone(),
        service_name: "ASR".to_string(),
        method_name: "StartTask".to_string(),
        request_id: request_id.clone(),
        ..Default::default()
    };

    let mut buf = Vec::new();
    request.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    println!("✓ 已发送");

    // 接收响应
    println!("\n4. 等待响应...");
    match tokio::time::timeout(std::time::Duration::from_secs(5), read.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => {
            println!("✗ 收到JSON错误: {}", text);
        }
        Ok(Some(Ok(WsMessage::Binary(data)))) => {
            let response = AsrResponse::decode(bytes::Bytes::from(data))?;
            println!("✓✓✓ 成功！收到protobuf响应:");
            println!("  message_type: {}", response.message_type);
            println!("  status_code: {}", response.status_code);
        }
        Ok(Some(Ok(msg))) => {
            println!("✗ 收到其他类型消息: {:?}", msg);
        }
        Ok(Some(Err(e))) => {
            println!("✗ WebSocket错误: {}", e);
        }
        Ok(None) => {
            println!("✗ 连接关闭");
        }
        Err(_) => {
            println!("✗ 超时");
        }
    }

    Ok(())
}
