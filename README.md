# 豆包语音输入

基于 Tauri + Rust 的桌面语音输入工具，使用豆包 ASR 服务实现实时语音转文字。

## 功能特性

- 🎤 **实时语音识别** - 流式识别，低延迟
- ⌨️ **快捷键支持** - 按住空格键开始录音
- 📋 **一键复制** - 快速复制识别结果
- 🎨 **简洁界面** - 现代化暗色主题
- 🔒 **本地运行** - 跨平台桌面应用

## 使用方法

1. 启动应用
2. 点击"开始录音"按钮或按住**空格键**
3. 开始说话，实时显示识别结果
4. 松开按钮或空格键停止录音
5. 点击"复制结果"保存文本

## 开发

### 前置要求

- Rust (1.70+)
- Node.js (16+)
- Opus 音频库

```bash
# macOS
brew install opus

# Ubuntu/Debian
sudo apt install libopus0

# Windows
# 下载预编译库或使用 vcpkg
```

### 构建运行

```bash
# 安装依赖
npm install

# 开发模式
npm run tauri dev

# 构建生产版本
npm run tauri build
```

## 技术栈

- **Tauri 2** - 桌面应用框架
- **Rust** - 后端逻辑
  - `cpal` - 音频采集
  - `opus` - 音频编码
  - `tokio-tungstenite` - WebSocket 客户端
- **Vanilla JS** - 前端界面

## 免责声明

本项目基于豆包输入法的非官方 API 实现，仅供学习研究使用。

- 不保证长期可用性
- 请勿用于商业用途
- 服务端协议可能随时变更

## 许可证

MIT License

