// 独立测试ASR流程
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use prost::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = "RTIHIRzbwS";
    let ws_url = format!("wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?token={}", token);

    println!("1. 连接 WebSocket: {}", ws_url);
    let (ws_stream, _) = connect_async(&ws_url).await?;
    println!("✓ WebSocket 连接成功");

    let (mut write, mut read) = ws_stream.split();

    let request_id = uuid::Uuid::new_v4().to_string();

    // 2. 发送 StartTask
    println!("\n2. 发送 StartTask...");
    let start_task = asr::AsrRequest {
        token: token.to_string(),
        service_name: "ASR".to_string(),
        method_name: "StartTask".to_string(),
        request_id: request_id.clone(),
        ..Default::default()
    };

    let mut buf = Vec::new();
    start_task.encode(&mut buf)?;
    write.send(WsMessage::Binary(buf)).await?;
    println!("✓ StartTask 已发送");

    // 3. 接收 TaskStarted
    println!("\n3. 等待 TaskStarted 响应...");
    if let Some(Ok(WsMessage::Binary(data))) = read.next().await {
        let response = asr::AsrResponse::decode(bytes::Bytes::from(data))?;
        println!("✓ 收到响应: {}", response.message_type);
        if response.message_type != "TaskStarted" {
            println!("✗ 期望 TaskStarted，实际收到: {}", response.message_type);
            return Ok(());
        }
    } else {
        println!("✗ 没有收到响应");
        return Ok(());
    }

    println!("\n✓✓✓ 测试成功！WebSocket 流程正常");
    Ok(())
}

mod asr {
    include!(concat!(env!("OUT_DIR"), "/asr.rs"));
}
