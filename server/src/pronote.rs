//! Unofficial PRONOTE client for homework and grades.
//!
//! Index Education does not publish a student/parent API. This talks to the
//! same JSON endpoints the web client uses, following the protocol documented
//! by [pronotepy](https://github.com/bain3/pronotepy/blob/master/PRONOTE%20protocol.md).
//! Direct username/password on `eleve.html` / `parent.html` is supported;
//! ENT / EduConnect portals are not.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use chrono::{Datelike, NaiveDate};
use md5::{Digest, Md5};
use num_bigint::BigUint;
use rand::RngCore;
use serde_json::{json, Value};
use sha2::Sha256;
use tracing::info;

use crate::config::PronoteConfig;
use crate::ics;
use crate::model::{School, SchoolItem};

const FETCH_TTL: Duration = Duration::from_secs(15 * 60);
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:142.0) Gecko/20100101 Firefox/142.0";

/// Hard-coded in Pronote's `eleve.js` (`c_rsaPub_modulo_1024`).
const RSA_MODULO: &str = "130337874517286041778445012253514395801341480334668979416920989365464528904618150245388048105865059387076357492684573172203245221386376405947824377827224846860699130638566643129067735803555082190977267155957271492183684665050351182476506458843580431717209261903043895605014125081521285387341454154194253026277";
const RSA_EXPONENT: u32 = 65537;

const HOMEWORK_TAB: i64 = 88;
const GRADES_TAB: i64 = 198;
const MAX_HOMEWORK: usize = 3;
const MAX_GRADES: usize = 2;
const MAX_ITEMS: usize = 4;
const SUBJECT_MAX: usize = 14;
const DETAIL_MAX: usize = 22;

static LAST: Mutex<Option<(Instant, String, School)>> = Mutex::new(None);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Account {
    Student,
    Parent,
}

struct Session {
    http: reqwest::Client,
    root: String,
    espace: i64,
    session_id: i64,
    request_number: u64,
    key: [u8; 16],
    iv: [u8; 16],
    encrypt: bool,
    compress: bool,
    child_id: Option<String>,
    general: Value,
    ressource: Value,
}

pub async fn load_school(cfg: &PronoteConfig, today: NaiveDate) -> Result<School> {
    let url = cfg.url.trim();
    let username = cfg.username.trim();
    let password = cfg.password.trim();
    if url.is_empty() || username.is_empty() || password.is_empty() {
        bail!("Pronote url/username/password are empty");
    }
    let cache_key = format!("{url}\0{username}\0{}", cfg.child.trim());
    if let Some((at, key, school)) = LAST.lock().ok().and_then(|g| g.clone()) {
        if key == cache_key && at.elapsed() < FETCH_TTL {
            return Ok(school);
        }
    }

    let school = fetch_school(cfg, today).await?;
    info!(
        student = %school.student,
        homework = school
            .items
            .iter()
            .filter(|i| i.kind == "homework")
            .count(),
        grades = school.items.iter().filter(|i| i.kind == "grade").count(),
        "loaded Pronote school"
    );
    if let Ok(mut guard) = LAST.lock() {
        *guard = Some((Instant::now(), cache_key, school.clone()));
    }
    Ok(school)
}

pub fn demo_school(today: NaiveDate) -> School {
    School {
        student: "Léa".into(),
        average: "14.2".into(),
        items: vec![
            school_item(
                "homework",
                ics::day_label(today, today),
                "Maths",
                "exercises p.24",
                "",
            ),
            school_item(
                "homework",
                ics::day_label(today + chrono::Duration::days(1), today),
                "Français",
                "learn the poem",
                "",
            ),
            school_item(
                "homework",
                ics::day_label(today + chrono::Duration::days(2), today),
                "Histoire",
                "worksheet 3",
                "",
            ),
            school_item(
                "grade",
                ics::day_label(today - chrono::Duration::days(1), today),
                "Maths",
                "15.5/20",
                "high",
            ),
        ],
    }
}

async fn fetch_school(cfg: &PronoteConfig, today: NaiveDate) -> Result<School> {
    let account = parse_account(&cfg.account, &cfg.url);
    let mut session = Session::login(cfg, account).await?;
    if account == Account::Parent {
        session.select_child(cfg.child.trim()).await?;
    }
    session.load_school(today).await
}

