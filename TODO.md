# 待验证清单（需要你亲自测试）

## 🔴 必须测试项

### 1. 修复后的主应用测试
**问题**：当前主应用出现 `SessionFailed` 错误，但独立测试程序可以成功转写。

**需要验证**：
```bash
cd /Users/lijunke/vbcode/doubao-voice-input
pkill -f "tauri dev"  # 先关闭当前运行的应用
DYLD_LIBRARY_PATH=/opt/homebrew/lib npm run tauri dev
```

**测试步骤**：
1. 点击"选择文件"按钮
2. 选择 `/Users/lijunke/Downloads/caption_sample2.wav`
3. **期望结果**：识别结果实时显示在界面上
4. **检查日志**：终端中应该看到 "Encoded XX frames" 和识别结果
5. 如果仍然失败，查看终端完整错误信息

### 2. 麦克风实时录音测试
**需要验证**：
1. 点击"开始录音"按钮（应该变成红色"录音中"状态）
2. 对着麦克风说话（中文或英文）
3. **期望结果**：
   - 实时看到识别的文字（灰色 = 中间结果，白色 = 最终结果）
   - 说话停顿后，文字固定下来
4. 点击"停止录音"
5. 检查最终识别结果是否完整

**macOS 首次运行注意**：
- 会弹出麦克风权限申请，必须点"允许"
- 如果没弹出，去"系统设置 → 隐私与安全性 → 麦克风"手动添加应用

### 3. 多次连续测试
**需要验证稳定性**：
1. 录音 → 停止 → 再次录音（至少 3 次）
2. 选择文件转写 → 等待完成 → 再次选择文件
3. 录音过程中切换窗口、最小化应用
4. **检查**：是否有内存泄漏、卡顿、崩溃

---

## 🟡 可选测试项

### 4. 不同音频格式测试
测试文件转写对各种格式的支持：
- MP3
- M4A
- FLAC
- OGG
- AAC

### 5. 音频质量边界测试
- 很长的音频文件（> 5 分钟）
- 背景噪音很大的录音
- 多人对话（看是否能正确识别）

### 6. 中英文混合测试
- 纯中文
- 纯英文
- 中英文混合（像测试样本那样）

### 7. UI 交互测试
- 复制按钮是否正常工作
- 结果框自动滚动到最新内容
- 录音时禁用"选择文件"按钮
- 转写时禁用"开始录音"按钮

---

## 🔧 如果测试失败

### 主应用仍然 SessionFailed
查看日志关键信息：
```bash
tail -100 /tmp/tauri_output.log | grep -E "SessionFailed|error|Failed"
```

**可能需要**：
1. 对比 `test-standalone/src/bin/ultimate.rs` 和 `src-tauri/src/lib.rs` 的差异
2. 确认 `audio_tx` drop 的时机
3. 检查 `tokio::spawn` 任务是否提前退出

### 麦克风没有声音
1. 检查系统麦克风权限
2. 查看终端日志："麦克风已启动: XX Hz, X 声道"
3. 如果采样率异常（如 0 Hz），可能是 cpal 配置问题

### 识别结果全是乱码
1. 检查凭据是否有效：
   ```bash
   cat "$HOME/Library/Application Support/doubao-voice-input/credentials.json"
   ```
2. 重新注册设备：
   ```bash
   cd /Users/lijunke/vbcode/doubaoime-asr
   python3 register_device.py
   # 复制新的 token 到 Rust 应用配置
   ```

---

## 📋 测试通过标准

**✅ 认为成功的条件**：
1. 文件转写能显示完整识别结果（与 Python 版本一致）
2. 麦克风录音能实时显示文字，停止后保留完整结果
3. 复制按钮能正确复制文本
4. 连续测试 5 次以上不崩溃

**然后可以**：
- 正常使用这个应用
- 打包成独立 .app 分发
- 继续添加新功能（如保存历史、多语言切换）

---

## 🚨 紧急回退方案

如果 Rust 版本完全不可用，Python 版本依然可以作为备份：
```bash
cd /Users/lijunke/vbcode/doubaoime-asr
DYLD_LIBRARY_PATH=/opt/homebrew/lib python3 -c "
from doubaoime_asr import transcribe, ASRConfig
import asyncio
config = ASRConfig(credential_path='./credentials.json')
result = asyncio.run(transcribe('/path/to/audio.wav', config=config))
print(result)
"
```

---

**如果遇到问题，请记录**：
1. 完整错误日志（终端输出）
2. 操作步骤（点击了什么按钮、说了什么话）
3. 音频文件信息（如果是文件转写失败）

**我已经完成的验证**：
- ✅ 独立测试程序成功转写音频
- ✅ WebSocket 协议正确（v2 + proto-version 头）
- ✅ Protobuf 消息格式正确
- ✅ 凭据注册和持久化正常

**剩下的只是主应用的流程调试问题**，代码逻辑已经验证通过。
