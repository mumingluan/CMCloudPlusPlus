use crate::cloud::AccountConfig;
use aes::cipher::{BlockDecrypt, KeyInit};
use aes::{Aes128, Aes192};
use base64::{engine::general_purpose::STANDARD, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use chrono::Utc;
use rand::{rngs::OsRng, RngCore};
use reqwest::{header::HeaderMap, Client, Response};
use rsa::{pkcs8::DecodePublicKey, Pkcs1v15Encrypt, RsaPublicKey};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;

pub const KEY_ONE: &str = "73634235495062495331515373756c734e7253306c673d3d";
const KEY_TWO: &str = "7150714477323633586746674c337538";
const MAIL_PUBLIC_KEY: &str = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAnsOHTFtwW5rq/8gGhPlM5Z3RPdeN/d+FYIHb5JmcfBOCozXuT8c+0anvxtkjzghixwNlnmBuhN8OYfS789YuH/qReQbHC7OdlisLildNWPHRNUYcPa0W3lXSG3+81CXK7FDXPvXo5ubw2GqVbIsccMarI1dyfXdi4ITiCXvmM9wYBdUs9yXtoorhlpyYUI2GV8HNsQjWK9P5QZHT3ox5Qy+mjRmvv6RUFJLPOMkOS/pGZ0DwC1ypFZBxstW0/ftVupdOmGWvW7J2/e3dq3A/UvIkC4YUY/diL1wighJx1G9MiRROISjNMvNyUDSTqPJy516+l3sgHEbc067QIJx2NQIDAQAB";

fn sha1(value: &str) -> String {
    hex::encode(Sha1::digest(value.as_bytes()))
}
fn cookie_value(cookies: &str, name: &str) -> Option<String> {
    cookies.split(';').find_map(|item| {
        let (key, value) = item.trim().split_once('=')?;
        (key == name && !value.is_empty()).then(|| value.to_owned())
    })
}
pub fn merge_cookies(old: &str, response: &Response) -> String {
    let mut cookies: BTreeMap<String, String> = old
        .split(';')
        .filter_map(|item| {
            item.trim()
                .split_once('=')
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        })
        .collect();
    for item in response.headers().get_all(reqwest::header::SET_COOKIE) {
        if let Ok(item) = item.to_str() {
            if let Some((key, value)) = item.split(';').next().unwrap_or_default().split_once('=') {
                cookies.insert(key.to_owned(), value.to_owned());
            }
        }
    }
    cookies
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn xml_field(key: &str, value: &str) -> String {
    format!(
        "<string name=\"{}\">{}</string>",
        xml_escape(key),
        xml_escape(value)
    )
}

// Some 139 Mail deployments return JavaScript object literals instead of JSON.
// Parse only the three string fields used for login; never execute returned text.
fn parse_mail_response(text: &str) -> Result<Value, String> {
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return Ok(value);
    }
    fn field(text: &str, name: &str) -> Option<String> {
        let mut remainder = text;
        while let Some(index) = remainder.find(name) {
            let after = &remainder[index + name.len()..];
            let after = after.trim_start_matches(['\'', '"']).trim_start();
            if let Some(after) = after.strip_prefix(':') {
                let after = after.trim_start();
                let quote = after.chars().next()?;
                if quote == '\'' || quote == '"' {
                    let mut output = String::new();
                    let mut escape = false;
                    for c in after[quote.len_utf8()..].chars() {
                        if escape {
                            output.push(c);
                            escape = false;
                        } else if c == '\\' {
                            escape = true;
                        } else if c == quote {
                            return Some(output);
                        } else {
                            output.push(c);
                        }
                    }
                }
            }
            remainder = &remainder[index + name.len()..];
        }
        None
    }
    let code = field(text, "code").ok_or("邮箱响应格式无效")?;
    Ok(
        json!({"code":code,"summary":field(text,"summary").unwrap_or_default(),"var":{"loginSuccessUrl":field(text,"loginSuccessUrl").unwrap_or_default()}}),
    )
}

pub fn encrypt_payload(value: &Value, key_hex: &str) -> Result<String, String> {
    let key = hex::decode(key_hex).map_err(|e| e.to_string())?;
    let text = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let mut iv = [0u8; 16];
    OsRng.fill_bytes(&mut iv);
    let encrypted = match key.len() {
        16 => cbc::Encryptor::<Aes128>::new_from_slices(&key, &iv)
            .map_err(|e| e.to_string())?
            .encrypt_padded_vec_mut::<Pkcs7>(&text),
        24 => cbc::Encryptor::<Aes192>::new_from_slices(&key, &iv)
            .map_err(|e| e.to_string())?
            .encrypt_padded_vec_mut::<Pkcs7>(&text),
        _ => return Err("不支持的云盘加密密钥长度".into()),
    };
    Ok(STANDARD.encode([iv.as_slice(), encrypted.as_slice()].concat()))
}
pub fn decrypt_payload(text: &str, key_hex: &str) -> Result<Value, String> {
    if text.trim_start().starts_with('{') {
        return serde_json::from_str(text).map_err(|_| "服务端 JSON 响应格式无效".into());
    }
    let key = hex::decode(key_hex).map_err(|e| e.to_string())?;
    let data = STANDARD
        .decode(text.trim().trim_matches('"'))
        .map_err(|_| "云盘加密响应编码无效")?;
    if data.len() < 32 || (data.len() - 16) % 16 != 0 {
        return Err("云盘加密响应长度无效".into());
    }
    let (iv, cipher) = data.split_at(16);
    let plain = match key.len() {
        16 => cbc::Decryptor::<Aes128>::new_from_slices(&key, iv)
            .map_err(|e| e.to_string())?
            .decrypt_padded_vec_mut::<Pkcs7>(cipher),
        24 => cbc::Decryptor::<Aes192>::new_from_slices(&key, iv)
            .map_err(|e| e.to_string())?
            .decrypt_padded_vec_mut::<Pkcs7>(cipher),
        _ => return Err("不支持的云盘加密密钥长度".into()),
    }
    .map_err(|_| "云盘响应解密失败")?;
    serde_json::from_slice(&plain).map_err(|_| "云盘解密响应格式无效".into())
}
pub async fn encrypted_post(
    http: &Client,
    url: &str,
    value: &Value,
    headers: HeaderMap,
    key: &str,
) -> Result<Value, String> {
    let payload = encrypt_payload(value, key)?;
    let response = http
        .post(url)
        .headers(headers)
        .body(payload)
        .send()
        .await
        .map_err(|e| format!("云盘连接失败：{}", e.without_url()))?;
    let status = response.status();
    let bytes = read_body_limited(response, 32 * 1024 * 1024).await?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "云盘响应编码无效")?;
    if !status.is_success() {
        return Err(format!("云盘服务返回 HTTP {}", status.as_u16()));
    }
    decrypt_payload(text, key)
}

