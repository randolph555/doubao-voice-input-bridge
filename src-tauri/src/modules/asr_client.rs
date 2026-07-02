use crate::modules::credentials::Credentials;
use crate::proto::asr::{AsrRequest, AsrResponse};
use bytes::Bytes;
use prost::Message;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub is_final: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TranscriptionResult {
    fn text(text: String, is_final: bool) -> Self {
        Self {
            text,
            is_final,
            status: None,
            error: None,
        }
    }

    fn completed() -> Self {
        Self {
            text: String::new(),
            is_final: true,
            status: Some("completed".to_string()),
            error: None,
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            text: String::new(),
            is_final: true,
            status: Some("failed".to_string()),
            error: Some(error.into()),
        }
    }

    pub fn cancelled() -> Self {
        Self {
            text: String::new(),
            is_final: true,
            status: Some("cancelled".to_string()),
            error: None,
        }
    }
}

pub struct AsrClient {
    credentials: Credentials,
}

impl AsrClient {
    pub fn new(credentials: Credentials) -> Self {
        Self { credentials }
    }

    /// 文件转写：一次性传入所有音频帧
    pub async fn transcribe_file(
        &self,
        audio_frames: Vec<Vec<u8>>,
        cancel_requested: Arc<AtomicBool>,
    ) -> Result<mpsc::Receiver<TranscriptionResult>, Box<dyn std::error::Error>> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let ws_url = format!(
            "wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?aid=401734&device_id={}",
            self.credentials.device_id
        );

        println!("Connecting to WebSocket: {}", ws_url);

        let mut request = ws_url.into_client_request()?;
        {
            let headers = request.headers_mut();
            headers.insert("User-Agent", "com.bytedance.android.doubaoime/100102018 (Linux; U; Android 16; en_US; Pixel 7 Pro; Build/BP2A.250605.031.A2; Cronet/TTNetVersion:94cf429a 2025-11-17 QuicVersion:1f89f732 2025-05-08)".parse()?);
            headers.insert("proto-version", "v2".parse()?);
            headers.insert("x-custom-keepalive", "true".parse()?);
        }

        let (ws_stream, _) = connect_async(request).await?;
        println!("WebSocket connected successfully!");

        let (mut write, mut read) = ws_stream.split();
        let (result_tx, result_rx) = mpsc::channel(100);

        let request_id = uuid::Uuid::new_v4().to_string();
        let token = self.credentials.token.clone();
        let device_id = self.credentials.device_id.clone();

