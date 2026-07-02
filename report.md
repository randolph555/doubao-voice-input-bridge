# 豆包语音输入 Rust 实现 - 完整报告

## 项目概述

成功将豆包语音识别的 Python 实现移植到 Rust + Tauri 桌面应用，支持：
- **麦克风实时转写**：点击按钮开始/停止录音，实时识别
- **音频文件转写**：支持 MP3/WAV/M4A/FLAC/OGG/AAC 格式

## 核心问题解决

### 关键突破：WebSocket 协议版本问题

**问题现象**：
- Python 版本：正常工作 ✅
- Rust 初始版本：收到 `EventInvalidAfterRestart` 错误（错误码 40100003）❌

**根本原因**：
服务器支持两种 WebSocket 协议：
1. **v1 协议**（JSON）：URL 参数用 `token=xxx`，服务器返回 JSON 错误
2. **v2 协议**（Protobuf）：URL 参数用 `aid=xxx&device_id=xxx`，HTTP 头带 `proto-version: v2`

**解决方案**：
```rust
// ✅ 正确方式（v2 协议）
let url = format!(
    "wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?aid=401734&device_id={}",
    device_id
);
let mut request = url.into_client_request()?;
request.headers_mut().insert("proto-version", "v2".parse()?);
request.headers_mut().insert("x-custom-keepalive", "true".parse()?);
```

### 会话配置完全匹配 Python

```rust
let config = serde_json::json!({
    "audio_info": {
        "channel": 1,
        "format": "speech_opus",  // ← 关键：不是 "opus"
        "sample_rate": 16000
    },
    "enable_punctuation": true,
    "enable_speech_rejection": false,
    "extra": {
        "app_name": "com.android.chrome",
        "cell_compress_rate": 8,
        "did": device_id,  // ← 关键：必须传 device_id
        "enable_asr_threepass": true,
        "enable_asr_twopass": true,
        "input_mode": "tool"
    }
});
```

## 技术架构

### 后端 (Rust + Tauri)

```
src-tauri/
├── src/
│   ├── lib.rs                    # Tauri 命令：transcribe_file, start/stop_recording
│   ├── proto/
│   │   ├── asr.proto             # Protobuf 协议定义
│   │   └── mod.rs
│   └── modules/
│       ├── credentials.rs        # 设备注册 + Token 获取
│       ├── asr_client.rs         # WebSocket ASR 客户端（核心）
│       ├── audio.rs              # 麦克风采集（cpal）
│       ├── encoder.rs            # Opus 编码
│       └── file_reader.rs        # 音频文件读取（symphonia）
```

**关键流程**：

1. **设备注册** (`credentials.rs`):
   - 生成唯一设备 ID（cdid/openudid/clientudid）
   - POST `https://log.snssdk.com/service/2/device_register/`
   - POST `https://is.snssdk.com/service/settings/v3/` 获取 Token
   - 缓存到 `~/.config/doubao-voice-input/credentials.json`

2. **WebSocket ASR 流** (`asr_client.rs`):
   ```
   连接 → StartTask → StartSession → 发送音频帧 → 接收识别结果 → FinishSession
   ```

3. **麦克风录音** (`audio.rs`):
   - cpal 采集原始 f32 样本（任意采样率/声道）
   - 实时降混 → 重采样到 16kHz 单声道
   - 每 20ms（320 样本）编码为 Opus 帧
   - 送入 ASR WebSocket 流

4. **音频文件转写** (`lib.rs::process_audio_file`):
   - symphonia 解码音频文件
   - 统一转换为 16kHz 单声道 PCM
   - 编码为 Opus 帧序列
   - 按 20ms 节奏发送（模拟真实录音）

### 前端 (HTML + JavaScript)

```
src/
├── index.html      # UI：录音按钮 + 文件选择 + 结果显示
├── main.js         # Tauri 事件监听、命令调用
└── styles.css      # 深色主题样式
```

**UI 功能**：
- **开始录音** → 调用 `start_recording`，实时显示识别结果
- **停止录音** → 调用 `stop_recording`
- **选择文件** → 调用 `transcribe_file`，显示转写进度
- **复制结果** → 一键复制最终文本到剪贴板

## 测试验证

### 独立测试程序（test-standalone/）

创建了多个测试 binary 验证协议：
- `ultimate.rs`：完整流程测试（注册设备 → 转写音频 → 输出结果）
- **测试结果**：✅ 成功转写 `caption_sample2.wav`

```
========== 最终识别结果 ==========
阿斯塔诺哥哥，你有钱吗？what do you like to see you on the moon？
==================================
```

### 主应用测试

**文件转写**：
- ✅ 成功建立 WebSocket 连接
- ✅ StartTask / StartSession 通过
- ⚠️ 发现 `SessionFailed` 错误（音频流未完全发送完毕）

**麦克风录音**：
- ✅ 麦克风采集正常（48kHz → 16kHz 重采样）
- ✅ WebSocket 连接正常
- ⚠️ 同样出现 `SessionFailed`（需进一步调试）

## 当前状态

### ✅ 已完成

1. **设备注册**：完全复制 Python 逻辑，凭据持久化
2. **WebSocket 协议**：正确使用 v2 协议（proto-version 头 + aid/device_id 参数）
3. **Protobuf 消息**：完整定义 `AsrRequest` / `AsrResponse`
4. **音频处理链**：
   - 文件解码（symphonia）
   - 麦克风采集（cpal）
   - 降混 / 重采样 / Opus 编码
5. **Tauri 集成**：前后端事件流（transcription 事件）
6. **UI 界面**：录音按钮 + 文件选择 + 实时结果显示

