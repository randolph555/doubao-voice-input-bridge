const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.dialog;

let finalText = "";
let interimText = "";
let recording = false;
let busy = false; // 文件转写进行中

let statusIndicator;
let statusText;
let statusDot;
let resultBox;
let recordBtn;
let selectFileBtn;
let copyBtn;
let recordingTransition = false;

function getDisplayedText() {
  return [finalText, interimText].filter(Boolean).join(finalText && interimText ? "\n\n" : "");
}

function setFileButtonMode(mode) {
  const icon = selectFileBtn.querySelector(".btn-icon");
  const text = selectFileBtn.querySelector(".btn-text");

  if (mode === "cancel") {
    icon.textContent = "✖️";
    text.textContent = "取消转写";
    selectFileBtn.disabled = false;
  } else {
    icon.textContent = "📁";
    text.textContent = "选择文件";
    selectFileBtn.disabled = recording;
  }
}

// 监听转写结果
async function setupListener() {
  await listen("transcription", (event) => {
    const result = event.payload;
    if (result.status) {
      if (result.status === "completed") {
        if (busy) {
          busy = false;
          setStatus(recording ? "recording" : "idle");
          recordBtn.disabled = false;
          setFileButtonMode("select");
        }
        return;
      }

      if (result.status === "failed") {
        busy = false;
        if (recording) {
          recording = false;
          recordBtn.querySelector(".btn-text").textContent = "开始录音";
          recordBtn.querySelector(".btn-icon").textContent = "🎙️";
        }
        setStatus("idle");
        recordBtn.disabled = false;
        setFileButtonMode("select");
        if (result.error) {
          console.error("转写失败:", result.error);
          alert("转写失败: " + result.error);
        }
        return;
      }

      if (result.status === "cancelled") {
        busy = false;
        recording = false;
        setStatus("idle");
        recordBtn.querySelector(".btn-text").textContent = "开始录音";
        recordBtn.querySelector(".btn-icon").textContent = "🎙️";
        recordBtn.disabled = false;
        setFileButtonMode("select");
        return;
      }
    }

    if (result.is_final) {
      finalText += (finalText ? " " : "") + result.text;
      interimText = "";
    } else {
      interimText = result.text;
    }
    updateResultDisplay();
  });
}

function updateResultDisplay() {
  resultBox.value = getDisplayedText();
  resultBox.scrollTop = resultBox.scrollHeight;
  copyBtn.disabled = !resultBox.value.trim();
}

function setStatus(mode) {
  // mode: "idle" | "recording" | "processing"
  if (mode === "recording") {
    statusDot.className = "status-dot recording";
    statusText.textContent = "录音中";
  } else if (mode === "processing") {
    statusDot.className = "status-dot recording";
    statusText.textContent = "转写中";
  } else {
    statusDot.className = "status-dot";
    statusText.textContent = "就绪";
  }
}

function applyRecordingUiState(isRecording) {
  recording = isRecording;
  setStatus(isRecording ? "recording" : "idle");
  recordBtn.querySelector(".btn-text").textContent = isRecording ? "停止录音" : "开始录音";
  recordBtn.querySelector(".btn-icon").textContent = isRecording ? "⏹️" : "🎙️";
  setFileButtonMode("select");
}

// 切换录音
async function toggleRecording() {
  if (busy || recordingTransition) return;

  if (!recording) {
    // 开始
    try {
      recordingTransition = true;
      finalText = "";
      interimText = "";
      updateResultDisplay();
      applyRecordingUiState(true);
      recordBtn.disabled = true;

      await invoke("start_recording");
    } catch (error) {
      applyRecordingUiState(false);
      console.error("开始录音失败:", error);
      alert("开始录音失败: " + error);
    } finally {
      recordingTransition = false;
      recordBtn.disabled = false;
    }
  } else {
    // 停止
    try {
      recordingTransition = true;
      recordBtn.disabled = true;
      applyRecordingUiState(false);
      await invoke("stop_recording");
    } catch (error) {
      applyRecordingUiState(true);
      console.error("停止录音失败:", error);
    } finally {
      recordingTransition = false;
      recordBtn.disabled = false;
    }
  }
}

// 选择并转写文件
async function selectAndTranscribeFile() {
  if (recording) return;

  if (busy) {
    try {
      await invoke("cancel_transcription");
      busy = false;
      setStatus(recording ? "recording" : "idle");
      recordBtn.disabled = false;
      setFileButtonMode("select");
    } catch (error) {
      console.error("取消转写失败:", error);
    }
    return;
  }

  try {
    console.log("[DEBUG] 开始选择文件...");
    const selected = await open({
      multiple: false,
      filters: [
        { name: "Audio", extensions: ["mp3", "wav", "m4a", "flac", "ogg", "aac"] },
      ],
    });

    console.log("[DEBUG] 选择的文件:", selected);
    if (!selected) {
      console.log("[DEBUG] 用户取消选择");
      return;
    }

    busy = true;
    finalText = "";
    interimText = "";
    updateResultDisplay();
    setStatus("processing");
    recordBtn.disabled = true;
    setFileButtonMode("cancel");

    console.log("[DEBUG] 调用 transcribe_file:", selected);
    const result = await invoke("transcribe_file", { filePath: selected });
    console.log("[DEBUG] transcribe_file 返回:", result);
  } catch (error) {
    console.error("转写失败:", error);
    alert("转写失败: " + error);
    setStatus("idle");
    recordBtn.disabled = false;
    busy = false;
    setFileButtonMode("select");
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  statusIndicator = document.getElementById("status-indicator");
  statusText = statusIndicator.querySelector(".status-text");
  statusDot = statusIndicator.querySelector(".status-dot");
  resultBox = document.getElementById("result-box");
  recordBtn = document.getElementById("record-btn");
  selectFileBtn = document.getElementById("select-file-btn");
  copyBtn = document.getElementById("copy-btn");
  setFileButtonMode("select");

  await setupListener();

  recordBtn.addEventListener("click", toggleRecording);
  selectFileBtn.addEventListener("click", selectAndTranscribeFile);

  copyBtn.addEventListener("click", async () => {
    if (resultBox.value.trim()) {
      try {
        await navigator.clipboard.writeText(resultBox.value);
        const el = copyBtn.querySelector(".btn-text");
        const original = el.textContent;
        el.textContent = "已复制!";
        setTimeout(() => {
          el.textContent = original;
        }, 2000);
      } catch (error) {
        console.error("复制失败:", error);
      }
    }
  });

  resultBox.addEventListener("input", () => {
    finalText = resultBox.value;
    interimText = "";
    copyBtn.disabled = !resultBox.value.trim();
  });
});