        tokio::spawn(async move {
            let mut failed = false;

            // 1. StartTask
            println!("Sending StartTask...");
            if let Err(e) = Self::send_start_task(&mut write, &request_id, &token).await {
                let message = format!("StartTask failed: {}", e);
                drop(e);
                let _ = result_tx
                    .send(TranscriptionResult::failed(message.clone()))
                    .await;
                eprintln!("{}", message);
                return;
            }

            match read.next().await {
                Some(Ok(WsMessage::Binary(data))) => {
                    if let Ok(response) = AsrResponse::decode(Bytes::from(data)) {
                        println!("Received: {}", response.message_type);
                        if response.message_type != "TaskStarted" {
                            let _ = result_tx
                                .send(TranscriptionResult::failed(format!(
                                    "Expected TaskStarted, got {}",
                                    response.message_type
                                )))
                                .await;
                            eprintln!("Expected TaskStarted, got: {}", response.message_type);
                            return;
                        }
                    }
                }
                _ => {
                    let _ = result_tx
                        .send(TranscriptionResult::failed("Failed to receive TaskStarted"))
                        .await;
                    eprintln!("Failed to receive TaskStarted");
                    return;
                }
            }

            // 2. StartSession
            println!("Sending StartSession...");
            if let Err(e) = Self::send_start_session(&mut write, &request_id, &token, &device_id).await {
                let message = format!("StartSession failed: {}", e);
                drop(e);
                let _ = result_tx
                    .send(TranscriptionResult::failed(message.clone()))
                    .await;
                eprintln!("{}", message);
                return;
            }

            match read.next().await {
                Some(Ok(WsMessage::Binary(data))) => {
                    if let Ok(response) = AsrResponse::decode(Bytes::from(data)) {
                        println!("Received: {}", response.message_type);
                        if response.message_type != "SessionStarted" {
                            let _ = result_tx
                                .send(TranscriptionResult::failed(format!(
                                    "Expected SessionStarted, got {}",
                                    response.message_type
                                )))
                                .await;
                            eprintln!("Expected SessionStarted, got: {}", response.message_type);
                            return;
                        }
                    }
                }
                _ => {
                    let _ = result_tx
                        .send(TranscriptionResult::failed("Failed to receive SessionStarted"))
                        .await;
                    eprintln!("Failed to receive SessionStarted");
                    return;
                }
            }

            println!("Session initialized, sending {} frames...", audio_frames.len());

            let start_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let total = audio_frames.len();
            for (i, frame) in audio_frames.iter().enumerate() {
                if cancel_requested.load(Ordering::Relaxed) {
                    let _ = result_tx.send(TranscriptionResult::cancelled()).await;
                    println!("File transcription cancelled before sending frame {}", i);
                    return;
                }

                let frame_state = if i == 0 { 1 } else if i == total - 1 { 9 } else { 3 };
                let timestamp_ms = start_time + (i as i64) * 20;

                if let Err(e) = Self::send_audio_frame(
                    &mut write,
                    &request_id,
                    frame,
                    frame_state,
                    timestamp_ms,
                )
                .await
                {
                    let message = format!("Send audio frame {} failed: {}", i, e);
                    drop(e);
                    let _ = result_tx
                        .send(TranscriptionResult::failed(message.clone()))
                        .await;
                    eprintln!("{}", message);
                    return;
                }
            }

            println!("All frames sent, sending FinishSession...");
            if let Err(e) = Self::send_finish_session(&mut write, &request_id, &token).await {
                let message = format!("FinishSession failed: {}", e);
                drop(e);
                let _ = result_tx
                    .send(TranscriptionResult::failed(message.clone()))
                    .await;
                eprintln!("{}", message);
                return;
            }

            println!("Waiting for final recognition results...");
            let mut last_activity = Instant::now();
            loop {
                if cancel_requested.load(Ordering::Relaxed) {
                    let _ = result_tx.send(TranscriptionResult::cancelled()).await;
                    println!("File transcription cancelled while waiting for results");
                    break;
                }

                if last_activity.elapsed() > Duration::from_secs(30) {
                    failed = true;
                    let _ = result_tx
                        .send(TranscriptionResult::failed("Timed out waiting for backend activity"))
                        .await;
                    eprintln!("Timed out waiting for backend activity");
                    break;
                }

                match tokio::time::timeout(Duration::from_millis(500), read.next()).await {
                    Ok(Some(Ok(WsMessage::Binary(data)))) => {
                        last_activity = Instant::now();
                        if let Ok(response) = AsrResponse::decode(Bytes::from(data)) {
                            eprintln!(
                                "Received: {} (code={}, status={}, extra={})",
                                response.message_type,
                                response.status_code,
                                response.status_message,
                                response.unknown_field_9
                            );

                            if !response.result_json.is_empty() {
                                eprintln!("result_json: {}", response.result_json);
                                if let Ok(result_data) = serde_json::from_str::<serde_json::Value>(&response.result_json) {
                                    if let Some(results) = result_data.get("results").and_then(|r| r.as_array()) {
                                        for result in results {
                                            if let Some(text) = result.get("text").and_then(|t| t.as_str()) {
                                                eprintln!("Got text: {}", text);
                                                let is_final = !result.get("is_interim")
                                                    .and_then(|f| f.as_bool())
                                                    .unwrap_or(true);

                                                if result_tx
                                                    .send(TranscriptionResult::text(text.to_string(), is_final))
                                                    .await
                                                    .is_err()
                                                {
                                                    eprintln!("Failed to send result to channel");
                                                    return;
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            if response.message_type == "SessionFinished" {
                                println!("Session finished");
                                break;
                            }

                            if response.message_type == "SessionFailed" || response.message_type == "TaskFailed" {
                                failed = true;
                                let failure = format!(
                                    "{}: code={}, status={}",
                                    response.message_type,
                                    response.status_code,
                                    response.status_message
                                );
                                let _ = result_tx.send(TranscriptionResult::failed(failure.clone())).await;
                                eprintln!("Session/Task failed: {}", failure);
                                break;
                            }
                        }
                    }
                    Ok(Some(Ok(WsMessage::Text(text)))) => {
                        last_activity = Instant::now();
                        failed = true;
                        let _ = result_tx
                            .send(TranscriptionResult::failed(format!(
                                "Server returned JSON error: {}",
                                text
                            )))
                            .await;
                        eprintln!("Server error (JSON): {}", text);
                        break;
                    }
                    Ok(Some(Ok(_))) => {}
                    Ok(Some(Err(e))) => {
                        last_activity = Instant::now();
                        let message = format!("Read failed: {}", e);
                        failed = true;
                        let _ = result_tx
                            .send(TranscriptionResult::failed(message.clone()))
                            .await;
                        eprintln!("{}", message);
                        break;
                    }
                    Ok(None) => {
                        failed = true;
                        let _ = result_tx
                            .send(TranscriptionResult::failed("Connection closed unexpectedly"))
                            .await;
                        eprintln!("Connection closed");
                        break;
                    }
                    Err(_) => {}
                }
            }

            if !failed {
                let _ = result_tx.send(TranscriptionResult::completed()).await;
            }

            println!("File transcription task ended");
        });

        Ok(result_rx)
    }

    /// 实时流式转写：通过 channel 接收音频帧
    pub async fn transcribe_stream(
        &self,
        mut audio_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<mpsc::Receiver<TranscriptionResult>, Box<dyn std::error::Error>> {
        // 关键：URL 用 aid+device_id（不是 token），并带上 proto-version=v2 头，
        // 否则服务器按 v1 (JSON) 协议处理，会返回 EventInvalidAfterRestart 错误
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let ws_url = format!(
            "wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?aid=401734&device_id={}",
            self.credentials.device_id
        );

        println!("Connecting to WebSocket: {}", ws_url);

        let mut request = ws_url.into_client_request()?;
        {
            let headers = request.headers_mut();
            headers.insert("User-Agent", "com.bytedance.android.doubaoime/100102018 (Linux; U; Android 16; en_US; Pixel 7 Pro; Build/BP2A.250605.031.A2; Cronet/TTNetVersion:94cf429a 2025-11-17 QuicVersion:1f89f732 2025-05-08)".parse()?);
            headers.insert("proto-version", "v2".parse()?);
            headers.insert("x-custom-keepalive", "true".parse()?);
        }

        let (ws_stream, _) = connect_async(request).await?;
        println!("WebSocket connected successfully!");

        let (mut write, mut read) = ws_stream.split();

        let (result_tx, result_rx) = mpsc::channel(100);

        let request_id = uuid::Uuid::new_v4().to_string();
        let token = self.credentials.token.clone();
        let device_id = self.credentials.device_id.clone();

        tokio::spawn(async move {
            let mut failed = false;

            // 1. 发送 StartTask 并等待响应
            println!("Sending StartTask...");
            if let Err(e) = Self::send_start_task(&mut write, &request_id, &token).await {
                let message = format!("StartTask failed: {}", e);
                drop(e);
                let _ = result_tx
                    .send(TranscriptionResult::failed(message.clone()))
                    .await;
                eprintln!("{}", message);
                return;
            }

            // 等待 TaskStarted 响应
            match read.next().await {
                Some(Ok(WsMessage::Binary(data))) => {
                    if let Ok(response) = AsrResponse::decode(Bytes::from(data)) {
                        println!("Received: {}", response.message_type);
                        if response.message_type != "TaskStarted" {
                            let _ = result_tx
                                .send(TranscriptionResult::failed(format!(
                                    "Expected TaskStarted, got {}",
                                    response.message_type
                                )))
                                .await;
                            eprintln!("Expected TaskStarted, got: {}", response.message_type);
                            return;
                        }
                    }
                }
                _ => {
                    let _ = result_tx
                        .send(TranscriptionResult::failed("Failed to receive TaskStarted"))
                        .await;
                    eprintln!("Failed to receive TaskStarted");
                    return;
                }
            }

            // 2. 发送 StartSession 并等待响应
            println!("Sending StartSession...");
            if let Err(e) = Self::send_start_session(&mut write, &request_id, &token, &device_id).await {
                let message = format!("StartSession failed: {}", e);
                drop(e);
                let _ = result_tx
                    .send(TranscriptionResult::failed(message.clone()))
                    .await;
                eprintln!("{}", message);
                return;
            }

            // 等待 SessionStarted 响应
            match read.next().await {
                Some(Ok(WsMessage::Binary(data))) => {
                    if let Ok(response) = AsrResponse::decode(Bytes::from(data)) {
                        println!("Received: {}", response.message_type);
                        if response.message_type != "SessionStarted" {
                            let _ = result_tx
                                .send(TranscriptionResult::failed(format!(
                                    "Expected SessionStarted, got {}",
                                    response.message_type
                                )))
                                .await;
                            eprintln!("Expected SessionStarted, got: {}", response.message_type);
                            return;
                        }
                    }
                }
                _ => {
                    let _ = result_tx
                        .send(TranscriptionResult::failed("Failed to receive SessionStarted"))
                        .await;
                    eprintln!("Failed to receive SessionStarted");
                    return;
                }
            }

            println!("Session initialized successfully!");

            // 3. 同时发送音频数据和接收识别结果
            let mut frame_index = 0;
            let mut audio_done = false;
            let start_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            loop {
                tokio::select! {
                    // 发送音频帧（音频结束后禁用该分支，只读取剩余结果）
                    audio_data = audio_rx.recv(), if !audio_done => {
                        if let Some(audio_data) = audio_data {
                            let timestamp_ms = start_time + frame_index * 20;
                            let frame_state = if frame_index == 0 { 1 } else { 3 };

                            if let Err(e) = Self::send_audio_frame(
                                &mut write,
                                &request_id,
                                &audio_data,
                                frame_state,
                                timestamp_ms,
                            )
                            .await
                            {
                                let message = format!("Send audio frame {} failed: {}", frame_index, e);
                                drop(e);
                                failed = true;
                                let _ = result_tx
                                    .send(TranscriptionResult::failed(message.clone()))
                                    .await;
                                eprintln!("{}", message);
                                break;
                            }
                            frame_index += 1;
                        } else {
                            // 音频流结束：发送 LAST 帧 + FinishSession，
                            // 然后继续读取，直到收到 SessionFinished（关键：不能立即 break）
                            println!("Audio stream ended, sending last frame + FinishSession");
                            let timestamp_ms = start_time + frame_index * 20;
                            if let Err(e) = Self::send_audio_frame(&mut write, &request_id, &[], 9, timestamp_ms).await {
                                let message = format!("Send final audio frame failed: {}", e);
                                drop(e);
                                failed = true;
                                let _ = result_tx
                                    .send(TranscriptionResult::failed(message.clone()))
                                    .await;
                                eprintln!("{}", message);
                                break;
                            }
                            if let Err(e) = Self::send_finish_session(&mut write, &request_id, &token).await {
                                let message = format!("FinishSession failed: {}", e);
                                drop(e);
                                failed = true;
                                let _ = result_tx
                                    .send(TranscriptionResult::failed(message.clone()))
                                    .await;
                                eprintln!("{}", message);
                                break;
                            }
                            audio_done = true;
                        }
                    }

                    // 接收识别结果
                    msg = read.next() => {
                        match msg {
                            Some(Ok(WsMessage::Binary(data))) => {
                                if let Ok(response) = AsrResponse::decode(Bytes::from(data)) {
                                    // 解析识别结果
                                    if !response.result_json.is_empty() {
                                        if let Ok(result_data) = serde_json::from_str::<serde_json::Value>(&response.result_json) {
                                            if let Some(results) = result_data.get("results").and_then(|r| r.as_array()) {
                                                for result in results {
                                                    if let Some(text) = result.get("text").and_then(|t| t.as_str()) {
                                                        let is_final = !result.get("is_interim")
                                                            .and_then(|f| f.as_bool())
                                                            .unwrap_or(true);

                                                        if result_tx
                                                            .send(TranscriptionResult::text(text.to_string(), is_final))
                                                            .await
                                                            .is_err()
                                                        {
                                                            return;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if response.message_type == "SessionFinished"
                                    {
                                        println!("Session ended: {}", response.message_type);
                                        break;
                                    }

                                    if response.message_type == "TaskFailed"
                                        || response.message_type == "SessionFailed"
                                    {
                                        failed = true;
                                        let failure = format!(
                                            "{}: code={}, status={}",
                                            response.message_type,
                                            response.status_code,
                                            response.status_message
                                        );
                                        let _ = result_tx.send(TranscriptionResult::failed(failure.clone())).await;
                                        println!("Session ended: {}", failure);
                                        break;
                                    }
                                }
                            }
                            Some(Ok(WsMessage::Text(text))) => {
                                failed = true;
                                let _ = result_tx
                                    .send(TranscriptionResult::failed(format!(
                                        "Server returned JSON error: {}",
                                        text
                                    )))
                                    .await;
                                eprintln!("Server error (JSON): {}", text);
                                break;
                            }
                            Some(Ok(_)) => {}
                            _ => {
                                failed = true;
                                let _ = result_tx
                                    .send(TranscriptionResult::failed("WebSocket closed unexpectedly"))
                                    .await;
                                println!("WebSocket closed");
                                break;
                            }
                        }
                    }
                }
            }

            if !failed {
                let _ = result_tx.send(TranscriptionResult::completed()).await;
            }

            println!("Transcription task ended");
        });

        Ok(result_rx)
    }

    async fn send_start_task(
        write: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            WsMessage,
        >,
        request_id: &str,
        token: &str,
    ) -> Result<(), String> {
        let request = AsrRequest {
            token: token.to_string(),
            service_name: "ASR".to_string(),
            method_name: "StartTask".to_string(),
            request_id: request_id.to_string(),
            ..Default::default()
        };

        let mut buf = Vec::new();
        request.encode(&mut buf).map_err(|e| e.to_string())?;
        write
            .send(WsMessage::Binary(buf))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn send_start_session(
        write: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            WsMessage,
        >,
        request_id: &str,
        token: &str,
        device_id: &str,
    ) -> Result<(), String> {
        // 会话配置需与 Python config.py 的 SessionConfig 完全一致
        let config = serde_json::json!({
            "audio_info": {
                "channel": 1,
                "format": "speech_opus",
                "sample_rate": 16000
            },
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

        let request = AsrRequest {
            token: token.to_string(),
            service_name: "ASR".to_string(),
            method_name: "StartSession".to_string(),
            request_id: request_id.to_string(),
            payload: config.to_string(),
            ..Default::default()
        };

        let mut buf = Vec::new();
        request.encode(&mut buf).map_err(|e| e.to_string())?;
        write
            .send(WsMessage::Binary(buf))
            .await
            .map_err(|e| e.to_string())?;
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
    ) -> Result<(), String> {
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
        request.encode(&mut buf).map_err(|e| e.to_string())?;
        write
            .send(WsMessage::Binary(buf))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn send_finish_session(
        write: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            WsMessage,
        >,
        request_id: &str,
        token: &str,
    ) -> Result<(), String> {
        let request = AsrRequest {
            token: token.to_string(),
            service_name: "ASR".to_string(),
            method_name: "FinishSession".to_string(),
            request_id: request_id.to_string(),
            ..Default::default()
        };

        let mut buf = Vec::new();
        request.encode(&mut buf).map_err(|e| e.to_string())?;
        write
            .send(WsMessage::Binary(buf))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
