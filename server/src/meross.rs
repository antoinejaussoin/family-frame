//! Meross cloud + hub client for house temperatures.
//!
//! MS100 thermometer / hygrometers report through a Meross hub. MTS200-class
//! wall thermostats are Wi-Fi devices on the same account. Hub TRVs
//! (MTS100 / MTS150) use `Appliance.Hub.Mts100.All`. Local HTTP is signed with
//! the account key from `/v1/Auth/signIn`; live readings otherwise come over
//! MQTT (or LAN HTTP when `hub_hosts` are set).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use md5::{Digest, Md5};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS, Transport};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::timeout;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::MerossConfig;
use crate::model::RoomClimate;

const CLOUD_SECRET: &str = "23x17ahWarFH6w29";
const MQTT_PORT: u16 = 443;
const MQTT_TIMEOUT: Duration = Duration::from_secs(15);
const ROOMS_TTL: Duration = Duration::from_secs(90);

static CREDS: Mutex<Option<CloudCreds>> = Mutex::new(None);
static ROOMS: Mutex<Option<(Instant, Vec<RoomClimate>)>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudCreds {
    email: String,
    token: String,
    key: String,
    user_id: String,
    domain: String,
    mqtt_domain: String,
}

#[derive(Debug, Deserialize)]
struct CloudEnvelope {
    #[serde(rename = "apiStatus")]
    api_status: i64,
    info: Option<String>,
    data: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct DeviceInfo {
    uuid: String,
    #[serde(default, rename = "devName")]
    dev_name: String,
    #[serde(default, rename = "deviceType")]
    device_type: String,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default, rename = "reservedDomain")]
    reserved_domain: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubdeviceInfo {
    #[serde(default, alias = "subDeviceId", alias = "sub_device_id")]
    id: String,
    #[serde(default, alias = "subDeviceName", alias = "sub_device_name")]
    name: String,
    #[serde(default, alias = "subDeviceType", alias = "sub_device_type")]
    device_type: String,
}

pub async fn load_rooms(cfg: &MerossConfig, creds_path: &Path) -> Result<Vec<RoomClimate>> {
    if let Some((at, rooms)) = ROOMS.lock().ok().and_then(|g| g.clone()) {
        if at.elapsed() < ROOMS_TTL {
            return Ok(rooms);
        }
    }

    let creds = login(cfg, creds_path).await?;
    let rooms = match fetch_rooms(cfg, &creds).await {
        Ok(rooms) => rooms,
        Err(err) if err.to_string().contains("token expired") => {
            invalidate_cache();
            let _ = std::fs::remove_file(creds_path);
            let creds = cloud_login(cfg, &cfg.api_base_url).await?;
            if let Err(err) = save_creds_file(creds_path, &creds) {
                warn!(%err, "could not cache Meross credentials");
            }
            store_creds(creds.clone());
            fetch_rooms(cfg, &creds).await?
        }
        Err(err) => return Err(err),
    };

    if let Ok(mut guard) = ROOMS.lock() {
        *guard = Some((Instant::now(), rooms.clone()));
    }
    Ok(rooms)
}

pub fn invalidate_cache() {
    invalidate_rooms();
    if let Ok(mut guard) = CREDS.lock() {
        *guard = None;
    }
}

/// Drop cached thermometer readings so the next dashboard load talks to the hub.
pub fn invalidate_rooms() {
    if let Ok(mut guard) = ROOMS.lock() {
        *guard = None;
    }
}

async fn login(cfg: &MerossConfig, creds_path: &Path) -> Result<CloudCreds> {
    if let Some(cached) = cached_creds(&cfg.email) {
        return Ok(cached);
    }
    if let Some(disk) = load_creds_file(creds_path) {
        if disk.email.eq_ignore_ascii_case(cfg.email.trim()) {
            store_creds(disk.clone());
            return Ok(disk);
        }
    }

    let creds = cloud_login(cfg, &cfg.api_base_url).await?;
    if let Err(err) = save_creds_file(creds_path, &creds) {
        warn!(%err, "could not cache Meross credentials");
    }
    store_creds(creds.clone());
    Ok(creds)
}