impl Session {
    async fn login(cfg: &PronoteConfig, account: Account) -> Result<Self> {
        let (root, page) = split_root_page(&cfg.url, account);
        let http = reqwest::Client::builder()
            .cookie_store(true)
            .timeout(Duration::from_secs(25))
            .user_agent(USER_AGENT)
            .build()
            .context("Pronote HTTP client")?;

        let html_url = format!("{root}/{page}");
        let html = http
            .get(&html_url)
            .send()
            .await
            .with_context(|| format!("Pronote GET {html_url}"))?
            .error_for_status()
            .with_context(|| format!("Pronote GET {html_url}"))?
            .text()
            .await?;
        let attrs = parse_start_attrs(&html)?;
        let session_id: i64 = attrs
            .get("h")
            .ok_or_else(|| anyhow!("Pronote HTML is missing a session id"))?
            .parse()
            .context("Pronote session id")?;
        let espace: i64 = attrs.get("a").map(|s| s.parse().unwrap_or(3)).unwrap_or(3);
        let skip_encrypt = attrs
            .get("sCrA")
            .map(|s| truthy(s))
            .or_else(|| attrs.get("CrA").map(|s| !truthy(s)))
            .unwrap_or(true);
        let skip_compress = attrs
            .get("sCoA")
            .map(|s| truthy(s))
            .or_else(|| attrs.get("CoA").map(|s| !truthy(s)))
            .unwrap_or(true);
        let use_http_rsa = attrs.get("http").map(|s| truthy(s)).unwrap_or(false)
            || cfg.url.trim().to_ascii_lowercase().starts_with("http://");

        let mut raw_iv = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut raw_iv);
        let mut session = Self {
            http,
            root,
            espace,
            session_id,
            request_number: 1,
            key: md5_bytes(b""),
            iv: [0u8; 16],
            encrypt: !skip_encrypt,
            compress: !skip_compress,
            child_id: None,
            general: Value::Null,
            ressource: Value::Null,
        };

        let uuid = if use_http_rsa {
            base64::engine::general_purpose::STANDARD.encode(rsa_pkcs1_encrypt(&raw_iv)?)
        } else {
            base64::engine::general_purpose::STANDARD.encode(raw_iv)
        };
        let params = session
            .call(
                "FonctionParametres",
                None,
                json!({ "Uuid": uuid, "identifiantNav": Value::Null }),
                Some(md5_bytes(&raw_iv)),
            )
            .await?;
        session.general = inner_data(&params)
            .get("General")
            .cloned()
            .unwrap_or(Value::Null);

        let mut username = cfg.username.trim().to_string();
        let mut password = cfg.password.trim().to_string();
        let ident = session
            .call(
                "Identification",
                None,
                json!({
                    "genreConnexion": 0,
                    "genreEspace": espace,
                    "identifiant": username,
                    "pourENT": false,
                    "enConnexionAuto": false,
                    "demandeConnexionAuto": false,
                    "demandeConnexionAppliMobile": false,
                    "demandeConnexionAppliMobileJeton": false,
                    "enConnexionAppliMobile": false,
                    "uuidAppliMobile": "",
                    "loginTokenSAV": "",
                }),
                None,
            )
            .await?;
        let ident_data = inner_data(&ident);
        if json_truthy(ident_data, "modeCompLog") {
            username = username.to_lowercase();
        }
        if json_truthy(ident_data, "modeCompMdp") {
            password = password.to_lowercase();
        }
        let alea = json_str(ident_data, &["alea"]).unwrap_or_default();
        let challenge = json_str(ident_data, &["challenge"])
            .ok_or_else(|| anyhow!("Pronote Identification returned no challenge"))?;
        let mtp = sha256_hex_upper(format!("{alea}{password}").as_bytes());
        let temp_key = md5_bytes(format!("{username}{mtp}").as_bytes());
        let solved = solve_challenge(&challenge, &temp_key, &session.iv)?;

        let auth = session
            .call(
                "Authentification",
                None,
                json!({
                    "connexion": 0,
                    "challenge": solved,
                    "espace": espace,
                }),
                None,
            )
            .await?;
        let auth_data = inner_data(&auth);
        let cle = json_str(auth_data, &["cle"])
            .ok_or_else(|| anyhow!("Pronote login failed (check username/password)"))?;
        let decrypted_cle = aes_decrypt(&temp_key, &session.iv, &hex::decode(cle)?)?;
        let key_bytes = comma_bytes(&String::from_utf8(decrypted_cle).context("Pronote AES key")?)?;
        session.key = md5_bytes(&key_bytes);