### ⚠️ 待修复问题

**主应用中的 `SessionFailed`**：
- 独立测试程序 (`ultimate.rs`) 可以成功转写
- 主应用 (`lib.rs`) 出现早期 SessionFailed
- **可能原因**：
  - 音频帧发送时序问题（发送太快 / 太慢）
  - 音频流未正确关闭（audio_tx drop 时机）
  - tokio 任务生命周期问题

**需要调试**：
1. 对比独立测试和主应用的日志差异
2. 确认音频帧是否全部发送完毕
3. 检查 WebSocket 流的完整生命周期

## 依赖清单

### Cargo 依赖 (src-tauri/Cargo.toml)

```toml
[dependencies]
tauri = { version = "2", features = ["devtools"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
tokio-tungstenite = { version = "0.24", features = ["native-tls"] }
futures-util = "0.3"
prost = "0.13"
bytes = "1"
uuid = { version = "1", features = ["v4"] }
reqwest = { version = "0.12", features = ["json", "blocking"] }
chrono = "0.4"
md5 = "0.7"
rand = "0.8"
cpal = "0.15"
opus = "0.3"
symphonia = { version = "0.5", features = ["all"] }

[build-dependencies]
prost-build = "0.13"
tauri-build = { version = "2", features = [] }
```

### 系统依赖

**macOS**：
```bash
brew install opus libopus
export DYLD_LIBRARY_PATH=/opt/homebrew/lib
```

**权限** (Info.plist):
```xml
<key>NSMicrophoneUsageDescription</key>
<string>需要访问麦克风以进行实时语音识别。</string>
```

## 项目目录结构

```
doubao-voice-input/
├── src/                          # 前端代码
│   ├── index.html
│   ├── main.js
│   └── styles.css
├── src-tauri/                    # 后端代码
│   ├── src/
│   │   ├── lib.rs
│   │   ├── main.rs
│   │   ├── proto/
│   │   └── modules/
│   ├── proto/
│   │   └── asr.proto
│   ├── build.rs
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── Info.plist               # macOS 麦克风权限
└── test-standalone/             # 独立测试程序
    ├── src/
    │   ├── main.rs
    │   └── bin/
    │       ├── ultimate.rs      # 完整测试（成功）
    │       ├── simple.rs
    │       └── ...
    ├── proto/asr.proto
    ├── build.rs
    └── Cargo.toml
```

## Python 原项目

位置：`/Users/lijunke/vbcode/doubaoime-asr/`

**核心文件**：
- `doubaoime_asr/asr.py`：WebSocket 客户端实现
- `doubaoime_asr/config.py`：配置管理
- `doubaoime_asr/device.py`：设备注册
- `doubaoime_asr/asr_pb2.py`：Protobuf 定义（生成）
- `register_device.py`：手动注册工具

## 运行方式

### 启动开发服务器

```bash
cd /Users/lijunke/vbcode/doubao-voice-input
DYLD_LIBRARY_PATH=/opt/homebrew/lib npm run tauri dev
```

### 独立测试

```bash
cd /Users/lijunke/vbcode/doubao-voice-input/test-standalone
DYLD_LIBRARY_PATH=/opt/homebrew/lib cargo run --release --bin ultimate
```

### Python 原版对比测试

```bash
cd /Users/lijunke/vbcode/doubaoime-asr
python3 register_device.py                    # 生成新凭据
DYLD_LIBRARY_PATH=/opt/homebrew/lib python3 -c "
from doubaoime_asr import transcribe, ASRConfig
import asyncio
config = ASRConfig(credential_path='./credentials.json')
result = asyncio.run(transcribe('/Users/lijunke/Downloads/caption_sample2.wav', config=config))
print(result)
"
```

## 下一步优化建议

1. **修复 SessionFailed 问题**：
   - 统一 `test-standalone/ultimate.rs` 和 `lib.rs` 的实现
   - 确保音频流完整关闭

2. **错误处理增强**：
   - 解析 `SessionFailed` 的 `status_message` 字段
   - 前端显示详细错误信息

3. **性能优化**：
   - 麦克风采集使用固定大小缓冲区（减少内存分配）
   - 编码器复用（避免每次重新初始化）

4. **功能扩展**：
   - 支持多语言切换（zh-CN / en-US）
   - VAD（语音活动检测）
   - 保存转写历史

5. **打包发布**：
   - macOS .app 签名
   - 自动更新机制
   - Windows 支持

## 技术难点总结

1. **WebSocket 协议版本陷阱**：
   - 服务器未明确文档说明 v1/v2 区别
   - 错误码 `EventInvalidAfterRestart` 误导性强
   - 需要通过对比 Python/Rust 的实际网络包才能发现

2. **音频处理管道**：
   - 麦克风任意采样率 → 统一 16kHz
   - cpal 的 `Stream` 非 `Send`，必须在独立线程持有
   - Opus 编码要求固定帧大小（320 样本 = 20ms @ 16kHz）

3. **异步流同步**：
   - 音频发送（tokio async）和结果接收（tokio async）需并行
   - audio_tx drop 时机决定流是否正确结束
   - 需要 `tokio::select!` 实现双向通信

## 总结

项目核心功能已基本实现，独立测试程序验证了协议的正确性。主应用存在一个 `SessionFailed` 的流程问题，需要进一步调试音频发送/接收的同步逻辑。修复后即可投入使用。

---

**项目时间**：2026-07-01  
**测试环境**：macOS (Darwin 25.5.0), Opus 4.8, Rust 1.84+
