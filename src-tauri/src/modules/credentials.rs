use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub device_id: String,
    pub install_id: String,
    pub cdid: String,
    pub openudid: String,
    pub clientudid: String,
    pub token: String,
}

impl Credentials {
    pub fn create_fresh() -> Result<Self, Box<dyn std::error::Error>> {
        let creds = Self::register_device()?;
        creds.save()?;
        Ok(creds)
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::credentials_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    fn credentials_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("doubao-voice-input");
        path.push("credentials.json");
        path
    }

    fn register_device() -> Result<Self, Box<dyn std::error::Error>> {
        use reqwest::blocking::Client;
        use uuid::Uuid;

        let cdid = Uuid::new_v4().to_string();
        let openudid = format!("{:016x}", rand::random::<u64>());
        let clientudid = Uuid::new_v4().to_string();

        let client = Client::new();

        // 设备注册
        let register_url = "https://log.snssdk.com/service/2/device_register/";

        let body = serde_json::json!({
            "magic_tag": "ss_app_log",
            "header": {
                "device_id": 0,
                "install_id": 0,
                "aid": 401734,
                "app_name": "oime",
                "version_code": 100102018,
                "version_name": "1.1.2",
                "manifest_version_code": 100102018,
                "update_version_code": 100102018,
                "channel": "official",
                "package": "com.bytedance.android.doubaoime",
                "device_platform": "android",
                "os": "android",
                "os_api": "34",
                "os_version": "16",
                "device_type": "Pixel 7 Pro",
                "device_brand": "google",
                "device_model": "Pixel 7 Pro",
                "resolution": "1080*2400",
                "dpi": "420",
                "language": "zh",
                "timezone": 8,
                "access": "wifi",
                "rom": "UP1A.231005.007",
                "rom_version": "UP1A.231005.007",
                "openudid": &openudid,
                "clientudid": &clientudid,
                "cdid": &cdid,
                "region": "CN",
                "tz_name": "Asia/Shanghai",
                "tz_offset": 28800,
                "sim_region": "cn",
                "carrier_region": "cn",
                "cpu_abi": "arm64-v8a",
                "build_serial": "unknown",
                "not_request_sender": 0,
                "sig_hash": "",
                "google_aid": "",
                "mc": "",
                "serial_number": "",
            },
            "_gen_time": chrono::Utc::now().timestamp_millis(),
        });

        let response = client
            .post(register_url)
            .header("User-Agent", "com.bytedance.android.doubaoime/100102018")
            .query(&[
                ("device_platform", "android"),
                ("os", "android"),
                ("ssmix", "a"),
                ("_rticket", &chrono::Utc::now().timestamp_millis().to_string()),
                ("cdid", &cdid),
                ("channel", "official"),
                ("aid", "401734"),
                ("app_name", "oime"),
                ("version_code", "100102018"),
                ("version_name", "1.1.2"),
                ("manifest_version_code", "100102018"),
                ("update_version_code", "100102018"),
                ("resolution", "1080*2400"),
                ("dpi", "420"),
                ("device_type", "Pixel 7 Pro"),
                ("device_brand", "google"),
                ("language", "zh"),
                ("os_api", "34"),
                ("os_version", "16"),
                ("ac", "wifi"),
            ])
            .json(&body)
            .send()?;

        let data: serde_json::Value = response.json()?;

        let device_id = data["device_id"].as_i64().unwrap_or(0).to_string();
        let install_id = data["install_id"].as_i64().unwrap_or(0).to_string();

        // 获取 token
        let token = Self::get_token(&device_id, &cdid)?;

        Ok(Credentials {
            device_id,
            install_id,
            cdid,
            openudid,
            clientudid,
            token,
        })
    }

    fn get_token(device_id: &str, cdid: &str) -> Result<String, Box<dyn std::error::Error>> {
        use reqwest::blocking::Client;

        let client = Client::new();
        let settings_url = "https://is.snssdk.com/service/settings/v3/";

        println!("Getting token for device_id={}, cdid={}", device_id, cdid);

        let rticket = chrono::Utc::now().timestamp_millis().to_string();

        // 计算 x-ss-stub
        let body_str = "body=null";
        let digest = md5::compute(body_str.as_bytes());
        let x_ss_stub = format!("{:X}", digest);

        let response = client
            .post(settings_url)
            .header("User-Agent", "com.bytedance.android.doubaoime/100102018 (Linux; U; Android 16; en_US; Pixel 7 Pro; Build/BP2A.250605.031.A2; Cronet/TTNetVersion:94cf429a 2025-11-17 QuicVersion:1f89f732 2025-05-08)")
            .header("x-ss-stub", x_ss_stub)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .query(&[
                ("device_platform", "android"),
                ("os", "android"),
                ("ssmix", "a"),
                ("_rticket", &rticket),
                ("cdid", cdid),
                ("channel", "official"),
                ("aid", "401734"),
                ("app_name", "oime"),
                ("version_code", "100102018"),
                ("version_name", "1.1.2"),
                ("device_id", device_id),
            ])
            .body(body_str)
            .send()?;

        let data: serde_json::Value = response.json()?;

        println!("Settings API response has asr_config: {}",
            data.get("data")
                .and_then(|d| d.get("settings"))
                .and_then(|s| s.get("asr_config"))
                .is_some());

        let token = data["data"]["settings"]["asr_config"]["app_key"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if token.is_empty() {
            return Err("Token is empty - API response may have changed".into());
        }

        println!("Got token: {}...", &token[..token.len().min(20)]);
        Ok(token)
    }
}