        if auth_data.get("actionsDoubleAuth").is_some() {
            session
                .complete_2fa(cfg.pin.trim(), auth_data)
                .await
                .context("Pronote 2FA")?;
        }

        let user = session
            .call("ParametresUtilisateur", None, Value::Null, None)
            .await?;
        session.ressource = inner_data(&user)
            .get("ressource")
            .cloned()
            .unwrap_or(Value::Null);
        Ok(session)
    }

    async fn complete_2fa(&mut self, pin: &str, auth_data: &Value) -> Result<()> {
        let actions = double_auth_actions(auth_data);
        let verify_pin = actions.iter().any(|a| *a == 3);
        if !verify_pin {
            return Ok(());
        }
        if pin.is_empty() {
            bail!("Pronote asked for a 2FA PIN; set pronote.pin in config.toml");
        }
        let encrypted = hex::encode(aes_encrypt(&self.key, &self.iv, pin.as_bytes())?);
        let resp = self
            .call(
                "SecurisationCompteDoubleAuth",
                None,
                json!({ "action": 0, "codePin": encrypted }),
                None,
            )
            .await?;
        if !json_truthy(inner_data(&resp), "result") {
            bail!("Pronote 2FA PIN was rejected");
        }
        Ok(())
    }

    async fn select_child(&mut self, want: &str) -> Result<()> {
        let children: Vec<Value> = list_field(&self.ressource, "listeRessources")
            .into_iter()
            .cloned()
            .collect();
        if children.is_empty() {
            if let Some(id) = json_str(&self.ressource, &["N"]) {
                self.child_id = Some(id);
            }
            return Ok(());
        }
        let chosen = if want.is_empty() {
            children.into_iter().next()
        } else {
            let want_l = want.to_ascii_lowercase();
            children.into_iter().find(|c| {
                json_str(c, &["L"])
                    .map(|name| name.to_ascii_lowercase().contains(&want_l))
                    .unwrap_or(false)
            })
        }
        .ok_or_else(|| anyhow!("Pronote child `{want}` was not found"))?;
        self.child_id = json_str(&chosen, &["N"]);
        self.ressource = chosen;
        Ok(())
    }

    async fn load_school(&mut self, today: NaiveDate) -> Result<School> {
        let student = first_name(&json_str(&self.ressource, &["L"]).unwrap_or_default());
        let start_day = self
            .general
            .get("PremierLundi")
            .and_then(|v| json_str(v, &["V"]))
            .and_then(|s| parse_pronote_date(&s))
            .unwrap_or_else(|| NaiveDate::from_ymd_opt(today.year(), 9, 1).unwrap_or(today));
        let last_day = self
            .general
            .get("DerniereDate")
            .and_then(|v| json_str(v, &["V"]))
            .and_then(|s| parse_pronote_date(&s))
            .unwrap_or(today + chrono::Duration::days(14));

        let homework_to = (today + chrono::Duration::days(14)).min(last_day);
        let homework = self
            .homework(today, homework_to, start_day)
            .await
            .unwrap_or_default();

        let period = current_period(&self.ressource, &self.general, today);
        let (average, grades) = match period {
            Some((id, name)) => self
                .grades(&id, &name)
                .await
                .unwrap_or((String::new(), Vec::new())),
            None => (String::new(), Vec::new()),
        };

        Ok(build_school(student, average, homework, grades, today))
    }

    async fn homework(
        &mut self,
        from: NaiveDate,
        to: NaiveDate,
        start_day: NaiveDate,
    ) -> Result<Vec<(NaiveDate, String, String, bool)>> {
        let week_from = pronote_week(from, start_day);
        let week_to = pronote_week(to, start_day).max(week_from);
        let resp = self
            .call(
                "PageCahierDeTexte",
                Some(HOMEWORK_TAB),
                json!({
                    "domaine": { "_T": 8, "V": format!("[{week_from}..{week_to}]") }
                }),
                None,
            )
            .await?;
        let data = inner_data(&resp);
        let mut out = Vec::new();
        for item in list_field(data, "ListeTravauxAFaire") {
            let date = item
                .get("PourLe")
                .and_then(|v| json_str(v, &["V"]))
                .and_then(|s| parse_pronote_date(&s));
            let Some(date) = date else { continue };
            if date < from || date > to {
                continue;
            }
            let subject = item
                .get("Matiere")
                .and_then(|v| v.get("V"))
                .and_then(|v| json_str(v, &["L"]))
                .unwrap_or_default();
            let description = item
                .get("descriptif")
                .and_then(|v| json_str(v, &["V"]))
                .map(|s| strip_html(&s))
                .unwrap_or_default();
            let done = json_truthy(&item, "TAFFait");
            if subject.is_empty() && description.is_empty() {
                continue;
            }
            out.push((date, subject, description, done));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        Ok(out)
    }

    async fn grades(
        &mut self,
        period_id: &str,
        period_name: &str,
    ) -> Result<(String, Vec<(NaiveDate, String, String, String)>)> {
        let resp = self
            .call(
                "DernieresNotes",
                Some(GRADES_TAB),
                json!({ "Periode": { "N": period_id, "L": period_name } }),
                None,
            )
            .await?;
        let data = inner_data(&resp);
        let average = data
            .get("moyGenerale")
            .and_then(|v| json_str(v, &["V"]))
            .map(|s| format_grade_value(&s))
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        let mut grades = Vec::new();
        for item in list_field(data, "listeDevoirs") {
            let mark = item
                .get("note")
                .and_then(|v| json_str(v, &["V"]))
                .unwrap_or_default();
            let mark = format_grade_value(&mark);
            if mark.is_empty() || mark.starts_with('|') {
                continue;
            }
            let out_of = item
                .get("bareme")
                .and_then(|v| json_str(v, &["V"]))
                .map(|s| format_grade_value(&s))
                .unwrap_or_else(|| "20".into());
            let subject = item
                .get("service")
                .and_then(|v| v.get("V"))
                .and_then(|v| json_str(v, &["L"]))
                .unwrap_or_default();
            let date = item
                .get("date")
                .and_then(|v| json_str(v, &["V"]))
                .and_then(|s| parse_pronote_date(&s))
                .unwrap_or(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
            if subject.is_empty() {
                continue;
            }
            let detail = if out_of.is_empty() {
                mark.clone()
            } else {
                format!("{mark}/{out_of}")
            };
            let level = grade_level(&mark, &out_of);
            grades.push((date, subject, detail, level));
        }
        grades.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        Ok((average, grades))
    }

    async fn call(
        &mut self,
        function: &str,
        onglet: Option<i64>,
        data: Value,
        next_iv: Option<[u8; 16]>,
    ) -> Result<Value> {
        let mut payload = serde_json::Map::new();
        if let Some(tab) = onglet {
            let mut signature = json!({ "onglet": tab });
            if let Some(id) = &self.child_id {
                signature["membre"] = json!({ "N": id, "G": 4 });
            }
            payload.insert("Signature".into(), signature);
        }
        if !data.is_null() {
            payload.insert("data".into(), data);
        }
        let body = Value::Object(payload);

        let mut data_sec = serde_json::to_vec(&body)?;
        if self.compress {
            let hex_json = hex::encode(&data_sec);
            data_sec = deflate_raw(hex_json.as_bytes())?;
        }
        let data_sec_field = if self.encrypt {
            Value::String(hex::encode_upper(aes_encrypt(
                &self.key, &self.iv, &data_sec,
            )?))
        } else if self.compress {
            Value::String(hex::encode_upper(data_sec))
        } else {
            body
        };

        let order = hex::encode(aes_encrypt(
            &self.key,
            &self.iv,
            self.request_number.to_string().as_bytes(),
        )?);
        let request = json!({
            "session": self.session_id,
            "no": order,
            "id": function,
            "dataSec": data_sec_field,
        });
        let url = format!(
            "{}/appelfonction/{}/{}/{}",
            self.root, self.espace, self.session_id, order
        );
        let response = self
            .http
            .post(&url)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("Pronote POST {function}"))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        self.request_number += 2;
        if let Some(iv) = next_iv {
            self.iv = iv;
        }
        if !status.is_success() {
            bail!("Pronote {function} HTTP {status}: {}", clip(&text, 180));
        }
        let parsed: Value = serde_json::from_str(&text)
            .with_context(|| format!("Pronote {function} JSON: {}", clip(&text, 180)))?;
        if let Some(err) = parsed.get("Erreur") {
            let code = err.get("G").and_then(|v| v.as_i64()).unwrap_or(0);
            let title = json_str(err, &["Titre"]).unwrap_or_else(|| "error".into());
            bail!("Pronote {function} error {code}: {title}");
        }
        let mut sec = parsed
            .get("dataSec")
            .cloned()
            .or_else(|| parsed.get("donneesSec").cloned())
            .unwrap_or(Value::Null);
        if let Some(hex_str) = sec.as_str() {
            let mut bytes = hex::decode(hex_str).context("Pronote dataSec hex")?;
            if self.encrypt {
                bytes = aes_decrypt(&self.key, &self.iv, &bytes)?;
            }
            if self.compress {
                bytes = inflate_raw(&bytes)?;
                if let Ok(as_text) = std::str::from_utf8(&bytes) {
                    if as_text.bytes().all(|b| b.is_ascii_hexdigit()) && as_text.len() % 2 == 0 {
                        if let Ok(decoded) = hex::decode(as_text) {
                            bytes = decoded;
                        }
                    }
                }
            }
            sec = serde_json::from_slice(&bytes).context("Pronote decrypted dataSec")?;
        }
        let mut out = parsed;
        if let Some(obj) = out.as_object_mut() {
            obj.insert("dataSec".into(), sec);
        }
        Ok(out)
    }
}