pub async fn read_body_limited(mut response: Response, limit: usize) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("云盘响应超出大小限制，请缩小查询范围".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取云盘响应失败")? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err("云盘响应超出大小限制，请缩小查询范围".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn read_text_limited(response: Response, limit: usize) -> Result<String, String> {
    String::from_utf8(read_body_limited(response, limit).await?)
        .map_err(|_| "云盘响应编码无效".into())
}

pub fn account_from_authorization(auth: &str) -> Result<String, String> {
    let raw = auth.trim().strip_prefix("Basic ").unwrap_or(auth.trim());
    let bytes = STANDARD
        .decode(raw)
        .map_err(|_| "Authorization 必须为 Base64 编码")?;
    let text = String::from_utf8(bytes).map_err(|_| "Authorization 格式无效")?;
    let mut split = text.splitn(3, ':');
    let _ = split.next();
    let account = split
        .next()
        .filter(|s| !s.is_empty())
        .ok_or("Authorization 缺少账号")?;
    let token = split
        .next()
        .filter(|s| !s.is_empty())
        .ok_or("Authorization 缺少令牌")?;
    if token.split('|').count() < 4 {
        return Err("Authorization 令牌格式无效".into());
    }
    Ok(account.to_owned())
}
pub async fn authenticate(http: &Client, cfg: &mut AccountConfig) -> Result<(), String> {
    cfg.authorization = cfg
        .authorization
        .trim()
        .trim_start_matches("Basic ")
        .to_owned();
    cfg.username = cfg.username.trim().to_owned();
    cfg.mail_cookies = cfg.mail_cookies.trim().to_owned();
    if cfg.space == "share"
        && cfg.authorization.is_empty()
        && cfg.username.is_empty()
        && cfg.mail_cookies.is_empty()
    {
        return Ok(());
    }
    if !cfg.authorization.is_empty() {
        match refresh(http, cfg, false).await {
            Ok(()) => return Ok(()),
            Err(cause) => {
                if cfg.username.is_empty() || cfg.password.is_empty() {
                    return Err(cause);
                }
                return password_login(http, cfg).await;
            }
        }
    }
    if let Some(sid) = cookie_value(&cfg.mail_cookies, "Os_SSo_Sid") {
        if cookie_value(&cfg.mail_cookies, "RMKEY").is_some()
            && exchange_and_login(http, cfg, &sid).await.is_ok()
        {
            return Ok(());
        }
    }
    if cfg.username.is_empty() != cfg.password.is_empty() {
        return Err("账号和密码需要一起填写".into());
    }
    if cfg.username.is_empty() || cfg.password.is_empty() {
        return Err("请使用账号密码、Authorization 或有效的 139 邮箱 Cookie 登录".into());
    }
    password_login(http, cfg).await
}
pub async fn refresh(http: &Client, cfg: &mut AccountConfig, force: bool) -> Result<(), String> {
    let account = account_from_authorization(&cfg.authorization)?;
    let bytes = STANDARD
        .decode(&cfg.authorization)
        .map_err(|_| "Authorization 编码无效")?;
    let text = String::from_utf8(bytes).map_err(|_| "Authorization 格式无效")?;
    let split: Vec<&str> = text.splitn(3, ':').collect();
    let expiration = split[2]
        .split('|')
        .nth(3)
        .ok_or("令牌缺少有效期")?
        .parse::<i64>()
        .map_err(|_| "令牌有效期无效")?;
    let remaining = expiration - Utc::now().timestamp_millis();
    if remaining <= 0 {
        return Err("登录已过期，请重新登录".into());
    }
    if !force && remaining > 15 * 24 * 60 * 60 * 1000 {
        return Ok(());
    }
    let body = format!(
        "<root><token>{}</token><account>{}</account><clienttype>656</clienttype></root>",
        xml_escape(split[2]),
        xml_escape(&account)
    );
    let response = http
        .post("https://aas.caiyun.feixin.10086.cn:443/tellin/authTokenRefresh.do")
        .header("Content-Type", "application/xml")
        .body(body)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !response.status().is_success() {
        return Err("令牌续期请求失败".into());
    }
    let body = read_text_limited(response, 1024 * 1024).await?;
    #[derive(serde::Deserialize)]
    struct Refresh {
        #[serde(rename = "return")]
        result: String,
        token: Option<String>,
        desc: Option<String>,
    }
    let result: Refresh = quick_xml::de::from_str(&body).map_err(|_| "令牌续期响应无效")?;
    if result.result != "0" {
        return Err(format!("令牌续期失败：{}", result.desc.unwrap_or_default()));
    }
    let token = result
        .token
        .filter(|s| !s.is_empty())
        .ok_or("续期未返回令牌")?;
    cfg.authorization = STANDARD.encode(format!("{}:{account}:{token}", split[0]));
    Ok(())
}
async fn password_login(http: &Client, cfg: &mut AccountConfig) -> Result<(), String> {
    // A separate client retains transport settings while preventing SSO redirects.
    let no_redirect = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let password = sha1(&format!("fetion.com.cn:{}", cfg.password));
    let response=no_redirect.post("https://mail.10086.cn/Login/Login.ashx")
        .header("Cookie",&cfg.mail_cookies).header("Origin","https://mail.10086.cn").header("Referer","https://mail.10086.cn/default.html")
        .header("User-Agent","Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0")
        .form(&[("UserName",cfg.username.as_str()),("passOld",""),("auto","on"),("Password",password.as_str()),("webIndexPagePwdLogin","1"),("pwdType","1"),("clientId","1003"),("authType","2")])
        .send().await.map_err(|e|format!("邮箱登录连接失败：{}",e.without_url()))?;
    cfg.mail_cookies = merge_cookies(&cfg.mail_cookies, &response);
    let location = response
        .headers()
        .get("Location")
        .and_then(|s| s.to_str().ok())
        .unwrap_or_default();
    let location = url::Url::parse(location).ok();
    let risk = location.as_ref().and_then(|u| {
        u.query_pairs()
            .find(|(k, _)| k == "ec")
            .map(|(_, v)| v.into_owned())
    });
    let sid = if let Some(risk) = risk {
        let scene = match risk.as_str() {
            "PML401010062" => 2,
            "MW0016" => 4,
            "S025" | "S035" => 1,
            _ => {
                return Err(format!(
                    "邮箱需要额外验证（{risk}），请在官方应用完成验证后导入凭据"
                ))
            }
        };
        if cfg.sms_code.trim().is_empty() {
            sms_request(http, cfg, scene, None, &risk).await?;
            return Err(format!(
                "SMS_REQUIRED:{risk}:验证码已发送，请输入验证码继续登录"
            ));
        }
        sms_request(http, cfg, scene, Some(cfg.sms_code.clone()), &risk)
            .await?
            .ok_or("验证码验证未返回登录会话")?
    } else {
        location
            .and_then(|u| {
                u.query_pairs()
                    .find(|(k, _)| k == "sid")
                    .map(|(_, v)| v.into_owned())
            })
            .or_else(|| cookie_value(&cfg.mail_cookies, "Os_SSo_Sid"))
            .or_else(|| cookie_value(&cfg.mail_cookies, "sid"))
            .ok_or("登录失败，请检查账号密码或在官方应用完成验证后导入凭据")?
    };
    exchange_and_login(http, cfg, &sid).await
}
async fn sms_request(
    http: &Client,
    cfg: &mut AccountConfig,
    scene: u32,
    code: Option<String>,
    risk: &str,
) -> Result<Option<String>, String> {
    let public = RsaPublicKey::from_public_key_der(
        &STANDARD
            .decode(MAIL_PUBLIC_KEY)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let encrypted = public
        .encrypt(&mut OsRng, Pkcs1v15Encrypt, cfg.username.as_bytes())
        .map_err(|e| e.to_string())?;
    let login_name = STANDARD.encode(encrypted);
    let mut fields = vec![
        ("loginName", login_name),
        ("clientId", "1003".into()),
        ("eMode", "1".into()),
        ("verifyCode", "".into()),
    ];
    let function = if let Some(code) = code.as_ref() {
        fields.extend([
            ("version", "4".into()),
            ("loginType", "0".into()),
            ("authType", "2".into()),
            ("loginPassword", sha1(&format!("fetion.com.cn:{code}"))),
            ("createAutoLoginSecretKey", "1".into()),
            ("verifyAgentId", "".into()),
            ("reqFrom", "3".into()),
            ("needWCookie", "1".into()),
        ]);
        if risk == "MW0016" {
            fields.push(("pwdType", "1".into()));
        }
        "/login/inlogin.action"
    } else {
        fields.extend([
            ("fv", "4".into()),
            ("loginFailureUrl", "".into()),
            ("loginSuccessUrl", "".into()),
            ("version", "1.0".into()),
            ("scene", scene.to_string()),
        ]);
        "login:sendSmsCodeByScene"
    };
    let body = format!(
        "<object>{}</object>",
        fields
            .into_iter()
            .map(|(k, v)| xml_field(k, &v))
            .collect::<String>()
    );
    let response = http
        .post("https://mail.10086.cn/s")
        .query(&[
            ("func", function),
            ("cguid", &Utc::now().timestamp_millis().to_string()),
        ])
        .header("Cookie", &cfg.mail_cookies)
        .header("User-Agent", "okhttp/4.12.0")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(body)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    cfg.mail_cookies = merge_cookies(&cfg.mail_cookies, &response);
    if !response.status().is_success() {
        return Err(format!(
            "验证码服务返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let text = read_text_limited(response, 1024 * 1024).await?;
    let value = parse_mail_response(&text)?;
    let result = value["code"].as_str().unwrap_or_default();
    if result != "S_OK" {
        return Err(match result {
            "PML401010021" | "PML401010022" => {
                "发送验证码前需要图片验证，请在官方应用完成验证".into()
            }
            "PML404010001" => "验证码发送过于频繁，请稍后重试".into(),
            _ => format!(
                "验证码验证失败：{} ({result})",
                value["summary"].as_str().unwrap_or_default()
            ),
        });
    }
    if code.is_none() {
        return Ok(None);
    }
    let sid = value["var"]["loginSuccessUrl"]
        .as_str()
        .and_then(|s| url::Url::parse(s).ok())
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "sid")
                .map(|(_, v)| v.into_owned())
        })
        .or_else(|| cookie_value(&cfg.mail_cookies, "Os_SSo_Sid"))
        .or_else(|| cookie_value(&cfg.mail_cookies, "sid"));
    cfg.sms_code.clear();
    Ok(sid)
}
async fn exchange_and_login(
    http: &Client,
    cfg: &mut AccountConfig,
    sid: &str,
) -> Result<(), String> {
    let rmkey = cookie_value(&cfg.mail_cookies, "RMKEY").ok_or("邮箱 Cookie 缺少 RMKEY")?;
    let response = http
        .post("https://smsrebuild1.mail.10086.cn/setting/s")
        .query(&[
            ("func", "umc:getArtifact"),
            ("sid", sid),
            ("cguid", &Utc::now().timestamp_millis().to_string()),
        ])
        .header("Cookie", format!("RMKEY={rmkey}"))
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("User-Agent", "okhttp/4.12.0")
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "邮箱登录交换服务返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let bytes = read_body_limited(response, 1024 * 1024).await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "读取邮箱登录交换响应失败")?;
    let artifact = value["var"]["artifact"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("邮箱登录会话失效，请重新登录")?;
    let body = json!({"clientkey_decrypt":"l3TryM&Q+X7@dzwk)qP","clienttype":"886","cpid":"507","dycpwd":artifact,"extInfo":{"ifOpenAccount":"0"},"loginMode":"0","msisdn":cfg.username,"pintype":"13","secinfo":sha1(&format!("fetion.com.cn:{artifact}")).to_uppercase(),"version":"20250901"});
    let mut headers = HeaderMap::new();
    for (key,value) in [("hcy-cool-flag","1"),("x-huawei-channelsrc","10246600"),("x-sdk-channelsrc",""),("x-mm-source","0"),("x-useragent","android|23116PN5BC|android15|1.2.6|||1440x3200|10246600"),("x-deviceinfo","4|127.0.0.1|5|1.2.6|Xiaomi|23116PN5BC||02-00-00-00-00-00|android 15|1440x3200|android|||"),("content-type","text/plain;charset=UTF-8"),("user-agent","okhttp/3.12.2")] {headers.insert(reqwest::header::HeaderName::from_bytes(key.as_bytes()).map_err(|e|e.to_string())?,value.parse().map_err(|e: reqwest::header::InvalidHeaderValue|e.to_string())?);}
    let layer = encrypted_post(
        http,
        "https://user-njs.yun.139.com/user/thirdlogin",
        &body,
        headers,
        KEY_ONE,
    )
    .await?;
    let hex_data = layer["data"].as_str().ok_or("云盘登录交换失败")?;
    let mut ciphertext = hex::decode(hex_data).map_err(|_| "云盘登录响应编码无效")?;
    if ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return Err("云盘登录响应长度无效".into());
    }
    let key = hex::decode(KEY_TWO).map_err(|e| e.to_string())?;
    let cipher = Aes128::new_from_slice(&key).map_err(|e| e.to_string())?;
    for chunk in ciphertext.chunks_exact_mut(16) {
        cipher.decrypt_block(aes::cipher::generic_array::GenericArray::from_mut_slice(
            chunk,
        ));
    }
    let padding = *ciphertext.last().ok_or("云盘登录解密失败")? as usize;
    if !(1..=16).contains(&padding)
        || ciphertext.len() < padding
        || !ciphertext[ciphertext.len() - padding..]
            .iter()
            .all(|b| *b as usize == padding)
    {
        return Err("云盘登录解密填充无效".into());
    }
    ciphertext.truncate(ciphertext.len() - padding);
    let result: Value = serde_json::from_slice(&ciphertext).map_err(|_| "云盘登录解密响应无效")?;
    let token = result["authToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("云盘登录未返回令牌")?;
    let account = result["account"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("云盘登录未返回账号")?;
    cfg.user_domain_id = result["userDomainId"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    cfg.authorization = STANDARD.encode(format!("pc:{account}:{token}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cbc_roundtrip() {
        let body = json!({"a":1,"中文":"文件"});
        for key in [KEY_ONE, KEY_TWO] {
            let encrypted = encrypt_payload(&body, key).unwrap();
            assert_eq!(decrypt_payload(&encrypted, key).unwrap(), body);
        }
    }
    #[test]
    fn reject_bad_padding() {
        assert!(decrypt_payload(&STANDARD.encode([0u8; 32]), KEY_TWO).is_err());
    }
    #[test]
    fn aes192_matches_go_reference_fixture() {
        // Generated independently with Go crypto/aes + CBC and IV 00..0f.
        let fixture = "AAECAwQFBgcICQoLDA0ODy5rLHJiUogHsqcJbruGanuyUQCMAMbxg8N+ngxjaG1R";
        assert_eq!(
            decrypt_payload(fixture, KEY_ONE).unwrap(),
            json!({"a":"中","n":1})
        );
    }
    #[test]
    fn mail_object_literal_is_data() {
        let response = parse_mail_response(
            "{code:'S_OK',var:{loginSuccessUrl:'https://mail.10086.cn/?sid=test'}}",
        )
        .unwrap();
        assert_eq!(response["code"], "S_OK");
        assert_eq!(
            response["var"]["loginSuccessUrl"],
            "https://mail.10086.cn/?sid=test"
        );
    }
}
