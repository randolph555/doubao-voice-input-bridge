// 测试：尝试直接用原项目的凭据文件
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use prost::Message;

mod asr {
    include!(concat!(env!("OUT_DIR"), "/asr.rs"));
}

use asr::{AsrRequest, AsrResponse};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("========== 测试: 读取原项目凭据 ==========\n");

    // 读取原项目的凭据
    let creds_path = "/Users/lijunke/vbcode/doubaoime-asr/credentials.json";
    let creds_data = std::fs::read_to_string(creds_path)?;
    let creds: serde_json::Value = serde_json::from_str(&creds_data)?;
    let token = creds["token"].as_str().ok_or("no token")?;

    println!("Token: {}", token);
    println!("Device ID: {}", creds["device_id"].as_str().unwrap_or("N/A"));

    println!("\n连接 WebSocket...");
    let ws_url = format!("wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?token={}", token);
    let (ws_stream, _) = connect_async(&ws_url).await?;
    println!("✓ 连接成功");

    let (mut write, mut read) = ws_stream.split();
    let request_id = uuid::Uuid::new_v4().to_string();

    // 发送 StartTask
    println!("\n发送 StartTask...");
    let request = AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "StartTask".to_string(),
        request_id: request_id.clone(),
        ..Default::default()
    };

    let mut buf = Vec::new();
    request.encode(&mut buf)?;
    println!("protobuf数据: {} 字节", buf.len());
    println!("Hex: {}", hex::encode(&buf));

    write.send(WsMessage::Binary(buf)).await?;
    println!("✓ 已发送");

    // 接收响应
    println!("\n等待响应...");
    match tokio::time::timeout(std::time::Duration::from_secs(5), read.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => {
            println!("✗ 收到JSON错误: {}", text);
        }
        Ok(Some(Ok(WsMessage::Binary(data)))) => {
            println!("✓✓✓ 成功！收到protobuf响应:");
            println!("Raw hex: {}", hex::encode(&data));

            let response = AsrResponse::decode(bytes::Bytes::from(data))?;
            println!("\n解析结果:");
            println!("  message_type: {}", response.message_type);
            println!("  status_code: {}", response.status_code);
            println!("  status_message: {}", response.status_message);
            println!("  request_id: {}", response.request_id);
            println!("  task_id: {}", response.task_id);
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