fn build_school(
    student: String,
    average: String,
    homework: Vec<(NaiveDate, String, String, bool)>,
    grades: Vec<(NaiveDate, String, String, String)>,
    today: NaiveDate,
) -> School {
    let mut items = Vec::new();
    for (date, subject, description, done) in homework {
        if done {
            continue;
        }
        if items.len() >= MAX_HOMEWORK || items.len() >= MAX_ITEMS {
            break;
        }
        let detail = if description.is_empty() {
            subject.clone()
        } else {
            description
        };
        items.push(school_item(
            "homework",
            ics::day_label(date, today),
            &subject,
            &detail,
            "",
        ));
    }
    let grade_slots =
        MAX_ITEMS
            .saturating_sub(items.len())
            .min(MAX_GRADES)
            .max(if items.is_empty() {
                MAX_GRADES.min(MAX_ITEMS)
            } else {
                0
            });
    let grade_slots = if grade_slots == 0 && items.len() < MAX_ITEMS {
        1.min(MAX_ITEMS - items.len())
    } else {
        grade_slots
    };
    for (date, subject, detail, level) in grades.into_iter().take(grade_slots) {
        items.push(school_item(
            "grade",
            ics::day_label(date, today),
            &subject,
            &detail,
            &level,
        ));
    }
    School {
        student,
        average,
        items,
    }
}