async fn cloud_login(cfg: &MerossConfig, api_base: &str) -> Result<CloudCreds> {
    let password_md5 = md5_hex(cfg.password.trim().as_bytes());
    let mut params = json!({
        "email": cfg.email.trim(),
        "password": password_md5,
        "accountCountryCode": cfg.country_code.trim().to_uppercase(),
        "encryption": 1,
        "agree": 1,
        "mobileInfo": {
            "deviceModel": "eink-frame",
            "mobileOsVersion": "0.1.0",
            "mobileOs": std::env::consts::OS,
            "uuid": format!("eink-frame-{}", Uuid::new_v4().simple()),
            "carrier": ""
        }
    });
    if !cfg.mfa_code.trim().is_empty() {
        params["mfaCode"] = json!(cfg.mfa_code.trim());
    }

    let url = format!("{}/v1/Auth/signIn", api_base.trim_end_matches('/'));
    match cloud_post(&url, params, None).await {
        Ok(data) => parse_creds(cfg.email.trim(), data),
        Err(err) if err.to_string().starts_with("meross-region:") => {
            let domain = err
                .to_string()
                .trim_start_matches("meross-region:")
                .to_string();
            info!(%domain, "Meross login redirected to another region");
            Box::pin(cloud_login(cfg, &domain)).await
        }
        Err(err) => Err(err),
    }
}

fn parse_creds(email: &str, data: Value) -> Result<CloudCreds> {
    let user_id = json_string(&data, &["userid", "userId"])
        .ok_or_else(|| anyhow!("Meross login missing userid"))?;
    Ok(CloudCreds {
        email: email.to_string(),
        token: json_string(&data, &["token"])
            .ok_or_else(|| anyhow!("Meross login missing token"))?,
        key: json_string(&data, &["key"]).ok_or_else(|| anyhow!("Meross login missing key"))?,
        user_id,
        domain: json_string(&data, &["domain"])
            .unwrap_or_else(|| "https://iotx-eu.meross.com".into()),
        mqtt_domain: json_string(&data, &["mqttDomain", "mqtt_domain"])
            .unwrap_or_else(|| "mqtt.meross.com".into()),
    })
}

async fn fetch_rooms(cfg: &MerossConfig, creds: &CloudCreds) -> Result<Vec<RoomClimate>> {
    let devices = list_devices(creds).await?;
    for d in &devices {
        info!(
            name = %d.dev_name,
            device_type = %d.device_type,
            "Meross device"
        );
    }
    let hubs: Vec<&DeviceInfo> = devices.iter().filter(|d| is_hub(&d.device_type)).collect();
    if hubs.is_empty() {
        bail!("no Meross hub on this account (looked for msh*)");
    }

    let mut names: HashMap<String, String> = HashMap::new();
    let mut has_hub_thermostat = false;
    for hub in &hubs {
        match list_subdevices(creds, &hub.uuid).await {
            Ok(subs) => {
                for sub in subs {
                    if sub.id.is_empty() {
                        continue;
                    }
                    if !is_climate_subdevice(&sub.device_type) {
                        info!(
                            hub = %hub.dev_name,
                            id = %sub.id,
                            name = %sub.name,
                            device_type = %sub.device_type,
                            "skipping Meross subdevice"
                        );
                        continue;
                    }
                    let raw = if sub.name.is_empty() {
                        sub.id.clone()
                    } else {
                        sub.name.clone()
                    };
                    let thermostat = is_hub_thermostat(&sub.device_type);
                    if thermostat {
                        has_hub_thermostat = true;
                    }
                    let label = if thermostat {
                        thermostat_display_name(&raw, &sub.id, cfg)
                    } else {
                        cfg.labels
                            .get(&sub.name)
                            .or_else(|| cfg.labels.get(&sub.id))
                            .cloned()
                            .unwrap_or(raw)
                    };
                    info!(
                        hub = %hub.dev_name,
                        id = %sub.id,
                        name = %sub.name,
                        device_type = %sub.device_type,
                        label = %label,
                        "Meross climate subdevice"
                    );
                    names.insert(sub.id, label);
                }
            }
            Err(err) => warn!(hub = %hub.dev_name, %err, "hub subdevice list failed"),
        }
    }

    let wifi_devices: Vec<DeviceInfo> = devices
        .iter()
        .filter(|d| is_wifi_thermostat(&d.device_type))
        .cloned()
        .collect();

    let mut rooms = Vec::new();
    for hub in hubs {
        match hub_get(
            cfg,
            creds,
            hub,
            "Appliance.Hub.Sensor.All",
            json!({ "all": [] }),
        )
        .await
        {
            Ok(payload) => rooms.extend(rooms_from_sensor_all(&payload, &names, cfg)),
            Err(err) => warn!(hub = %hub.dev_name, %err, "hub sensor poll failed"),
        }
        if !has_hub_thermostat {
            continue;
        }
        match hub_get(
            cfg,
            creds,
            hub,
            "Appliance.Hub.Mts100.All",
            json!({ "all": [] }),
        )
        .await
        {
            Ok(payload) => {
                for room in rooms_from_mts100_all(&payload, &names, cfg) {
                    push_unique_room(&mut rooms, room);
                }
            }
            Err(err) => warn!(hub = %hub.dev_name, %err, "hub thermostat poll failed"),
        }
    }
    for room in poll_wifi_thermostats(cfg, creds, &wifi_devices).await {
        push_unique_room(&mut rooms, room);
    }

    rooms.sort_by(|a, b| a.name.cmp(&b.name));
    if !cfg.rooms.is_empty() {
        rooms.retain(|r| {
            cfg.rooms
                .iter()
                .any(|want| want.eq_ignore_ascii_case(&r.name))
        });
        rooms.sort_by_key(|r| {
            cfg.rooms
                .iter()
                .position(|want| want.eq_ignore_ascii_case(&r.name))
                .unwrap_or(usize::MAX)
        });
    }
    Ok(rooms)
}

