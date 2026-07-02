// 极简测试：只测试StartTask
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use prost::Message;

mod asr {
    include!(concat!(env!("OUT_DIR"), "/asr.rs"));
}

use asr::{AsrRequest, AsrResponse};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "RTIHIRzbwS";

    println!("========== 测试1: 只发送StartTask ==========\n");

    let ws_url = format!("wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?token={}", token);
    println!("连接 WebSocket...");
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
    println!("  protobuf 长度: {} 字节", buf.len());
    write.send(WsMessage::Binary(buf)).await?;
    println!("✓ 已发送");

    // 接收响应
    println!("\n等待响应...");
    match tokio::time::timeout(std::time::Duration::from_secs(5), read.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => {
            println!("✗ 收到JSON错误:");
            println!("{}", text);

            // 解析JSON查看详情
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                println!("\n错误详情:");
                println!("  event: {}", json.get("event").and_then(|v| v.as_str()).unwrap_or("N/A"));
                println!("  status_code: {}", json.get("status_code").and_then(|v| v.as_i64()).unwrap_or(0));
                println!("  status_text: {}", json.get("status_text").and_then(|v| v.as_str()).unwrap_or("N/A"));
            }
        }
        Ok(Some(Ok(WsMessage::Binary(data)))) => {
            let response = AsrResponse::decode(bytes::Bytes::from(data))?;
            println!("✓ 收到protobuf响应:");
            println!("  message_type: {}", response.message_type);
            println!("  status_code: {}", response.status_code);
            println!("  status_message: {}", response.status_message);
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