fn school_item(
    kind: &str,
    when: impl Into<String>,
    subject: &str,
    detail: &str,
    level: &str,
) -> SchoolItem {
    SchoolItem {
        kind: kind.into(),
        when: when.into(),
        subject: clip(subject, SUBJECT_MAX),
        detail: clip(detail, DETAIL_MAX),
        done: false,
        level: level.into(),
    }
}

fn parse_account(account: &str, url: &str) -> Account {
    match account.trim().to_ascii_lowercase().as_str() {
        "parent" | "parents" => Account::Parent,
        "eleve" | "élève" | "student" => Account::Student,
        _ if url.to_ascii_lowercase().contains("parent") => Account::Parent,
        _ => Account::Student,
    }
}

fn split_root_page(url: &str, account: Account) -> (String, String) {
    let url = url.trim();
    let without_hash = url.split_once('#').map(|(a, _)| a).unwrap_or(url);
    let (path_part, query) = without_hash
        .split_once('?')
        .map(|(p, q)| (p, q))
        .unwrap_or((without_hash, ""));
    let path_part = path_part.trim_end_matches('/');
    let default_page = match account {
        Account::Parent => "parent.html",
        Account::Student => "eleve.html",
    };
    if path_part
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains(".html"))
    {
        let (root, page) = path_part.rsplit_once('/').unwrap();
        let page = if query.is_empty() {
            page.to_string()
        } else {
            format!("{page}?{query}")
        };
        (root.to_string(), page)
    } else {
        (path_part.to_string(), default_page.to_string())
    }
}