async fn list_devices(creds: &CloudCreds) -> Result<Vec<DeviceInfo>> {
    let url = format!("{}/v1/Device/devList", creds.domain.trim_end_matches('/'));
    let data = cloud_post(&url, json!({}), Some(creds)).await?;
    let list = data
        .as_array()
        .cloned()
        .ok_or_else(|| anyhow!("Meross devList was not an array"))?;
    Ok(list
        .into_iter()
        .filter_map(|v| serde_json::from_value::<DeviceInfo>(v).ok())
        .collect())
}

async fn list_subdevices(creds: &CloudCreds, hub_id: &str) -> Result<Vec<SubdeviceInfo>> {
    let url = format!(
        "{}/v1/Hub/getSubDevices",
        creds.domain.trim_end_matches('/')
    );
    let data = cloud_post(&url, json!({ "uuid": hub_id }), Some(creds)).await?;
    let list = data.as_array().cloned().unwrap_or_default();
    Ok(list
        .into_iter()
        .filter_map(|v| serde_json::from_value::<SubdeviceInfo>(v).ok())
        .collect())
}

async fn hub_get(
    cfg: &MerossConfig,
    creds: &CloudCreds,
    hub: &DeviceInfo,
    namespace: &str,
    payload: Value,
) -> Result<Value> {
    if is_hub(&hub.device_type) {
        for host in &cfg.hub_hosts {
            match lan_command(host, creds, hub, namespace, payload.clone()).await {
                Ok(reply) => {
                    info!(hub = %hub.dev_name, host, namespace, "read Meross over LAN");
                    return Ok(reply);
                }
                Err(err) => {
                    warn!(hub = %hub.dev_name, host, namespace, %err, "LAN Meross poll failed")
                }
            }
        }
    }

    mqtt_command(creds, hub, "GET", namespace, payload).await
}

async fn poll_wifi_thermostats(
    cfg: &MerossConfig,
    creds: &CloudCreds,
    devices: &[DeviceInfo],
) -> Vec<RoomClimate> {
    let mut rooms = Vec::new();
    for dev in devices {
        let attempts = [
            ("Appliance.System.All", json!({})),
            (
                "Appliance.Control.Thermostat.Mode",
                json!({ "mode": [{ "channel": 0 }] }),
            ),
            ("Appliance.Control.Sensor.Latest", json!({ "latest": [] })),
        ];
        let mut got = None;
        for (namespace, command) in attempts {
            match hub_get(cfg, creds, dev, namespace, command).await {
                Ok(payload) => {
                    if let Some(room) = room_from_wifi_thermostat(&payload, &dev.dev_name, cfg) {
                        info!(
                            name = %dev.dev_name,
                            label = %room.name,
                            temp = %room.temperature,
                            namespace,
                            "read Meross wifi thermostat"
                        );
                        got = Some(room);
                        break;
                    }
                    warn!(
                        name = %dev.dev_name,
                        namespace,
                        payload = %truncate_json(&payload, 800),
                        "wifi thermostat reply had no currentTemp"
                    );
                }
                Err(err) => {
                    warn!(name = %dev.dev_name, namespace, %err, "wifi thermostat command failed")
                }
            }
        }
        match got {
            Some(room) => push_unique_room(&mut rooms, room),
            None => warn!(name = %dev.dev_name, "could not read wifi thermostat temperature"),
        }
    }
    rooms
}

