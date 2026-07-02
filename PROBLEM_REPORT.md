# 豆包语音输入 - 问题报告

## 项目背景

这是一个使用 Tauri 开发的桌面应用，功能是：
1. **麦克风实时语音转写**（已工作正常）
2. **音频文件转写**（目前有问题）

技术栈：
- 前端：HTML + JavaScript + Tauri API
- 后端：Rust + Tauri
- 语音识别：豆包 ASR WebSocket API (protobuf 协议)
- 音频处理：symphonia (解码) + opus (编码)

## 核心问题

**独立测试程序能成功转写音频文件，但集成到 Tauri 应用后却失败**

### 成功的测试代码

位置：`test-standalone/src/bin/ultimate.rs`

流程：
1. 注册设备、获取 token
2. 读取音频文件 → 编码为 Opus 帧
3. 连接 WebSocket (带 `proto-version: v2` header)
4. 发送 StartTask → 收到 TaskStarted
5. 发送 StartSession → 收到 SessionStarted
6. **同步循环发送所有音频帧**（550帧，每帧标记 FIRST/MIDDLE/LAST）
7. 发送 FinishSession
8. **循环接收识别结果**，直到 SessionFinished
9. ✅ **成功输出识别文本**

运行结果：
```
========== 最终识别结果 ==========
阿斯塔诺哥哥，你有钱吗？what do you like to see you on the moon？
==================================
```

### 失败的应用代码

位置：`src-tauri/src/lib.rs` 和 `src-tauri/src/modules/asr_client.rs`

流程与测试代码**完全一致**：
1. `transcribe_file` 命令被前端调用
2. 在 `tokio::spawn` 中异步执行 `process_audio_file`
3. 加载凭据、读取音频、编码为 Opus 帧（完全相同的代码）
4. 调用 `AsrClient::transcribe_file(encoded_frames)`
5. 在 `tokio::spawn` 中：
   - 连接 WebSocket（相同的 URL 和 headers）
   - StartTask → SessionStarted（成功）
   - StartSession → SessionStarted（成功）
   - **同步循环发送所有音频帧**（与测试代码一致）
   - 发送 FinishSession
   - 循环接收结果
6. ❌ **收到大量 `SessionFailed` 消息，没有识别结果**

实际日志输出：
```
========== transcribe_file CALLED ==========
File path: /Users/lijunke/Downloads/caption_sample2.wav
========== SPAWN STARTED ==========
========== process_audio_file STARTED ==========
Loading credentials...
Credentials loaded successfully
Reading audio file...
Audio file read: 352000 bytes, sample rate: 16000
Encoded 550 frames, starting transcription stream...
Connecting to WebSocket: wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?aid=401734&device_id=1923476667833563
WebSocket connected successfully!
Sending StartTask...
Received: TaskStarted
Sending StartSession...
Received: SessionStarted
Session initialized, sending 550 frames...
All frames sent, sending FinishSession...
Received binary message, size: 232
Decoded response: message_type=SessionFailed, result_json.len=0
Received binary message, size: 187
Decoded response: message_type=SessionFailed, result_json.len=0
(重复多次 SessionFailed...)
```

## 代码对比

### 测试代码的关键部分

```rust
// ultimate.rs:235-245
let total = frames.len();
for (i, frame) in frames.iter().enumerate() {
    let frame_state = if i == 0 { 1 } else if i == total - 1 { 9 } else { 3 };
    let timestamp_ms = start_time + (i as i64) * 20;
    send_audio_frame(&mut write, &request_id, frame, frame_state, timestamp_ms).await?;
}
println!("   ✓ 音频发送完毕");

// FinishSession
send_finish_session(&mut write, &request_id, &token).await?;

// 接收结果
loop {
    let msg = tokio::time::timeout(Duration::from_secs(15), read.next()).await?...;
    // 处理识别结果
}
```

### 应用代码的关键部分