fn parse_start_attrs(html: &str) -> Result<HashMap<String, String>> {
    let lower = html.to_ascii_lowercase();
    if html.contains("IP") && (lower.contains("suspend") || lower.contains("bloqu")) {
        bail!("Pronote has suspended this IP address");
    }
    let start = html
        .find("Start ({")
        .or_else(|| html.find("Start({"))
        .or_else(|| html.find("Start"))
        .ok_or_else(|| anyhow!("Pronote HTML is missing Start({{...}})"))?;
    let rest = &html[start..];
    let open = rest
        .find('{')
        .ok_or_else(|| anyhow!("Pronote HTML Start() has no object"))?;
    let close = rest[open..]
        .find('}')
        .ok_or_else(|| anyhow!("Pronote HTML Start() object is unclosed"))?;
    let obj = &rest[open..open + close + 1];
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(obj) {
        let mut attrs = HashMap::new();
        for (key, value) in map {
            attrs.insert(key, json_value_to_string(value));
        }
        if !attrs.contains_key("h") {
            bail!("Pronote HTML Start() is missing session id h");
        }
        return Ok(attrs);
    }
    let body = &rest[open + 1..open + close];
    let mut attrs = HashMap::new();
    for part in body.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((key, value)) = part.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches(['\'', '"']).to_string();
        let value = value.trim().trim_matches(['\'', '"']).to_string();
        attrs.insert(key, value);
    }
    if !attrs.contains_key("h") {
        bail!("Pronote HTML Start() is missing session id h");
    }
    Ok(attrs)
}

fn json_value_to_string(v: Value) -> String {
    match v {
        Value::String(s) => s,
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn inner_data(v: &Value) -> &Value {
    v.get("dataSec")
        .or_else(|| v.get("donneesSec"))
        .and_then(|sec| sec.get("data").or_else(|| sec.get("donnees")))
        .unwrap_or(v)
}

fn json_str(v: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        match v.get(*key) {
            Some(Value::String(s)) if !s.is_empty() => return Some(s.clone()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            Some(Value::Bool(b)) => return Some(b.to_string()),
            _ => {}
        }
    }
    None
}

fn json_truthy(v: &Value, key: &str) -> bool {
    match v.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(Value::String(s)) => truthy(s),
        _ => false,
    }
}

fn list_field<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    match v.get(key) {
        Some(Value::Array(arr)) => arr.iter().collect(),
        Some(obj) => obj
            .get("V")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().collect())
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

fn truthy(s: &str) -> bool {
    matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
}

fn current_period(
    ressource: &Value,
    general: &Value,
    today: NaiveDate,
) -> Option<(String, String)> {
    let onglets = list_field(ressource, "listeOngletsPourPeriodes");
    let grades_onglet = onglets
        .iter()
        .find(|o| o.get("G").and_then(|g| g.as_i64()) == Some(GRADES_TAB))
        .or_else(|| onglets.first());
    if let Some(onglet) = grades_onglet {
        if let Some(period) = onglet.get("periodeParDefaut").and_then(|v| v.get("V")) {
            let id = json_str(period, &["N"])?;
            let name = json_str(period, &["L"]).unwrap_or_default();
            return Some((id, name));
        }
    }
    for period in list_field(general, "ListePeriodes") {
        let start = period
            .get("dateDebut")
            .and_then(|v| json_str(v, &["V"]))
            .and_then(|s| parse_pronote_date(&s));
        let end = period
            .get("dateFin")
            .and_then(|v| json_str(v, &["V"]))
            .and_then(|s| parse_pronote_date(&s));
        if let (Some(start), Some(end)) = (start, end) {
            if start <= today && today <= end {
                let id = json_str(&period, &["N"])?;
                let name = json_str(&period, &["L"]).unwrap_or_default();
                return Some((id, name));
            }
        }
    }
    let period = list_field(general, "ListePeriodes").into_iter().next()?;
    Some((
        json_str(period, &["N"])?,
        json_str(period, &["L"]).unwrap_or_default(),
    ))
}

fn pronote_week(date: NaiveDate, start_day: NaiveDate) -> i64 {
    1 + (date - start_day).num_days().div_euclid(7)
}

fn parse_pronote_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    let date = s.split_whitespace().next().unwrap_or(s);
    NaiveDate::parse_from_str(date, "%d/%m/%Y")
        .or_else(|_| NaiveDate::parse_from_str(date, "%d/%m/%y"))
        .ok()
}

