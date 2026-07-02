# 豆包语音输入 - 开发说明

## 当前状态

✅ 项目框架已搭建完成
✅ 核心模块已实现：
  - 设备注册和凭据管理
  - ASR WebSocket 客户端
  - 音频编码（Opus）
  - Tauri 前后端通信

⚠️ 待完善：
  - 真实麦克风音频采集（当前是模拟数据）
  - 完整的 WebSocket 协议实现（需要按照豆包协议发送 StartTask, StartSession 等消息）

## 快速开始

```bash
# 开发模式运行
cd /Users/lijunke/vbcode/doubao-voice-input
npm run tauri dev
```

## 项目结构

```
doubao-voice-input/
├── src/                          # 前端代码
│   ├── index.html               # UI 界面
│   ├── main.js                  # 前端逻辑
│   └── styles.css               # 样式
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── lib.rs               # 主入口和 Tauri 命令
│   │   ├── proto.rs             # Protobuf 定义
│   │   └── modules/
│   │       ├── credentials.rs   # 设备注册和凭据管理
│   │       ├── asr_client.rs    # ASR 客户端
│   │       ├── audio.rs         # 音频录制（待完善）
│   │       └── encoder.rs       # Opus 编码
│   └── proto/
│       └── asr.proto            # Protobuf 协议定义
└── README.md
```

## 核心功能

### 1. 设备注册和凭据缓存
- 首次运行自动注册虚拟设备
- 凭据保存在 `~/.config/doubao-voice-input/credentials.json`
- 下次启动自动复用

### 2. 实时语音识别
- WebSocket 连接豆包 ASR 服务
- 流式传输音频数据
- 实时返回识别结果（中间结果 + 最终结果）

### 3. UI 交互
- 点击按钮或按住空格键开始录音
- 实时显示识别文字
- 一键复制结果

## 下一步工作

### 紧急：完善 WebSocket 协议

参考 Python 版本的实现，需要在建立 WebSocket 连接后依次发送：

1. **StartTask** 消息
2. **StartSession** 消息（包含音频配置）
3. **TaskRequest** 消息（携带音频数据）
4. **FinishSession** 消息

当前代码只发送了音频数据，缺少完整的握手流程。

### 音频采集

当前使用模拟数据，需要集成 `cpal` 实现真实的麦克风录音：
- 采样率: 16000 Hz
- 声道: 单声道
- 格式: PCM f32

### 热键支持

可选：使用 `global-hotkey` 实现全局快捷键监听。

## 测试

```bash
# 编译
cargo build

# 运行
npm run tauri dev
```

## 注意事项

1. **macOS**: 需要先安装 Opus 库 `brew install opus`
2. **首次运行**: 需要联网注册设备
3. **凭据**: 保存在用户配置目录，删除后会重新注册
4. **协议变更**: 豆包可能随时更新协议导致失效

## 相关资源

- Python 参考实现: `/Users/lijunke/vbcode/doubaoime-asr`
- Tauri 文档: https://tauri.app
- Protobuf 定义: `src-tauri/proto/asr.proto`