```rust
// asr_client.rs:105-180 (当前版本)
let total = audio_frames.len();

// 发送所有音频帧（与测试代码完全一致）
for (i, frame) in audio_frames.iter().enumerate() {
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
        eprintln!("Send audio frame {} failed: {}", i, e);
        return;
    }
}

println!("All frames sent, sending FinishSession...");

if let Err(e) = Self::send_finish_session(&mut write, &request_id, &token).await {
    eprintln!("FinishSession failed: {}", e);
    return;
}

// 接收识别结果
println!("Waiting for final recognition results...");
loop {
    match read.next().await {
        Some(Ok(WsMessage::Binary(data))) => {
            // 解析并发送识别结果到 channel
            // ...
            if response.message_type == "SessionFinished" { break; }
        }
        _ => break,
    }
}
```

## 关键差异点（已尝试消除但仍失败）

1. ✅ **WebSocket 连接参数**：完全一致
2. ✅ **音频帧发送逻辑**：完全一致（同步循环，无延迟）
3. ✅ **protobuf 消息构造**：使用相同的 `AsrRequest` 结构
4. ⚠️ **执行环境**：
   - 测试代码：在 `#[tokio::main]` 的 main 函数中直接执行
   - 应用代码：在 `tokio::spawn` 异步任务中执行（嵌套在 Tauri 的 async runtime 中）

## 已尝试的修复方案（均失败）

### 方案1：使用 channel 异步发送（最初版本）
```rust
// lib.rs 中
let (audio_tx, audio_rx) = mpsc::channel(100);
let mut result_rx = asr_client.transcribe_stream(audio_rx).await?;

tokio::spawn(async move {
    for frame in encoded_frames.iter() {
        audio_tx.send(frame.clone()).await;
        tokio::time::sleep(Duration::from_millis(20)).await; // ← 问题？
    }
});
```
结果：❌ SessionFailed

### 方案2：去除延迟，但保留 channel
```rust
tokio::spawn(async move {
    for frame in encoded_frames.iter() {
        audio_tx.send(frame.clone()).await;
        // 去除 sleep
    }
});
```
结果：❌ SessionFailed

### 方案3：改为直接传递完整帧数组（当前版本）
```rust
// lib.rs
let mut result_rx = asr_client.transcribe_file(encoded_frames).await?;

// asr_client.rs 新增方法
pub async fn transcribe_file(
    &self,
    audio_frames: Vec<Vec<u8>>,
) -> Result<mpsc::Receiver<TranscriptionResult>, Box<dyn std::error::Error>> {
    // ... 连接 WebSocket
    // 同步循环发送所有帧（与测试代码一致）
    for (i, frame) in audio_frames.iter().enumerate() {
        // ...
    }
    // FinishSession
    // 接收结果
}
```
结果：❌ 仍然 SessionFailed

## 核心疑问

1. **为什么测试代码和应用代码在逻辑完全一致的情况下，结果不同？**
2. **SessionFailed 的具体原因是什么？**（服务器没有返回详细错误信息）
3. **是否与 Tauri 的异步运行时环境有关？**
4. **是否需要在 WebSocket 连接/发送时做特殊处理？**

## 相关文件

- `src-tauri/src/lib.rs:17-102` - transcribe_file 命令和 process_audio_file 函数
- `src-tauri/src/modules/asr_client.rs:27-194` - transcribe_file 方法（在 tokio::spawn 中执行）
- `test-standalone/src/bin/ultimate.rs` - 成功的独立测试程序
- `src-tauri/src/modules/file_reader.rs` - 音频文件读取（symphonia）
- `src-tauri/src/modules/encoder.rs` - Opus 编码

## 环境信息

- 操作系统：macOS Darwin 25.5.0
- Rust：stable
- 依赖：
  - tauri = "2.2.1"
  - tokio = { version = "1", features = ["full"] }
  - tokio-tungstenite = "0.25.0"
  - opus = "0.3.0"
  - symphonia = "0.5.4"
  - prost = "0.13.4"

## 期望协助

需要找出为什么相同的代码逻辑在不同执行环境下会导致 SessionFailed，以及如何修复。