fn format_grade_value(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('|') {
        return String::new();
    }
    let normalized = raw.replace(',', ".");
    if let Ok(n) = normalized.parse::<f64>() {
        if n.fract() == 0.0 {
            return format!("{n:.0}");
        }
        let s = format!("{n:.2}");
        return s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    raw.to_string()
}

fn grade_level(mark: &str, out_of: &str) -> String {
    let mark = mark.replace(',', ".").parse::<f64>().ok();
    let out_of = out_of.replace(',', ".").parse::<f64>().ok();
    match (mark, out_of) {
        (Some(m), Some(max)) if max > 0.0 => {
            let ratio = m / max;
            if ratio >= 0.8 {
                "high".into()
            } else if ratio < 0.5 {
                "low".into()
            } else {
                "mid".into()
            }
        }
        _ => String::new(),
    }
}

fn first_name(full: &str) -> String {
    full.split_whitespace()
        .next()
        .unwrap_or(full)
        .trim()
        .to_string()
}

fn clip(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        return s;
    }
    let take = max.saturating_sub(1);
    let mut out: String = s.chars().take(take).collect();
    while out.ends_with(' ') {
        out.pop();
    }
    out.push('…');
    out
}

fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    html_unescape(&out)
}

fn html_unescape(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn double_auth_actions(auth_data: &Value) -> Vec<i64> {
    let raw = auth_data
        .get("actionsDoubleAuth")
        .and_then(|v| json_str(v, &["V"]).or_else(|| v.as_str().map(|s| s.to_string())));
    let Some(raw) = raw else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<i64>>(&raw).unwrap_or_default()
}

fn solve_challenge(challenge: &str, key: &[u8; 16], iv: &[u8; 16]) -> Result<String> {
    if let Ok(raw) = hex::decode(challenge) {
        if let Ok(plain) = aes_decrypt(key, iv, &raw) {
            if let Ok(text) = String::from_utf8(plain) {
                let stripped: String = text
                    .chars()
                    .enumerate()
                    .filter_map(|(i, c)| (i % 2 == 0).then_some(c))
                    .collect();
                return Ok(hex::encode(aes_encrypt(key, iv, stripped.as_bytes())?));
            }
        }
    }
    Ok(hex::encode(aes_encrypt(key, iv, challenge.as_bytes())?))
}

fn comma_bytes(s: &str) -> Result<Vec<u8>> {
    s.split(',')
        .map(|p| {
            p.trim()
                .parse::<u8>()
                .map_err(|_| anyhow!("bad Pronote key byte `{p}`"))
        })
        .collect()
}

fn md5_bytes(data: &[u8]) -> [u8; 16] {
    Md5::digest(data).into()
}

fn sha256_hex_upper(data: &[u8]) -> String {
    hex::encode_upper(Sha256::digest(data))
}

fn aes_encrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    use aes::Aes128;
    use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    use cbc::Encryptor;

    Encryptor::<Aes128>::new_from_slices(key, iv)
        .map(|enc| enc.encrypt_padded_vec_mut::<Pkcs7>(data))
        .map_err(|err| anyhow!("AES encrypt init: {err}"))
}

fn aes_decrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    use aes::Aes128;
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    use cbc::Decryptor;

    Decryptor::<Aes128>::new_from_slices(key, iv)
        .map_err(|err| anyhow!("AES decrypt init: {err}"))?
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .map_err(|err| anyhow!("AES decrypt: {err}"))
}

fn rsa_pkcs1_encrypt(data: &[u8]) -> Result<Vec<u8>> {
    let n = BigUint::parse_bytes(RSA_MODULO.as_bytes(), 10)
        .ok_or_else(|| anyhow!("bad Pronote RSA modulus"))?;
    let e = BigUint::from(RSA_EXPONENT);
    let k = ((n.bits() + 7) / 8) as usize;
    if data.len() + 11 > k {
        bail!("Pronote RSA payload is too long");
    }
    let mut em = vec![0x00, 0x02];
    let ps_len = k - data.len() - 3;
    let mut rng = rand::thread_rng();
    while em.len() < 2 + ps_len {
        let mut b = [0u8; 1];
        rng.fill_bytes(&mut b);
        if b[0] != 0 {
            em.push(b[0]);
        }
    }
    em.push(0x00);
    em.extend_from_slice(data);
    let m = BigUint::from_bytes_be(&em);
    let c = m.modpow(&e, &n);
    let mut out = c.to_bytes_be();
    while out.len() < k {
        out.insert(0, 0);
    }
    Ok(out)
}