fn truncate_json(v: &Value, max: usize) -> String {
    let s = v.to_string();
    if s.chars().count() <= max {
        s
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn push_unique_room(rooms: &mut Vec<RoomClimate>, room: RoomClimate) {
    if rooms
        .iter()
        .any(|existing| existing.name.eq_ignore_ascii_case(&room.name))
    {
        return;
    }
    rooms.push(room);
}

async fn lan_command(
    host: &str,
    creds: &CloudCreds,
    hub: &DeviceInfo,
    namespace: &str,
    payload: Value,
) -> Result<Value> {
    let app_id = "einkframe";
    let from = format!("/app/{}-{app_id}/subscribe", creds.user_id);
    let body = signed_command(&creds.key, "GET", namespace, payload, &hub.uuid, &from);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    let resp = client
        .post(format!("http://{}/config", host.trim()))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let v: Value = serde_json::from_str(resp.trim_end_matches('\0'))
        .with_context(|| format!("LAN response from {host}"))?;
    let method = v
        .pointer("/header/method")
        .and_then(Value::as_str)
        .unwrap_or("");
    if method == "ERROR" {
        bail!(
            "LAN error: {}",
            v.pointer("/payload/error/detail")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    Ok(v.get("payload").cloned().unwrap_or(Value::Null))
}

async fn mqtt_command(
    creds: &CloudCreds,
    hub: &DeviceInfo,
    method: &str,
    namespace: &str,
    payload: Value,
) -> Result<Value> {
    let app_id = md5_hex(format!("API{}", Uuid::new_v4()).as_bytes());
    let client_id = format!("app:{app_id}");
    let response_topic = format!("/app/{}-{app_id}/subscribe", creds.user_id);
    let user_topic = format!("/app/{}/subscribe", creds.user_id);
    let device_topic = format!("/appliance/{}/subscribe", hub.uuid);
    let body = signed_command(
        &creds.key,
        method,
        namespace,
        payload,
        &hub.uuid,
        &response_topic,
    );
    let message_id = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.pointer("/header/messageId")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();

    let (host, port) = mqtt_host(
        hub.domain
            .as_deref()
            .or(hub.reserved_domain.as_deref())
            .unwrap_or(&creds.mqtt_domain),
    );
    let mqtt_password = md5_hex(format!("{}{}", creds.user_id, creds.key).as_bytes());

    let mut opts = MqttOptions::new(client_id, host, port);
    opts.set_keep_alive(Duration::from_secs(20));
    opts.set_clean_session(true);
    opts.set_credentials(&creds.user_id, mqtt_password);
    opts.set_transport(Transport::tls_with_default_config());

    let (client, mut eventloop) = AsyncClient::new(opts, 16);
    client
        .subscribe(&response_topic, QoS::AtLeastOnce)
        .await
        .context("MQTT subscribe response topic")?;
    client
        .subscribe(&user_topic, QoS::AtLeastOnce)
        .await
        .context("MQTT subscribe user topic")?;

    let deadline = Instant::now() + MQTT_TIMEOUT;
    let mut published = false;
    let mut subacks = 0u8;

    while Instant::now() < deadline {
        let ev = match timeout(Duration::from_millis(500), eventloop.poll()).await {
            Ok(ev) => ev,
            Err(_) => continue,
        };
        match ev {
            Ok(Event::Incoming(Incoming::SubAck(_))) => {
                subacks += 1;
                if !published && subacks >= 1 {
                    client
                        .publish(device_topic.clone(), QoS::AtLeastOnce, false, body.clone())
                        .await
                        .context("MQTT publish")?;
                    published = true;
                }
            }
            Ok(Event::Incoming(Incoming::Publish(p))) => {
                if let Some(payload) = parse_ack(&p.payload, &message_id)? {
                    let _ = client.disconnect().await;
                    return Ok(payload);
                }
            }
            Ok(Event::Incoming(Incoming::ConnAck(ack))) => {
                if ack.code != rumqttc::ConnectReturnCode::Success {
                    bail!("MQTT connect refused: {:?}", ack.code);
                }
            }
            Ok(_) => {}
            Err(err) => {
                warn!(%err, "MQTT event");
            }
        }
    }
    bail!(
        "no GETACK from hub {} within {:?}",
        hub.dev_name,
        MQTT_TIMEOUT
    )
}

fn parse_ack(bytes: &[u8], message_id: &str) -> Result<Option<Value>> {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else {
        return Ok(None);
    };
    let mid = v
        .pointer("/header/messageId")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !message_id.is_empty() && mid != message_id {
        return Ok(None);
    }
    match v.pointer("/header/method").and_then(Value::as_str) {
        Some("GETACK") | Some("SETACK") => Ok(v.get("payload").cloned()),
        Some("ERROR") => bail!(
            "Meross MQTT error: {}",
            v.pointer("/payload/error/detail")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        _ => Ok(None),
    }
}

fn signed_command(
    key: &str,
    method: &str,
    namespace: &str,
    payload: Value,
    uuid: &str,
    from: &str,
) -> String {
    let message_id = md5_hex(Uuid::new_v4().to_string().as_bytes());
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let sign = md5_hex(format!("{message_id}{key}{timestamp}").as_bytes());
    serde_json::to_string(&json!({
        "header": {
            "from": from,
            "messageId": message_id,
            "method": method,
            "namespace": namespace,
            "payloadVersion": 1,
            "sign": sign,
            "timestamp": timestamp,
            "triggerSrc": "Android",
            "uuid": uuid
        },
        "payload": payload
    }))
    .expect("meross command json")
}

async fn cloud_post(url: &str, params: Value, creds: Option<&CloudCreds>) -> Result<Value> {
    let encoded = base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_vec(&params).context("encode meross params")?);
    let nonce: String = Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(16)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let sign = md5_hex(format!("{CLOUD_SECRET}{timestamp}{nonce}{encoded}").as_bytes());
    let auth = match creds {
        Some(c) => format!("Basic {}", c.token),
        None => "Basic".into(),
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let resp = client
        .post(url)
        .header("Authorization", auth)
        .header("vender", "meross")
        .header("AppType", "MerossIOT")
        .header("AppVersion", "0.4.10.4")
        .header("AppLanguage", "EN")
        .header("User-Agent", "eink-frame/0.1.0")
        .json(&json!({
            "params": encoded,
            "sign": sign,
            "timestamp": timestamp,
            "nonce": nonce
        }))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    let body: CloudEnvelope = resp.json().await.context("decode Meross cloud JSON")?;
    match body.api_status {
        0 => body
            .data
            .ok_or_else(|| anyhow!("Meross cloud reply had no data")),
        1030 => {
            let domain = body
                .data
                .as_ref()
                .and_then(|d| json_string(d, &["domain"]))
                .unwrap_or_default();
            bail!("meross-region:{domain}")
        }
        1033 => bail!("Meross MFA required — set meross.mfa_code in config.toml"),
        1032 => bail!("wrong Meross MFA code"),
        1002 | 1004 | 1008 => bail!("wrong Meross email or password"),
        1200 | 1019 | 1022 => bail!("Meross token expired"),
        1301 => bail!("too many Meross tokens — log out of unused Meross app sessions"),
        other => bail!(
            "Meross API {other} ({status}): {}",
            body.info.unwrap_or_default()
        ),
    }
}

pub fn rooms_from_sensor_all(
    payload: &Value,
    names: &HashMap<String, String>,
    cfg: &MerossConfig,
) -> Vec<RoomClimate> {
    let Some(all) = payload.get("all").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in all {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let (temp, humidity) = reading(entry);
        if temp.is_none() && humidity.is_none() {
            continue;
        }
        let meross_name = names.get(&id).cloned().unwrap_or_else(|| {
            if id.is_empty() {
                "Sensor".into()
            } else {
                id.clone()
            }
        });
        let name = cfg.labels.get(&meross_name).cloned().unwrap_or(meross_name);
        let online = entry
            .pointer("/online/status")
            .and_then(Value::as_i64)
            .unwrap_or(1)
            == 1;
        out.push(RoomClimate {
            name,
            temperature: temp.map(format_temp).unwrap_or_else(|| "—".into()),
            humidity: humidity.map(format_humidity).unwrap_or_else(|| "—".into()),
            online,
        });
    }
    out
}

pub fn rooms_from_mts100_all(
    payload: &Value,
    names: &HashMap<String, String>,
    cfg: &MerossConfig,
) -> Vec<RoomClimate> {
    let Some(all) = payload.get("all").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in all {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let temp = thermostat_temp(entry);
        if temp.is_none() {
            continue;
        }
        let meross_name = names.get(&id).cloned().unwrap_or_default();
        let name = thermostat_display_name(&meross_name, &id, cfg);
        let online = entry
            .pointer("/online/status")
            .and_then(Value::as_i64)
            .unwrap_or(1)
            == 1;
        out.push(RoomClimate {
            name,
            temperature: temp.map(format_temp).unwrap_or_else(|| "—".into()),
            humidity: "—".into(),
            online,
        });
    }
    out
}

pub fn room_from_wifi_thermostat(
    payload: &Value,
    meross_name: &str,
    cfg: &MerossConfig,
) -> Option<RoomClimate> {
    let (temp, online) = wifi_thermostat_reading(payload)?;
    Some(RoomClimate {
        name: thermostat_display_name(meross_name, "", cfg),
        temperature: format_temp(temp),
        humidity: "—".into(),
        online,
    })
}

fn wifi_thermostat_reading(payload: &Value) -> Option<(f64, bool)> {
    if let Some(entry) = thermostat_mode_entry(payload) {
        if let Some(temp) = tenths(
            entry
                .get("currentTemp")
                .or_else(|| entry.get("currentTemperature"))
                .or_else(|| entry.pointer("/temperature/currentTemp"))
                .or_else(|| entry.pointer("/temperature/room"))
                .or_else(|| entry.get("room")),
        ) {
            let online = entry.get("onoff").and_then(Value::as_i64).unwrap_or(1) != 0;
            return Some((temp, online));
        }
    }
    let latest = payload.get("latest").and_then(Value::as_array)?.first()?;
    let value = match latest.get("value") {
        Some(Value::Array(a)) => a.first(),
        other => other,
    };
    let temp = tenths(value.or_else(|| latest.get("temperature")))?;
    Some((temp, true))
}

fn thermostat_mode_entry(payload: &Value) -> Option<&Value> {
    for key in ["mode", "modeB"] {
        if let Some(mode) = payload.get(key) {
            if let Some(first) = mode.as_array().and_then(|a| a.first()) {
                return Some(first);
            }
            if mode.is_object() && !mode.as_object()?.is_empty() {
                return Some(mode);
            }
        }
    }
    for pointer in [
        "/all/digest/thermostat/mode",
        "/all/digest/thermostat/modeB",
        "/digest/thermostat/mode",
        "/digest/thermostat/modeB",
    ] {
        if let Some(first) = payload
            .pointer(pointer)
            .and_then(Value::as_array)
            .and_then(|a| a.first())
        {
            return Some(first);
        }
    }
    None
}

fn reading(entry: &Value) -> (Option<f64>, Option<f64>) {
    let temp = tenths(
        entry
            .pointer("/temperature/latest")
            .or_else(|| entry.get("latestTemperature")),
    );
    let humidity = tenths(
        entry
            .pointer("/humidity/latest")
            .or_else(|| entry.get("latestHumidity")),
    );
    (temp, humidity)
}

fn thermostat_temp(entry: &Value) -> Option<f64> {
    tenths(
        entry
            .pointer("/temperature/room")
            .or_else(|| entry.pointer("/temperature/current"))
            .or_else(|| entry.pointer("/temperature/latest"))
            .or_else(|| entry.get("currentTemp"))
            .or_else(|| entry.get("room")),
    )
}

fn tenths(v: Option<&Value>) -> Option<f64> {
    let n = match v? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.parse().ok()?,
        _ => return None,
    };
    Some(n / 10.0)
}

fn format_temp(c: f64) -> String {
    format!("{c:.1}°")
}

fn format_humidity(h: f64) -> String {
    format!("{:.0}%", h.round())
}

fn is_hub(device_type: &str) -> bool {
    device_type.to_ascii_lowercase().starts_with("msh")
}

fn is_climate_subdevice(device_type: &str) -> bool {
    is_temp_sensor(device_type) || is_hub_thermostat(device_type)
}

fn is_temp_sensor(device_type: &str) -> bool {
    let t = device_type.to_ascii_lowercase();
    t.is_empty() || t.starts_with("ms100") || t.contains("temp") || t.contains("hum")
}

fn is_hub_thermostat(device_type: &str) -> bool {
    let t = device_type.to_ascii_lowercase();
    t.starts_with("mts") || t.contains("thermostat")
}

fn is_wifi_thermostat(device_type: &str) -> bool {
    device_type.to_ascii_lowercase().starts_with("mts")
}

fn thermostat_display_name(raw: &str, id: &str, cfg: &MerossConfig) -> String {
    if let Some(label) = cfg
        .labels
        .get(raw)
        .or_else(|| cfg.labels.get(id).filter(|_| !id.is_empty()))
    {
        return label.clone();
    }
    let stripped = strip_thermostat_suffix(raw);
    if stripped.is_empty() || looks_like_thermostat_model(&stripped) {
        "Kitchen".into()
    } else {
        stripped
    }
}

fn strip_thermostat_suffix(raw: &str) -> String {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    for suffix in [" thermostat", " trv", " valve"] {
        if let Some(rest) = lower.strip_suffix(suffix) {
            return trimmed[..rest.len()].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn looks_like_thermostat_model(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n.is_empty() || n.starts_with("mts") || n == "thermostat"
}

fn mqtt_host(domain: &str) -> (String, u16) {
    let d = domain
        .trim()
        .trim_start_matches("mqtts://")
        .trim_start_matches("mqtt://")
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if let Some((host, port)) = d.rsplit_once(':') {
        if let Ok(p) = port.parse::<u16>() {
            return (host.to_string(), p);
        }
    }
    (d.to_string(), MQTT_PORT)
}

fn md5_hex(bytes: &[u8]) -> String {
    let mut h = Md5::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

fn json_string(v: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        match v.get(*key) {
            Some(Value::String(s)) if !s.is_empty() => return Some(s.clone()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            _ => {}
        }
    }
    None
}

fn cached_creds(email: &str) -> Option<CloudCreds> {
    let guard = CREDS.lock().ok()?;
    let creds = guard.as_ref()?;
    creds
        .email
        .eq_ignore_ascii_case(email.trim())
        .then(|| creds.clone())
}

fn store_creds(creds: CloudCreds) {
    if let Ok(mut guard) = CREDS.lock() {
        *guard = Some(creds);
    }
}

fn load_creds_file(path: &Path) -> Option<CloudCreds> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_creds_file(path: &Path, creds: &CloudCreds) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(creds)?)?;
    Ok(())
}

pub fn demo_rooms() -> Vec<RoomClimate> {
    vec![
        RoomClimate {
            name: "Kitchen".into(),
            temperature: "21.4°".into(),
            humidity: "48%".into(),
            online: true,
        },
        RoomClimate {
            name: "Studio".into(),
            temperature: "20.1°".into(),
            humidity: "51%".into(),
            online: true,
        },
        RoomClimate {
            name: "Bedroom".into(),
            temperature: "19.8°".into(),
            humidity: "53%".into(),
            online: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MerossConfig;

    #[test]
    fn parses_ms100_tenths() {
        let payload = json!({
            "all": [{
                "id": "00AABB",
                "online": {"status": 1},
                "temperature": {"latest": 214},
                "humidity": {"latest": 481}
            }]
        });
        let mut names = HashMap::new();
        names.insert("00AABB".into(), "Kitchen".into());
        let rooms = rooms_from_sensor_all(&payload, &names, &MerossConfig::default());
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].name, "Kitchen");
        assert_eq!(rooms[0].temperature, "21.4°");
        assert_eq!(rooms[0].humidity, "48%");
        assert!(rooms[0].online);
    }

    #[test]
    fn parses_mts100_room_temp() {
        let payload = json!({
            "all": [{
                "id": "010031F6",
                "online": {"status": 1},
                "temperature": {
                    "room": 205,
                    "currentSet": 200,
                    "heating": 0
                }
            }]
        });
        let mut names = HashMap::new();
        names.insert("010031F6".into(), "Kitchen".into());
        let rooms = rooms_from_mts100_all(&payload, &names, &MerossConfig::default());
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].name, "Kitchen");
        assert_eq!(rooms[0].temperature, "20.5°");
        assert_eq!(rooms[0].humidity, "—");
        assert!(rooms[0].online);
    }

    #[test]
    fn unlabeled_thermostat_defaults_to_kitchen() {
        let payload = json!({
            "all": [{
                "id": "0300A3EA",
                "online": {"status": 1},
                "temperature": {"room": 198}
            }]
        });
        let rooms = rooms_from_mts100_all(&payload, &HashMap::new(), &MerossConfig::default());
        assert_eq!(rooms[0].name, "Kitchen");
        assert_eq!(rooms[0].temperature, "19.8°");
    }

    #[test]
    fn kitchen_thermostat_wifi_name() {
        let payload = json!({
            "mode": [{
                "channel": 0,
                "onoff": 1,
                "currentTemp": 214,
                "heatTemp": 200
            }]
        });
        let room =
            room_from_wifi_thermostat(&payload, "Kitchen Thermostat", &MerossConfig::default())
                .unwrap();
        assert_eq!(room.name, "Kitchen");
        assert_eq!(room.temperature, "21.4°");
        assert_eq!(room.humidity, "—");
        assert!(room.online);
    }

    #[test]
    fn parses_mts200_system_all_digest() {
        let payload = json!({
            "all": {
                "digest": {
                    "thermostat": {
                        "mode": [{
                            "channel": 0,
                            "onoff": 1,
                            "currentTemp": 193
                        }]
                    }
                }
            }
        });
        let room =
            room_from_wifi_thermostat(&payload, "Kitchen Thermostat", &MerossConfig::default())
                .unwrap();
        assert_eq!(room.name, "Kitchen");
        assert_eq!(room.temperature, "19.3°");
    }

    #[test]
    fn thermostat_label_overrides_kitchen() {
        let payload = json!({
            "all": [{
                "id": "010031F6",
                "temperature": {"room": 221}
            }]
        });
        let mut names = HashMap::new();
        names.insert("010031F6".into(), "Hall TRV".into());
        let cfg = MerossConfig {
            labels: HashMap::from([("Hall TRV".into(), "Hall".into())]),
            ..MerossConfig::default()
        };
        let rooms = rooms_from_mts100_all(&payload, &names, &cfg);
        assert_eq!(rooms[0].name, "Hall");
        assert_eq!(rooms[0].temperature, "22.1°");
    }

    #[test]
    fn applies_labels_and_room_filter() {
        let payload = json!({
            "all": [
                {"id": "1", "latestTemperature": 200, "latestHumidity": 400},
                {"id": "2", "latestTemperature": 180, "latestHumidity": 550}
            ]
        });
        let mut names = HashMap::new();
        names.insert("1".into(), "Sensor A".into());
        names.insert("2".into(), "Sensor B".into());
        let cfg = MerossConfig {
            labels: HashMap::from([("Sensor A".into(), "Hall".into())]),
            ..MerossConfig::default()
        };
        let rooms = rooms_from_sensor_all(&payload, &names, &cfg);
        assert_eq!(rooms.len(), 2);
        assert_eq!(rooms[0].name, "Hall");
        assert_eq!(rooms[0].temperature, "20.0°");
        assert_eq!(rooms[0].humidity, "40%");
        assert_eq!(rooms[1].name, "Sensor B");
        assert_eq!(rooms[1].temperature, "18.0°");
        assert_eq!(rooms[1].humidity, "55%");
    }

    #[test]
    fn mqtt_host_strips_scheme() {
        assert_eq!(
            mqtt_host("mqtts://mqtt-eu-4.meross.com"),
            ("mqtt-eu-4.meross.com".into(), 443)
        );
        assert_eq!(
            mqtt_host("mqtt-eu-4.meross.com:8883"),
            ("mqtt-eu-4.meross.com".into(), 8883)
        );
    }

    #[test]
    fn command_sign_is_md5_of_id_key_timestamp() {
        let body = signed_command(
            "secret",
            "GET",
            "Appliance.System.All",
            json!({}),
            "uuid",
            "/app/1-x/subscribe",
        );
        let v: Value = serde_json::from_str(&body).unwrap();
        let mid = v.pointer("/header/messageId").unwrap().as_str().unwrap();
        let ts = v.pointer("/header/timestamp").unwrap().as_u64().unwrap();
        let sign = v.pointer("/header/sign").unwrap().as_str().unwrap();
        assert_eq!(sign, md5_hex(format!("{mid}secret{ts}").as_bytes()));
    }

    #[test]
    fn invalidate_rooms_drops_ttl_cache_only() {
        {
            let mut guard = ROOMS.lock().unwrap();
            *guard = Some((Instant::now(), demo_rooms()));
        }
        {
            let mut guard = CREDS.lock().unwrap();
            *guard = Some(CloudCreds {
                email: "a@b.c".into(),
                token: "t".into(),
                key: "k".into(),
                user_id: "1".into(),
                domain: "d".into(),
                mqtt_domain: "m".into(),
            });
        }
        invalidate_rooms();
        assert!(ROOMS.lock().unwrap().is_none());
        assert!(CREDS.lock().unwrap().is_some());
        invalidate_cache();
        assert!(CREDS.lock().unwrap().is_none());
    }
}