fn deflate_raw(data: &[u8]) -> Result<Vec<u8>> {
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

fn inflate_raw(data: &[u8]) -> Result<Vec<u8>> {
    let mut dec = flate2::read::DeflateDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_numero_ordre_matches_protocol() {
        let key = md5_bytes(b"");
        let iv = [0u8; 16];
        let order = hex::encode(aes_encrypt(&key, &iv, b"1").unwrap());
        assert_eq!(order, "3fa959b13967e0ef176069e01e23c8d7");
    }

    #[test]
    fn parses_start_attributes() {
        let html = r#"<body onload="try { Start ({h:'2052117',sCrA:true,sCoA:true,a:3,d:true}) } catch (e) {}">"#;
        let attrs = parse_start_attrs(html).unwrap();
        assert_eq!(attrs.get("h").unwrap(), "2052117");
        assert_eq!(attrs.get("sCrA").unwrap(), "true");
        assert_eq!(attrs.get("a").unwrap(), "3");
    }

    #[test]
    fn parses_json_start_attributes() {
        let html = r#"window.addEventListener("load", () => {try{Start ({"h":1697793,"d":true,"a":3});} catch (e) {}})"#;
        let attrs = parse_start_attrs(html).unwrap();
        assert_eq!(attrs.get("h").unwrap(), "1697793");
        assert_eq!(attrs.get("a").unwrap(), "3");
        assert_eq!(attrs.get("d").unwrap(), "true");
    }

    #[test]
    fn infers_parent_from_url() {
        assert!(matches!(
            parse_account("", "https://school.example/pronote/parent.html"),
            Account::Parent
        ));
        assert!(matches!(
            parse_account("eleve", "https://school.example/pronote/parent.html"),
            Account::Student
        ));
    }

    #[test]
    fn splits_pronote_root() {
        let (root, page) = split_root_page(
            "https://demo.index-education.net/pronote/eleve.html",
            Account::Student,
        );
        assert_eq!(root, "https://demo.index-education.net/pronote");
        assert_eq!(page, "eleve.html");
    }

    #[test]
    fn strips_homework_html() {
        assert_eq!(
            strip_html("<div>Learn the <b>poem</b>&nbsp;tonight</div>"),
            "Learn the poem tonight"
        );
    }

    #[test]
    fn formats_french_grades() {
        assert_eq!(format_grade_value("15,50"), "15.5");
        assert_eq!(format_grade_value("20"), "20");
        assert_eq!(format_grade_value("|1"), "");
        assert_eq!(grade_level("16", "20"), "high");
        assert_eq!(grade_level("8", "20"), "low");
        assert_eq!(grade_level("12", "20"), "mid");
    }

    #[test]
    fn builds_compact_school_rows() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        let school = build_school(
            "Léa".into(),
            "14.2".into(),
            vec![
                (today, "Maths".into(), "exercises p.24".into(), false),
                (
                    today + chrono::Duration::days(1),
                    "Français".into(),
                    "poem".into(),
                    false,
                ),
                (
                    today + chrono::Duration::days(2),
                    "Histoire".into(),
                    "worksheet".into(),
                    true,
                ),
            ],
            vec![(
                today - chrono::Duration::days(1),
                "Maths".into(),
                "15.5/20".into(),
                "high".into(),
            )],
            today,
        );
        assert_eq!(school.student, "Léa");
        assert_eq!(school.average, "14.2");
        assert_eq!(school.items.len(), 3);
        assert_eq!(school.items[0].kind, "homework");
        assert_eq!(school.items[0].when, "Today");
        assert_eq!(school.items[2].kind, "grade");
        assert_eq!(school.items[2].detail, "15.5/20");
    }

    #[test]
    fn demo_school_is_visible() {
        let school = demo_school(NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        assert!(school.is_visible());
        assert!(!school.items.is_empty());
    }

    #[tokio::test]
    #[ignore = "hits Index Education's public demo"]
    async fn demo_index_education_returns_school() {
        let cfg = PronoteConfig {
            url: "https://demo.index-education.net/pronote/eleve.html".into(),
            username: "demonstration".into(),
            password: "pronotevs".into(),
            ..PronoteConfig::default()
        };
        let today = chrono::Utc::now().date_naive();
        let school = load_school(&cfg, today).await.expect("demo login");
        assert!(
            school.is_visible(),
            "demo school was empty: student={} items={}",
            school.student,
            school.items.len()
        );
    }
}
